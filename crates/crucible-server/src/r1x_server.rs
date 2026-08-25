//! Product composition for the experimental R1X Configuration -> Play smoke route.
//!
//! R1X still uses the finite captured Play prefix only to establish the first visible-world oracle.
//! Once that prefix has completely drained and both Crucible byte queues are empty, the pre-play
//! lifecycle object is destroyed and the same TCP stream enters a live Play controller. The live
//! controller owns a fresh bounded `ConnectionDriver`; unread kernel bytes remain on the socket, so
//! the handoff copies no packet data and never creates two simultaneous Crucible queues.
//!
//! Live Play liveness is deadline driven. The 26.2 keep-alive state owns no clock or task and its
//! challenge state is committed only after the challenge body enters bounded egress successfully.
//! Consequently backpressure cannot create a phantom pending challenge.

use core::mem::size_of;
use std::convert::Infallible;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use crucible_connection_core::{ConnectionLimits, FrameView};
use crucible_connection_driver::{ConnectionDriver, DriverError, OutboundBatch, TransactionResult};
use crucible_preplay_core::{PrePlayConnection, PrePlayError};
use crucible_preplay_io::{
    ActionBudget, IoOperation, PrePlayIo, PrePlayIoError, PublicationServiceStop,
};
use crucible_session_core::{
    KeepAliveReply, LivenessDecision, LivenessPolicy, LivenessState, SessionPhase,
};
use crucible_target_26_2::{R1xError, Target26_2R1x, Target26_2R1xContext, Target26_2R1xState};

use crate::ServerSessionEpoch;

const FRAME_BODY_LIMIT: usize = 65_536;
const INGRESS_LIMIT: usize = 256 * 1_024;
const EGRESS_LIMIT: usize = 128 * 1_024;
const READ_SCRATCH_BYTES: usize = 16 * 1_024;
const ACTIONS_PER_SERVICE: usize = 4;
const _: () = assert!(ACTIONS_PER_SERVICE > 0);

const KEEP_ALIVE_INTERVAL_MS: u64 = 15_000;
const CLOSED_LISTENER_TIMEOUT_MS: u64 = 15_000;

// R2A experimental wire binding. The exact IDs are independently corroborated against the reviewed
// Minecraft 26.2 `GameProtocols` Play registration order and the real capture. Production target
// admission remains gated on committing the dedicated source-derived Play-liveness contract; R1X is
// still explicitly `production_admitted=false`.
const CLIENTBOUND_KEEP_ALIVE_PACKET_ID: u8 = 0x2c;
const SERVERBOUND_KEEP_ALIVE_PACKET_ID: i32 = 0x1c;
const KEEP_ALIVE_BODY_BYTES: usize = 1 + size_of::<i64>();
const _: () = assert!(CLIENTBOUND_KEEP_ALIVE_PACKET_ID < 0x80);

