//! Replay-free R2B Configuration -> Play entry composition.
//!
//! This development composition intentionally reuses the source-admitted R1X Configuration target
//! with an empty captured Play image. At the fully drained Play boundary it consumes the pre-play
//! owner and transfers the exact existing bounded driver plus retained read scratch into continuing
//! Play; no connection-buffer allocation is destroyed/recreated at the phase transition. That same
//! driver publishes the canonical [`PreparedR2bPlan`] and continues teleport/liveness control at the
//! explicit `WorldProjection` seam.
//!
//! Captured Play bodies are rejected before transport I/O. R1X is used only as the temporary
//! Configuration carrier; every non-world Play body comes from replay-free R2B semantic projection.

use std::convert::Infallible;
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};
use crucible_connection_driver::{ConnectionDriver, DriverError, OutboundBatch, TransactionResult};
use crucible_packet_core::{PacketCodecError, PacketWriter};
use crucible_preplay_core::{PrePlayConnection, PrePlayError};
use crucible_preplay_io::{
    ActionBudget, IoOperation, PrePlayIo, PrePlayIoError, PublicationServiceStop,
};
use crucible_publication_core::{
    StagedPublicationCursor, StagedPublicationStep, publish_staged_plan_one,
};
use crucible_session_core::{
    KeepAliveReply, LivenessDecision, LivenessError, LivenessState, SessionPhase,
};
use crucible_target_26_2::{
    R1xError, Target26_2R1x, Target26_2R1xContext, Target26_2R1xState,
    play_liveness::{
        PLAY_LIVENESS_POLICY, PlayLivenessCodecError, decode_serverbound_keep_alive,
        encode_clientbound_keep_alive,
    },
    r2b::{
        FreshR2bBootstrapSnapshot, PLAY_PUBLICATION_STAGES, PlayBootstrapImage26_2,
        PlayBootstrapStage, PrepareR2bError, PreparedR2bPlan, SELECTED_DYNAMIC_ARENA_CAPACITY,
        TeleportAckResult, TeleportTransaction, decode_serverbound_teleport_ack,
        stage_for_publication_index,
    },
};

use crate::ServerSessionEpoch;

const FRAME_BODY_LIMIT: usize = 65_536;
const INGRESS_LIMIT: usize = 256 * 1_024;
const EGRESS_LIMIT: usize = 128 * 1_024;
const READ_SCRATCH_BYTES: usize = 16 * 1_024;
const PREPLAY_ACTIONS_PER_SERVICE: usize = 4;
const PREPARE_SCRATCH_BYTES: usize = 4 * 1_024;
const INITIAL_LIVENESS: LivenessState = match LivenessState::new(0, 0) {
    Ok(state) => state,
    Err(_) => panic!("zero must remain inside the signed-64-bit monotone liveness domain"),
};
const _: () = assert!(PREPLAY_ACTIONS_PER_SERVICE > 0);

/// Fail-closed R2B server-composition error.
#[derive(Debug)]
pub enum R2bServerError {
    /// The supplied R1X context still contains captured Play bodies.
    CapturedPlayNotEmpty {
        /// Captured body count that would otherwise be replayed.
        frames: usize,
        /// Aggregate captured Play body bytes.
        body_bytes: usize,
    },
    /// The phase handoff was attempted with userspace bytes still owned by pre-play.
    HandoffNotDrained {
        /// Active ingress bytes that would be lost by moving owners.
        buffered_ingress: usize,
        /// Queued egress bytes that would be lost by moving owners.
        queued_egress: usize,
    },
    /// Source-admitted pre-play I/O/target failure.
    PrePlay(PrePlayIoError<R1xError>),
    /// Replay-free semantic bootstrap preparation failed.
    Prepare(PrepareR2bError),
    /// The finite staged publication did not reach its explicit complete observation within the
    /// exact body-plus-stage action bound.
    PublicationDidNotConverge,
}

impl From<PrePlayIoError<R1xError>> for R2bServerError {
    fn from(value: PrePlayIoError<R1xError>) -> Self {
        Self::PrePlay(value)
    }
}

impl From<PrepareR2bError> for R2bServerError {
    fn from(value: PrepareR2bError) -> Self {
        Self::Prepare(value)
    }
}