/// Why one R1X development connection ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1xConnectionExit {
    /// The target committed a terminal session state and all queued output was flushed before Play.
    SessionClosed,
    /// The peer closed cleanly before the live-Play handoff.
    PeerEof,
    /// The peer closed after live liveness had begun.
    LivePeerEof {
        /// Exact number of valid keep-alive replies committed before EOF.
        accepted_keep_alives: u64,
        /// Last source-compatible smoothed latency value.
        latency_ms: i32,
    },
    /// A live keep-alive challenge remained unacknowledged through its next deadline.
    LiveKeepAliveTimedOut {
        /// Exact number of valid replies committed before timeout.
        accepted_keep_alives: u64,
        /// Last source-compatible smoothed latency value.
        latency_ms: i32,
    },
    /// The client sent a malformed, unexpected, or wrong-ID keep-alive response.
    LiveInvalidKeepAlive {
        /// Exact number of valid replies committed before rejection.
        accepted_keep_alives: u64,
        /// Last source-compatible smoothed latency value.
        latency_ms: i32,
    },
    /// The local monotonic-time domain exceeded the admitted signed-64-bit wire domain.
    LiveClockInvalid,
    /// A closed-listener liveness deadline fired unexpectedly in this development composition.
    LiveClosedTimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveDisposition {
    Ignored,
    KeepAliveAccepted { latency_ms: i32 },
    InvalidKeepAlive,
    ClockInvalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveInboundAction {
    next_liveness: LivenessState,
    disposition: LiveDisposition,
}

impl OutboundBatch for LiveInboundAction {
    type Body = [u8; 0];

    fn outbound_frames(&self) -> &[Self::Body] {
        &[]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveLivenessService {
    Idle,
    ChallengeQueued,
    KeepAliveTimedOut,
    ClosedTimedOut,
    ClockInvalid,
}

/// Drives one TCP transport through Login, real Configuration, the selected finite R1X Play replay,
/// then live Crucible-owned keep-alive traffic.
///
/// The replay phase uses the existing bounded pre-play publisher. Handoff is legal only after Play
/// has been entered, the selected replay is complete, and both pre-play ingress and egress are empty.
/// A fresh live driver then owns the same bounds over the same TCP stream; no raw framing path or
/// second outbound queue exists.
///
/// # Errors
///
/// Returns the existing fail-closed bounded I/O/target error shape. Live driver buffer failures are
/// mapped back into the same generic connection error variants, and transport failures retain their
/// operation/error-kind evidence.
pub fn serve_r1x_blocking_transport(
    transport: &mut TcpStream,
    session_epoch: ServerSessionEpoch,
    context: &Target26_2R1xContext,
) -> Result<R1xConnectionExit, PrePlayIoError<R1xError>> {
    let target_state = Target26_2R1xState::with_login_session_uuid(session_epoch.into_bytes());
    let connection = PrePlayConnection::<Target26_2R1x>::with_target_state(limits(), target_state);
    let mut io = PrePlayIo::from_connection(connection, read_scratch_bytes());
    let budget = action_budget();

    loop {
        if ready_for_live_handoff(&io) {
            break;
        }

        let report = io.service_once_with_publication(transport, context, budget)?;
        match report.stop {
            PublicationServiceStop::SessionClosed => return Ok(R1xConnectionExit::SessionClosed),
            PublicationServiceStop::PeerEof => return Ok(R1xConnectionExit::PeerEof),
            PublicationServiceStop::InputPending
            | PublicationServiceStop::OutputPending
            | PublicationServiceStop::ActionBudgetExhausted
            | PublicationServiceStop::PublicationProgress => {}
        }
    }

    // The handoff invariant is deliberately stronger than merely reaching Play. Dropping `io` here
    // destroys an empty pre-play driver before the live driver is created, so no queue is duplicated
    // and no buffered user-space bytes can be lost.
    debug_assert!(ready_for_live_handoff(&io));
    drop(io);
    serve_live_play_liveness(transport)
}

fn ready_for_live_handoff(io: &PrePlayIo<Target26_2R1x>) -> bool {
    let connection = io.connection();
    connection.phase() == SessionPhase::Play
        && connection.target_state().replay_complete()
        && connection.buffered_ingress() == 0
        && connection.queued_egress() == 0
}

fn serve_live_play_liveness(
    transport: &mut TcpStream,
) -> Result<R1xConnectionExit, PrePlayIoError<R1xError>> {
    let policy = liveness_policy();
    let origin = Instant::now();
    let mut liveness = LivenessState::new(0, 0)
        .expect("zero is inside the admitted signed-64-bit monotonic time domain");
    let mut driver = ConnectionDriver::new(limits());
    let mut read_scratch = vec![0_u8; READ_SCRATCH_BYTES].into_boxed_slice();
    let mut accepted_keep_alives = 0_u64;

    loop {
        let Some(now_ms) = elapsed_millis(origin) else {
            return Ok(R1xConnectionExit::LiveClockInvalid);
        };

        for _ in 0..ACTIONS_PER_SERVICE {
            let Some(disposition) = process_one_live_inbound(&mut driver, &mut liveness, now_ms)?
            else {
                break;
            };
            match disposition {
                LiveDisposition::Ignored => {}
                LiveDisposition::KeepAliveAccepted { .. } => {
                    accepted_keep_alives = accepted_keep_alives
                        .checked_add(1)
                        .ok_or(PrePlayIoError::AccountingOverflow)?;
                }
                LiveDisposition::InvalidKeepAlive => {
                    return Ok(R1xConnectionExit::LiveInvalidKeepAlive {
                        accepted_keep_alives,
                        latency_ms: liveness.latency_ms(),
                    });
                }
                LiveDisposition::ClockInvalid => {
                    return Ok(R1xConnectionExit::LiveClockInvalid);
                }
            }
        }

        if driver.queued_egress() != 0 {
            write_live_once(transport, &mut driver)?;
            if driver.queued_egress() != 0 {
                continue;
            }
        }

        let Some(now_ms) = elapsed_millis(origin) else {
            return Ok(R1xConnectionExit::LiveClockInvalid);
        };
        match service_live_liveness(&mut driver, &mut liveness, now_ms, policy)? {
            LiveLivenessService::Idle => {}
            LiveLivenessService::ChallengeQueued => {
                write_live_once(transport, &mut driver)?;
                if driver.queued_egress() != 0 {
                    continue;
                }
            }
            LiveLivenessService::KeepAliveTimedOut => {
                return Ok(R1xConnectionExit::LiveKeepAliveTimedOut {
                    accepted_keep_alives,
                    latency_ms: liveness.latency_ms(),
                });
            }
            LiveLivenessService::ClosedTimedOut => {
                return Ok(R1xConnectionExit::LiveClosedTimedOut);
            }
            LiveLivenessService::ClockInvalid => {
                return Ok(R1xConnectionExit::LiveClockInvalid);
            }
        }

        let Some(now_ms) = elapsed_millis(origin) else {
            return Ok(R1xConnectionExit::LiveClockInvalid);
        };
        let deadline_ms = liveness.next_deadline_ms(policy);
        let wait_ms = deadline_ms.saturating_sub(now_ms).max(1);
        transport
            .set_read_timeout(Some(Duration::from_millis(wait_ms)))
            .map_err(|error| live_io_error(IoOperation::Read, &error))?;

        match transport.read(&mut read_scratch) {
            Ok(0) => {
                if driver.buffered_ingress() != 0 {
                    return Err(PrePlayIoError::TruncatedEof {
                        buffered_ingress: driver.buffered_ingress(),
                    });
                }
                return Ok(R1xConnectionExit::LivePeerEof {
                    accepted_keep_alives,
                    latency_ms: liveness.latency_ms(),
                });
            }
            Ok(read) => driver
                .ingest::<Infallible>(&read_scratch[..read])
                .map_err(|error| map_live_driver_error(&error))?,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                // A timeout is the blocking-development adapter's deadline wake. The next loop first
                // processes any already-buffered complete input, then services the exact deadline.
            }
            Err(error) => return Err(live_io_error(IoOperation::Read, &error)),
        }
    }
}

fn process_one_live_inbound(
    driver: &mut ConnectionDriver,
    liveness: &mut LivenessState,
    now_ms: u64,
) -> Result<Option<LiveDisposition>, PrePlayIoError<R1xError>> {
    let current = *liveness;
    let transaction = driver
        .process_one_transactional::<Infallible, LiveInboundAction, _>(|frame| {
            Ok(decode_live_frame(current, now_ms, frame))
        })
        .map_err(|error| map_live_driver_error(&error))?;

    match transaction {
        TransactionResult::Incomplete => Ok(None),
        TransactionResult::Committed(action) => {
            *liveness = action.next_liveness;
            Ok(Some(action.disposition))
        }
    }
}

fn decode_live_frame(
    current: LivenessState,
    now_ms: u64,
    frame: FrameView<'_>,
) -> LiveInboundAction {
    if frame.packet_id() != SERVERBOUND_KEEP_ALIVE_PACKET_ID {
        return LiveInboundAction {
            next_liveness: current,
            disposition: LiveDisposition::Ignored,
        };
    }

    let Ok(payload) = <&[u8; size_of::<i64>()]>::try_from(frame.payload()) else {
        return LiveInboundAction {
            next_liveness: current,
            disposition: LiveDisposition::InvalidKeepAlive,
        };
    };
    let id = i64::from_be_bytes(*payload);
    let mut candidate = current;
    let disposition = match candidate.receive_keep_alive(now_ms, id) {
        Ok(KeepAliveReply::Accepted { latency_ms }) => {
            LiveDisposition::KeepAliveAccepted { latency_ms }
        }
        Ok(KeepAliveReply::Rejected) => LiveDisposition::InvalidKeepAlive,
        Err(_) => LiveDisposition::ClockInvalid,
    };
    let next_liveness = if matches!(disposition, LiveDisposition::KeepAliveAccepted { .. }) {
        candidate
    } else {
        current
    };
    LiveInboundAction {
        next_liveness,
        disposition,
    }
}

fn service_live_liveness(
    driver: &mut ConnectionDriver,
    liveness: &mut LivenessState,
    now_ms: u64,
    policy: LivenessPolicy,
) -> Result<LiveLivenessService, PrePlayIoError<R1xError>> {
    let mut candidate = *liveness;
    match candidate.service(now_ms, policy) {
        Ok(LivenessDecision::Idle) => Ok(LiveLivenessService::Idle),
        Ok(LivenessDecision::IssueChallenge { id }) => {
            let body = keep_alive_body(id);
            driver
                .queue_frame::<Infallible>(&body)
                .map_err(|error| map_live_driver_error(&error))?;
            *liveness = candidate;
            Ok(LiveLivenessService::ChallengeQueued)
        }
        Ok(LivenessDecision::KeepAliveTimedOut) => Ok(LiveLivenessService::KeepAliveTimedOut),
        Ok(LivenessDecision::ClosedTimedOut) => Ok(LiveLivenessService::ClosedTimedOut),
        Err(_) => Ok(LiveLivenessService::ClockInvalid),
    }
}

fn keep_alive_body(id: i64) -> [u8; KEEP_ALIVE_BODY_BYTES] {
    let mut body = [0_u8; KEEP_ALIVE_BODY_BYTES];
    body[0] = CLIENTBOUND_KEEP_ALIVE_PACKET_ID;
    body[1..].copy_from_slice(&id.to_be_bytes());
    body
}

#[cfg(test)]
fn serverbound_keep_alive_frame(id: i64) -> [u8; KEEP_ALIVE_BODY_BYTES + 1] {
    let mut frame = [0_u8; KEEP_ALIVE_BODY_BYTES + 1];
    frame[0] = u8::try_from(KEEP_ALIVE_BODY_BYTES).expect("keep-alive body length fits one byte");
    frame[1] = u8::try_from(SERVERBOUND_KEEP_ALIVE_PACKET_ID)
        .expect("serverbound keep-alive packet id fits one byte");
    frame[2..].copy_from_slice(&id.to_be_bytes());
    frame
}

fn write_live_once(
    transport: &mut TcpStream,
    driver: &mut ConnectionDriver,
) -> Result<(), PrePlayIoError<R1xError>> {
    let pending = driver.pending_egress().len();
    if pending == 0 {
        return Ok(());
    }

    let write_result = {
        let bytes = driver.pending_egress();
        transport.write(bytes)
    };
    match write_result {
        Ok(0) => Err(PrePlayIoError::ZeroWrite { pending }),
        Ok(written) => driver
            .consume_written::<Infallible>(written)
            .map_err(|error| map_live_driver_error(&error)),
        Err(error) => Err(live_io_error(IoOperation::Write, &error)),
    }
}

fn elapsed_millis(origin: Instant) -> Option<u64> {
    u64::try_from(origin.elapsed().as_millis()).ok()
}

fn liveness_policy() -> LivenessPolicy {
    LivenessPolicy::new(KEEP_ALIVE_INTERVAL_MS, CLOSED_LISTENER_TIMEOUT_MS)
        .expect("Minecraft 26.2 liveness intervals are positive and representable")
}

fn map_live_driver_error(error: &DriverError<Infallible>) -> PrePlayIoError<R1xError> {
    match error {
        DriverError::Buffer(error) => PrePlayIoError::Connection(PrePlayError::Buffer(*error)),
        DriverError::Handler(never) => match *never {},
        DriverError::RollbackFailed {
            operation,
            rollback,
        } => PrePlayIoError::Connection(PrePlayError::RollbackFailed {
            operation: *operation,
            rollback: *rollback,
        }),
        DriverError::AccountingOverflow => PrePlayIoError::AccountingOverflow,
    }
}

fn live_io_error(operation: IoOperation, error: &io::Error) -> PrePlayIoError<R1xError> {
    PrePlayIoError::Io {
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(FRAME_BODY_LIMIT, INGRESS_LIMIT, EGRESS_LIMIT)
        .expect("R1X product limits are positive and coherent")
}

fn read_scratch_bytes() -> NonZeroUsize {
    NonZeroUsize::new(READ_SCRATCH_BYTES).expect("R1X read scratch is positive")
}

fn action_budget() -> ActionBudget {
    ActionBudget::new(ACTIONS_PER_SERVICE).expect("R1X action budget is positive")
}

#[cfg(test)]
mod tests {
    use super::{
        EGRESS_LIMIT, FRAME_BODY_LIMIT, INGRESS_LIMIT, LiveDisposition, LiveLivenessService,
        SERVERBOUND_KEEP_ALIVE_PACKET_ID, keep_alive_body, limits, liveness_policy,
        process_one_live_inbound, serverbound_keep_alive_frame, service_live_liveness,
    };
    use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};
    use crucible_connection_driver::ConnectionDriver;
    use crucible_preplay_core::PrePlayError;
    use crucible_preplay_io::PrePlayIoError;
    use crucible_session_core::LivenessState;

    fn test_limits(egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(64, 256, egress).expect("coherent live test limits")
    }

    #[test]
    fn r1x_limits_are_finite_and_cover_one_maximal_body() {
        let limits = limits();
        assert_eq!(limits.max_frame_body_len(), FRAME_BODY_LIMIT);
        assert_eq!(limits.max_ingress_buffered(), INGRESS_LIMIT);
        assert_eq!(limits.max_egress_queued(), EGRESS_LIMIT);
    }

    #[test]
    fn keep_alive_body_and_frame_are_exact_big_endian_i64() {
        let id = 0x0102_0304_0506_0708_i64;
        assert_eq!(
            keep_alive_body(id),
            [0x2c, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
        assert_eq!(
            serverbound_keep_alive_frame(id),
            [0x09, 0x1c, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
        assert_eq!(SERVERBOUND_KEEP_ALIVE_PACKET_ID, 0x1c);
    }

    #[test]
    fn egress_backpressure_cannot_commit_a_phantom_challenge() {
        let mut driver = ConnectionDriver::new(test_limits(1));
        let mut liveness = LivenessState::new(0, 7).expect("valid liveness start");
        let before = liveness;

        assert!(matches!(
            service_live_liveness(&mut driver, &mut liveness, 15_000, liveness_policy()),
            Err(PrePlayIoError::Connection(PrePlayError::Buffer(
                ConnectionBufferError::EgressLimitExceeded { .. }
            )))
        ));
        assert_eq!(liveness, before);
        assert!(!liveness.keep_alive_pending());
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn successful_challenge_commits_only_after_bounded_frame_admission() {
        let mut driver = ConnectionDriver::new(test_limits(64));
        let mut liveness = LivenessState::new(0, 0).expect("valid liveness start");

        assert_eq!(
            service_live_liveness(&mut driver, &mut liveness, 15_000, liveness_policy()),
            Ok(LiveLivenessService::ChallengeQueued)
        );
        assert_eq!(liveness.pending_challenge(), Some(15_000));
        assert_eq!(
            driver.pending_egress(),
            &[0x09, 0x2c, 0, 0, 0, 0, 0, 0x3a, 0x98]
        );
    }

    #[test]
    fn exact_reply_is_transactional_and_updates_latency() {
        let mut driver = ConnectionDriver::new(test_limits(64));
        let mut liveness = LivenessState::new(0, 40).expect("valid liveness start");
        service_live_liveness(&mut driver, &mut liveness, 15_000, liveness_policy())
            .expect("challenge queues");
        let queued = driver.queued_egress();
        driver
            .consume_written::<()>(queued)
            .expect("drain synthetic challenge");
        driver
            .ingest::<()>(&serverbound_keep_alive_frame(15_000))
            .expect("response frame fits");

        assert_eq!(
            process_one_live_inbound(&mut driver, &mut liveness, 15_120),
            Ok(Some(LiveDisposition::KeepAliveAccepted { latency_ms: 60 }))
        );
        assert_eq!(liveness.latency_ms(), 60);
        assert!(!liveness.keep_alive_pending());
        assert_eq!(driver.buffered_ingress(), 0);
    }

    #[test]
    fn malformed_or_wrong_keep_alive_is_consumed_as_rejection_without_state_change() {
        let mut driver = ConnectionDriver::new(test_limits(128));
        let mut liveness = LivenessState::new(0, 5).expect("valid liveness start");
        service_live_liveness(&mut driver, &mut liveness, 15_000, liveness_policy())
            .expect("challenge queues");
        let queued = driver.queued_egress();
        driver
            .consume_written::<()>(queued)
            .expect("drain synthetic challenge");
        let pending = liveness;

        driver
            .ingest::<()>(&serverbound_keep_alive_frame(14_999))
            .expect("wrong response fits");
        assert_eq!(
            process_one_live_inbound(&mut driver, &mut liveness, 15_010),
            Ok(Some(LiveDisposition::InvalidKeepAlive))
        );
        assert_eq!(liveness, pending);
        assert_eq!(driver.buffered_ingress(), 0);

        // Body length 1: packet id only, no required i64 payload.
        driver
            .ingest::<()>(&[0x01, 0x1c])
            .expect("malformed response framing fits");
        assert_eq!(
            process_one_live_inbound(&mut driver, &mut liveness, 15_020),
            Ok(Some(LiveDisposition::InvalidKeepAlive))
        );
        assert_eq!(liveness, pending);
    }

    #[test]
    fn ten_fake_time_cycles_share_one_driver_without_polling_or_allocating_actions() {
        let mut driver = ConnectionDriver::new(test_limits(256));
        let mut liveness = LivenessState::new(0, 0).expect("valid liveness start");
        let mut accepted = 0_u64;

        for cycle in 1_u64..=10 {
            let issue_ms = cycle * 15_000;
            assert_eq!(
                service_live_liveness(&mut driver, &mut liveness, issue_ms, liveness_policy()),
                Ok(LiveLivenessService::ChallengeQueued),
                "cycle={cycle}"
            );
            let challenge = liveness.pending_challenge().expect("challenge is pending");
            let expected_latency = (liveness.latency_ms() * 3 + 100) / 4;
            let queued = driver.queued_egress();
            driver
                .consume_written::<()>(queued)
                .expect("drain challenge before next cycle");

            let frame = serverbound_keep_alive_frame(challenge);
            for fragment in frame.chunks(3) {
                driver.ingest::<()>(fragment).expect("fragment fits");
            }
            assert_eq!(
                process_one_live_inbound(&mut driver, &mut liveness, issue_ms + 100),
                Ok(Some(LiveDisposition::KeepAliveAccepted {
                    latency_ms: expected_latency,
                })),
                "cycle={cycle}"
            );
            accepted += 1;
        }

        assert_eq!(accepted, 10);
        assert_eq!(driver.buffered_ingress(), 0);
        assert_eq!(driver.queued_egress(), 0);
        assert_eq!(liveness.latency_ms(), 93);
    }
}