/// Fail-closed continuing-Play control-slice error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R2bPlayError {
    /// Bounded ingress/egress or frame law rejected the operation.
    Buffer(ConnectionBufferError),
    /// The target-owned teleport acknowledgement codec rejected a claimed packet.
    TeleportCodec(PacketCodecError),
    /// The target-owned keep-alive codec rejected a claimed packet.
    KeepAliveCodec(PlayLivenessCodecError),
    /// Caller-supplied monotone time violated the admitted liveness domain.
    Liveness(LivenessError),
    /// An impossible ingress-commit failure was followed by failed egress rollback.
    RollbackFailed {
        /// Failure consuming the already-admitted inbound frame.
        operation: ConnectionBufferError,
        /// Failure restoring the prior egress tail.
        rollback: ConnectionBufferError,
    },
    /// Driver accounting overflowed.
    AccountingOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayInboundDecodeError {
    TeleportCodec(PacketCodecError),
    KeepAliveCodec(PlayLivenessCodecError),
    Liveness(LivenessError),
    Unclaimed(i32),
}

/// One R2B-owned continuing-Play semantic event committed from the existing driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R2bPlayInbound {
    /// Client teleport confirmation was consumed and applied to the pending transaction.
    TeleportAcknowledgement(TeleportAckResult),
    /// A keep-alive response matched the outstanding challenge and updated smoothed latency.
    KeepAliveAccepted {
        /// Source-compatible smoothed latency after this response.
        latency_ms: i32,
    },
    /// A syntactically valid keep-alive response did not match an outstanding challenge.
    KeepAliveRejected {
        /// Rejected wire challenge identifier.
        id: i64,
    },
}

/// Result of one bounded R2B continuing-Play control service opportunity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R2bPlayProcess {
    /// No complete frame is currently buffered.
    Incomplete,
    /// One R2B-owned frame committed.
    Committed(R2bPlayInbound),
    /// A complete frame belongs to another Play slice and remains entirely unconsumed.
    Unclaimed {
        /// Target-decoded packet identity at the front of ingress.
        packet_id: i32,
    },
}

/// Result of servicing the source-admitted continuing-Play liveness deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R2bLivenessProcess {
    /// No externally visible liveness action is due.
    Idle,
    /// One fixed-size keep-alive challenge entered the existing bounded egress successfully.
    ChallengeQueued {
        /// Exact challenge identifier carried on the wire.
        id: i64,
    },
    /// The previous challenge remained pending through its next keep-alive deadline.
    KeepAliveTimedOut,
    /// Closed-listener linger timeout fired. R2B does not currently close the listener itself.
    ClosedTimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PreparedPlayInbound {
    teleport: TeleportTransaction,
    liveness: LivenessState,
    event: R2bPlayInbound,
}

impl OutboundBatch for PreparedPlayInbound {
    type Body = [u8; 0];

    fn outbound_frames(&self) -> &[Self::Body] {
        &[]
    }
}

/// Connection-owned state handed to the world/live-Play owner after R2B network bootstrap.
///
/// `driver` and `read_scratch` are the exact allocations used by pre-play, moved rather than
/// recreated once both userspace queues were proven empty. The driver then admits every R2B frame
/// and remains the sole continuing queue. Teleport acknowledgement and source-admitted keep-alive
/// liveness both continue through this same driver with no packet registry or auxiliary queue.
pub struct R2bPlaySession {
    pub(crate) driver: ConnectionDriver,
    pub(crate) read_scratch: Box<[u8]>,
    pub(crate) teleport: TeleportTransaction,
    pub(crate) liveness: LivenessState,
}

impl R2bPlaySession {
    /// Outstanding teleport transaction carried into continuing Play processing.
    #[must_use]
    pub const fn teleport_transaction(&self) -> TeleportTransaction {
        self.teleport
    }

    /// Source-compatible smoothed keep-alive latency.
    #[must_use]
    pub const fn latency_ms(&self) -> i32 {
        self.liveness.latency_ms()
    }

    /// Outstanding keep-alive challenge, when one exists.
    #[must_use]
    pub const fn pending_keep_alive(&self) -> Option<i64> {
        self.liveness.pending_challenge()
    }

    /// Retained transport-read scratch transferred from pre-play without reallocation.
    #[must_use]
    pub fn read_scratch_bytes(&self) -> usize {
        self.read_scratch.len()
    }

    /// Userspace ingress bytes already buffered in the continuing Play driver.
    #[must_use]
    pub fn buffered_ingress(&self) -> usize {
        self.driver.buffered_ingress()
    }

    /// Userspace egress bytes still queued in the continuing Play driver.
    #[must_use]
    pub fn queued_egress(&self) -> usize {
        self.driver.queued_egress()
    }

    /// Contiguous encoded Play bytes ready for the transport writer.
    #[must_use]
    pub fn pending_play_egress(&self) -> &[u8] {
        self.driver.pending_egress()
    }

    /// Appends one arbitrary post-R2B transport fragment to the same bounded ingress allocation.
    ///
    /// # Errors
    ///
    /// Returns the fail-closed connection-buffer error without changing the logical active stream
    /// on rejection.
    pub fn ingest_play(&mut self, incoming: &[u8]) -> Result<(), R2bPlayError> {
        self.driver
            .ingest::<PlayInboundDecodeError>(incoming)
            .map_err(|error| map_play_driver_error(&error))
    }

    /// Acknowledges bytes successfully written by the continuing Play transport owner.
    ///
    /// # Errors
    ///
    /// Rejects an impossible write count instead of clamping it.
    pub fn consume_play_written(&mut self, bytes: usize) -> Result<(), R2bPlayError> {
        self.driver
            .consume_written::<PlayInboundDecodeError>(bytes)
            .map_err(|error| map_play_driver_error(&error))
    }

    /// Tries to commit exactly one R2B-owned continuing-Play control frame.
    ///
    /// `now_ms` is monotone milliseconds since this session crossed the `WorldProjection` handoff.
    /// Teleport and liveness state are decoded into candidate copies. The driver consumes ingress
    /// first; only a committed transaction is then adopted into live state. Other Play packet
    /// identities are reported as `Unclaimed` and remain byte-for-byte at the front of ingress for
    /// the world/gameplay owner.
    ///
    /// # Errors
    ///
    /// Returns malformed claimed-packet, invalid monotone time, bounded-buffer, rollback or
    /// accounting failures without committing candidate teleport/liveness state.
    pub fn process_one_play_control(
        &mut self,
        now_ms: u64,
    ) -> Result<R2bPlayProcess, R2bPlayError> {
        let teleport = self.teleport;
        let liveness = self.liveness;
        let transaction = self.driver.process_one_transactional(|frame| {
            if let Some(received) = decode_serverbound_teleport_ack(frame)
                .map_err(PlayInboundDecodeError::TeleportCodec)?
            {
                let mut candidate = teleport;
                let result = candidate.acknowledge(received);
                return Ok(PreparedPlayInbound {
                    teleport: candidate,
                    liveness,
                    event: R2bPlayInbound::TeleportAcknowledgement(result),
                });
            }

            let id = decode_serverbound_keep_alive(frame)
                .map_err(PlayInboundDecodeError::KeepAliveCodec)?
                .ok_or(PlayInboundDecodeError::Unclaimed(frame.packet_id()))?;
            let mut candidate = liveness;
            let (next_liveness, event) = match candidate
                .receive_keep_alive(now_ms, id)
                .map_err(PlayInboundDecodeError::Liveness)?
            {
                KeepAliveReply::Accepted { latency_ms } => {
                    (candidate, R2bPlayInbound::KeepAliveAccepted { latency_ms })
                }
                KeepAliveReply::Rejected => (liveness, R2bPlayInbound::KeepAliveRejected { id }),
            };
            Ok(PreparedPlayInbound {
                teleport,
                liveness: next_liveness,
                event,
            })
        });

        match transaction {
            Ok(TransactionResult::Incomplete) => Ok(R2bPlayProcess::Incomplete),
            Ok(TransactionResult::Committed(action)) => {
                self.teleport = action.teleport;
                self.liveness = action.liveness;
                Ok(R2bPlayProcess::Committed(action.event))
            }
            Err(DriverError::Handler(PlayInboundDecodeError::Unclaimed(packet_id))) => {
                Ok(R2bPlayProcess::Unclaimed { packet_id })
            }
            Err(error) => Err(map_play_driver_error(&error)),
        }
    }

    /// Services one source-admitted keep-alive deadline through the existing bounded egress.
    ///
    /// `now_ms` uses the same handoff-relative monotone domain as [`Self::process_one_play_control`].
    /// Challenge state is prepared on a copy, the fixed nine-byte 26.2 body is queued, and only then
    /// is the candidate liveness state adopted. Egress backpressure therefore cannot create a
    /// phantom pending challenge.
    ///
    /// # Errors
    ///
    /// Returns invalid monotone time or bounded-egress failure without committing candidate
    /// liveness state.
    pub fn service_play_liveness(
        &mut self,
        now_ms: u64,
    ) -> Result<R2bLivenessProcess, R2bPlayError> {
        let mut candidate = self.liveness;
        match candidate
            .service(now_ms, PLAY_LIVENESS_POLICY)
            .map_err(R2bPlayError::Liveness)?
        {
            LivenessDecision::Idle => Ok(R2bLivenessProcess::Idle),
            LivenessDecision::IssueChallenge { id } => {
                let body = encode_clientbound_keep_alive(id);
                self.driver
                    .queue_frame::<PlayInboundDecodeError>(&body)
                    .map_err(|error| map_play_driver_error(&error))?;
                self.liveness = candidate;
                Ok(R2bLivenessProcess::ChallengeQueued { id })
            }
            LivenessDecision::KeepAliveTimedOut => Ok(R2bLivenessProcess::KeepAliveTimedOut),
            LivenessDecision::ClosedTimedOut => Ok(R2bLivenessProcess::ClosedTimedOut),
        }
    }
}

impl core::fmt::Debug for R2bPlaySession {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("R2bPlaySession")
            .field("buffered_ingress", &self.driver.buffered_ingress())
            .field("queued_egress", &self.driver.queued_egress())
            .field("read_scratch_bytes", &self.read_scratch.len())
            .field("teleport", &self.teleport)
            .field("liveness", &self.liveness)
            .finish()
    }
}

/// Result of attempting the R2B entry boundary on one connection.
///
/// The successful variant intentionally stores the session inline. Boxing it solely to shrink this
/// transient return enum would add a heap allocation on every successful join after we explicitly
/// eliminated the Configuration -> Play driver/read-buffer allocation churn.
#[allow(
    clippy::large_enum_variant,
    reason = "boxing the successful continuing session would add a per-join heap allocation; this transient outcome is immediately destructured"
)]
#[derive(Debug)]
pub enum R2bEntryOutcome {
    /// The target committed a terminal session before reaching Play and all queued output drained.
    SessionClosed,
    /// The peer closed cleanly before the replay-free Play handoff.
    PeerEof,
    /// R2B network bootstrap is flushed and ownership is ready to pass to R2C/live Play.
    WorldProjectionReady(R2bPlaySession),
}

/// Drives one transport through Login, source-admitted Configuration, zero captured Play bodies and
/// the complete replay-free R2B network bootstrap.
///
/// Once the generic phase is Play, empty-Play publication is committed and both userspace queues are
/// empty, the pre-play owners are consumed. The exact existing bounded driver and retained read
/// scratch move into Play; there is no driver, ingress, egress or transport-scratch reallocation at
/// the phase boundary. R2B then publishes through that driver and returns the same owner at
/// `WorldProjection`.
///
/// # Errors
///
/// Fails before I/O when captured Play is non-empty, rejects any impossible non-drained ownership
/// transfer, then preserves the existing fail-closed pre-play/driver errors and transactional R2B
/// preparation errors.
pub fn enter_r2b_play_blocking_transport<RW>(
    transport: &mut RW,
    session_epoch: ServerSessionEpoch,
    preplay_context: &Target26_2R1xContext,
    bootstrap_image: &PlayBootstrapImage26_2,
    snapshot: FreshR2bBootstrapSnapshot<'_>,
) -> Result<R2bEntryOutcome, R2bServerError>
where
    RW: Read + Write + ?Sized,
{
    if preplay_context.play_frame_count() != 0 {
        return Err(R2bServerError::CapturedPlayNotEmpty {
            frames: preplay_context.play_frame_count(),
            body_bytes: preplay_context.play_body_bytes(),
        });
    }

    let target_state = Target26_2R1xState::with_login_session_uuid(session_epoch.into_bytes());
    let connection = PrePlayConnection::<Target26_2R1x>::with_target_state(limits(), target_state);
    let mut io = PrePlayIo::from_connection(connection, read_scratch_bytes());
    let budget = action_budget();

    loop {
        if ready_for_r2b_handoff(&io) {
            break;
        }

        let report = io.service_once_with_publication(transport, preplay_context, budget)?;
        match report.stop {
            PublicationServiceStop::SessionClosed => return Ok(R2bEntryOutcome::SessionClosed),
            PublicationServiceStop::PeerEof => return Ok(R2bEntryOutcome::PeerEof),
            PublicationServiceStop::InputPending
            | PublicationServiceStop::OutputPending
            | PublicationServiceStop::ActionBudgetExhausted
            | PublicationServiceStop::PublicationProgress => {}
        }
    }

    debug_assert!(ready_for_r2b_handoff(&io));
    let (connection, read_scratch, peer_eof) = io.into_parts();
    if peer_eof {
        return Ok(R2bEntryOutcome::PeerEof);
    }
    let mut driver = match connection.try_into_drained_driver() {
        Ok(driver) => driver,
        Err(connection) => {
            return Err(R2bServerError::HandoffNotDrained {
                buffered_ingress: connection.buffered_ingress(),
                queued_egress: connection.queued_egress(),
            });
        }
    };

    let mut scratch = PacketWriter::new(PREPARE_SCRATCH_BYTES)
        .map_err(PrepareR2bError::from)
        .map_err(R2bServerError::from)?;
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot,
        bootstrap_image,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )?;
    debug_assert!(scratch.is_empty());

    publish_complete_plan(&plan, &mut driver)?;
    drain_egress(transport, &mut driver)?;

    debug_assert_eq!(driver.buffered_ingress(), 0);
    debug_assert_eq!(driver.queued_egress(), 0);
    Ok(R2bEntryOutcome::WorldProjectionReady(R2bPlaySession {
        driver,
        read_scratch,
        teleport,
        liveness: INITIAL_LIVENESS,
    }))
}

fn ready_for_r2b_handoff(io: &PrePlayIo<Target26_2R1x>) -> bool {
    let connection = io.connection();
    connection.phase() == SessionPhase::Play
        && connection.target_state().replay_complete()
        && connection.buffered_ingress() == 0
        && connection.queued_egress() == 0
}

fn publish_complete_plan(
    plan: &PreparedR2bPlan<'_>,
    driver: &mut ConnectionDriver,
) -> Result<(), R2bServerError> {
    let action_bound = plan
        .body_count()
        .checked_add(PLAY_PUBLICATION_STAGES.len())
        .and_then(|value| value.checked_add(1))
        .ok_or(R2bServerError::PublicationDidNotConverge)?;
    let mut cursor = StagedPublicationCursor::new();

    for _ in 0..action_bound {
        match publish_staged_plan_one::<Infallible, _>(plan, &mut cursor, driver)
            .map_err(|error| R2bServerError::PrePlay(map_driver_error(&error)))?
        {
            StagedPublicationStep::Complete => {
                debug_assert_eq!(
                    stage_for_publication_index(cursor.stage_index()),
                    PlayBootstrapStage::WorldProjection
                );
                return Ok(());
            }
            StagedPublicationStep::StageComplete { .. } | StagedPublicationStep::Queued { .. } => {}
        }
    }

    Err(R2bServerError::PublicationDidNotConverge)
}

fn drain_egress<RW>(transport: &mut RW, driver: &mut ConnectionDriver) -> Result<(), R2bServerError>
where
    RW: Write + ?Sized,
{
    while driver.queued_egress() != 0 {
        let pending = driver.pending_egress().len();
        let write_result = {
            let bytes = driver.pending_egress();
            transport.write(bytes)
        };
        match write_result {
            Ok(0) => {
                return Err(R2bServerError::PrePlay(PrePlayIoError::ZeroWrite {
                    pending,
                }));
            }
            Ok(written) => driver
                .consume_written::<Infallible>(written)
                .map_err(|error| R2bServerError::PrePlay(map_driver_error(&error)))?,
            Err(error) => {
                return Err(R2bServerError::PrePlay(io_error(
                    IoOperation::Write,
                    &error,
                )));
            }
        }
    }
    Ok(())
}

fn map_play_driver_error(error: &DriverError<PlayInboundDecodeError>) -> R2bPlayError {
    match error {
        DriverError::Buffer(error) => R2bPlayError::Buffer(*error),
        DriverError::Handler(PlayInboundDecodeError::TeleportCodec(error)) => {
            R2bPlayError::TeleportCodec(*error)
        }
        DriverError::Handler(PlayInboundDecodeError::KeepAliveCodec(error)) => {
            R2bPlayError::KeepAliveCodec(*error)
        }
        DriverError::Handler(PlayInboundDecodeError::Liveness(error)) => {
            R2bPlayError::Liveness(*error)
        }
        DriverError::Handler(PlayInboundDecodeError::Unclaimed(_)) => {
            unreachable!("unclaimed packets are mapped to a non-error process result")
        }
        DriverError::RollbackFailed {
            operation,
            rollback,
        } => R2bPlayError::RollbackFailed {
            operation: *operation,
            rollback: *rollback,
        },
        DriverError::AccountingOverflow => R2bPlayError::AccountingOverflow,
    }
}

fn map_driver_error(error: &DriverError<Infallible>) -> PrePlayIoError<R1xError> {
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

fn io_error(operation: IoOperation, error: &io::Error) -> PrePlayIoError<R1xError> {
    PrePlayIoError::Io {
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(FRAME_BODY_LIMIT, INGRESS_LIMIT, EGRESS_LIMIT)
        .expect("R2B development limits are positive and coherent")
}

fn read_scratch_bytes() -> NonZeroUsize {
    NonZeroUsize::new(READ_SCRATCH_BYTES).expect("R2B read scratch is positive")
}

fn action_budget() -> ActionBudget {
    ActionBudget::new(PREPLAY_ACTIONS_PER_SERVICE).expect("R2B action budget is positive")
}

#[cfg(test)]
mod tests {
    use crucible_connection_driver::ConnectionDriver;
    use crucible_packet_core::PacketWriter;
    use crucible_publication_core::StagedPublicationPlan;
    use crucible_target_26_2::r2b::{
        CommandPermissionProfile, CommandProjectionKey, PlayBootstrapImage26_2, ProjectionRevision,
        QualifiedProjectionArtifact, RecipeProjectionKey,
    };

    use super::{EGRESS_LIMIT, FRAME_BODY_LIMIT, INGRESS_LIMIT, READ_SCRATCH_BYTES, limits};

    const fn revision(byte: u8) -> ProjectionRevision {
        ProjectionRevision::new([byte; 32])
    }

    fn assert_plan_trait<T: StagedPublicationPlan>() {}

    #[test]
    fn r2b_entry_limits_are_finite_and_share_the_existing_single_driver_shape() {
        let limits = limits();
        assert_eq!(limits.max_frame_body_len(), FRAME_BODY_LIMIT);
        assert_eq!(limits.max_ingress_buffered(), INGRESS_LIMIT);
        assert_eq!(limits.max_egress_queued(), EGRESS_LIMIT);
        assert_eq!(READ_SCRATCH_BYTES, 16 * 1_024);
        let driver = ConnectionDriver::new(limits);
        assert_eq!(driver.buffered_ingress(), 0);
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn shared_image_construction_remains_process_owned_not_driver_owned() {
        let command_key = CommandProjectionKey::new(
            revision(1),
            revision(2),
            revision(3),
            revision(4),
            CommandPermissionProfile::DefaultNonOperator,
        );
        let recipe_key =
            RecipeProjectionKey::new(revision(5), revision(6), revision(7), revision(8));
        let image = PlayBootstrapImage26_2::new(
            QualifiedProjectionArtifact::new(command_key, vec![16, 0].into_boxed_slice())
                .expect("command artifact"),
            QualifiedProjectionArtifact::new(recipe_key, vec![0x85, 0x01, 0].into_boxed_slice())
                .expect("recipe artifact"),
        );

        let mut scratch = PacketWriter::new(8).expect("bounded scratch");
        scratch.write_u8(1).expect("test byte");
        assert_eq!(scratch.as_slice(), &[1]);
        assert_eq!(image.commands(&command_key), Ok(&[16, 0][..]));
        assert_eq!(image.update_recipes(&recipe_key), Ok(&[0x85, 0x01, 0][..]));

        assert_plan_trait::<crucible_target_26_2::r2b::PreparedR2bPlan<'_>>();
    }
}
