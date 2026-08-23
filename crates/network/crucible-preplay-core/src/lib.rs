//! Target-neutral transactional pre-play binding for Crucible.
//!
//! This crate is the production boundary between the bounded byte/connection machinery and a
//! source-backed target-version packet adapter. It owns no Minecraft packet identities or field
//! layouts. A target decoder produces an owned candidate action; the live session state is adopted
//! only after the complete outbound batch is admitted and the inbound frame is consumed atomically.

#![forbid(unsafe_code)]

use std::fmt;
use std::marker::PhantomData;

use crucible_connection_core::{ConnectionBufferError, ConnectionLimits, FrameView};
use crucible_connection_driver::{ConnectionDriver, DriverError, OutboundBatch, TransactionResult};
use crucible_session_core::{SessionPhase, SessionState};

/// One target-decoded candidate action which can be committed atomically.
///
/// The candidate session must be derived from the session passed to [`PrePlayTarget::decode`]. The
/// binder independently validates that it is either unchanged, one admitted forward transition, or
/// terminal closure before any outbound bytes are admitted.
pub trait PrePlayAction: OutboundBatch {
    /// Session state to adopt after the complete action commits.
    fn candidate_session(&self) -> SessionState;
}

/// Statically bound target-version decoder above the generic pre-play transport.
///
/// Implementations should be small adapters generated/built from admitted protocol evidence. The
/// target receives immutable context and the current session by value; it never receives mutable
/// access to the live [`PrePlayConnection`].
pub trait PrePlayTarget {
    /// Target-specific semantic/codec error.
    type Error;
    /// Immutable runtime context used while constructing candidate actions.
    type Context: ?Sized;
    /// Owned candidate action returned by one successful decode.
    type Action: PrePlayAction;

    /// Decodes one complete borrowed frame into an owned candidate action.
    ///
    /// No target-specific state is committed by this call. The returned candidate is validated and
    /// admitted transactionally by [`PrePlayConnection::process_one`].
    ///
    /// # Errors
    ///
    /// Returns a target-specific error for a packet that is invalid in the supplied session state.
    fn decode(
        context: &Self::Context,
        state: SessionState,
        frame: FrameView<'_>,
    ) -> Result<Self::Action, Self::Error>;
}

/// Result of one target-bound pre-play processing attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrePlayProcess {
    /// No complete inbound frame is currently available.
    Incomplete,
    /// Exactly one inbound action committed.
    Committed {
        /// Session phase before the action.
        from: SessionPhase,
        /// Session phase after the action.
        to: SessionPhase,
        /// Number of outbound packet frames admitted atomically with the action.
        outbound_frames: usize,
    },
}

/// Fail-closed target-bound pre-play error.
#[derive(Debug, Eq, PartialEq)]
pub enum PrePlayError<E> {
    /// Bounded ingress/egress or generic wire machinery rejected the operation.
    Buffer(ConnectionBufferError),
    /// The statically bound target decoder rejected the current frame.
    Target(E),
    /// The target returned a candidate session which is not an admitted direct successor.
    InvalidCandidate {
        /// Live phase before target decoding.
        from: SessionPhase,
        /// Phase requested by the candidate action.
        to: SessionPhase,
    },
    /// Processing was requested after terminal session closure.
    ClosedSession,
    /// An unexpected inbound-commit failure was followed by an unexpected egress rollback failure.
    RollbackFailed {
        /// Failure while consuming the already-admitted inbound frame.
        operation: ConnectionBufferError,
        /// Failure while restoring the previously queued egress tail.
        rollback: ConnectionBufferError,
    },
    /// Internal connection-driver accounting overflowed.
    AccountingOverflow,
}

#[derive(Debug)]
enum TargetBoundaryError<E> {
    Target(E),
    InvalidCandidate {
        from: SessionPhase,
        to: SessionPhase,
    },
}

struct ValidatedAction<A> {
    action: A,
    candidate: SessionState,
}

impl<A> OutboundBatch for ValidatedAction<A>
where
    A: PrePlayAction,
{
    type Body = A::Body;

    fn outbound_frames(&self) -> &[Self::Body] {
        self.action.outbound_frames()
    }
}

/// One bounded pre-play connection statically specialized for target `T`.
///
/// `T` is present only at the type level. No target registry, trait object or runtime service lookup
/// exists on the frame-processing path.
pub struct PrePlayConnection<T>
where
    T: PrePlayTarget,
{
    driver: ConnectionDriver,
    session: SessionState,
    target: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for PrePlayConnection<T>
where
    T: PrePlayTarget,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrePlayConnection")
            .field("driver", &self.driver)
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl<T> PrePlayConnection<T>
where
    T: PrePlayTarget,
{
    /// Creates an empty target-bound connection in the handshake phase.
    #[must_use]
    pub const fn new(limits: ConnectionLimits) -> Self {
        Self {
            driver: ConnectionDriver::new(limits),
            session: SessionState::new(),
            target: PhantomData,
        }
    }

    /// Current admitted session state.
    #[must_use]
    pub const fn session(&self) -> SessionState {
        self.session
    }

    /// Current admitted session phase.
    #[must_use]
    pub const fn phase(&self) -> SessionPhase {
        self.session.phase()
    }

    /// Appends one arbitrary socket-read fragment to bounded ingress storage.
    ///
    /// # Errors
    ///
    /// Returns a bounded-buffer/wire error without changing the logical active stream on failure.
    pub fn ingest(&mut self, incoming: &[u8]) -> Result<(), PrePlayError<T::Error>> {
        self.driver
            .ingest::<TargetBoundaryError<T::Error>>(incoming)
            .map_err(map_driver_error)
    }

    /// Processes at most one complete target packet as one semantic/outbound transaction.
    ///
    /// The target decoder sees the current session by value and a borrowed frame. The driver admits
    /// the target action's complete outbound batch before consuming the frame. Only after both have
    /// committed does this binder install the prevalidated candidate session.
    ///
    /// # Errors
    ///
    /// Returns fail-closed on terminal sessions, target rejection, an invalid candidate transition,
    /// bounded egress failure, malformed wire input, rollback failure or internal accounting
    /// overflow. No error path adopts the candidate session.
    pub fn process_one(
        &mut self,
        context: &T::Context,
    ) -> Result<PrePlayProcess, PrePlayError<T::Error>> {
        if self.session.phase() == SessionPhase::Closed {
            return Err(PrePlayError::ClosedSession);
        }

        let before = self.session;
        let transaction = self
            .driver
            .process_one_transactional(|frame| {
                let action =
                    T::decode(context, before, frame).map_err(TargetBoundaryError::Target)?;
                let candidate = action.candidate_session();
                if !candidate_is_admitted(before, candidate) {
                    return Err(TargetBoundaryError::InvalidCandidate {
                        from: before.phase(),
                        to: candidate.phase(),
                    });
                }
                Ok(ValidatedAction { action, candidate })
            })
            .map_err(map_driver_error)?;

        match transaction {
            TransactionResult::Incomplete => Ok(PrePlayProcess::Incomplete),
            TransactionResult::Committed(validated) => {
                let from = before.phase();
                let to = validated.candidate.phase();
                let outbound_frames = validated.outbound_frames().len();
                self.session = validated.candidate;
                Ok(PrePlayProcess::Committed {
                    from,
                    to,
                    outbound_frames,
                })
            }
        }
    }

    /// Contiguous encoded bytes currently ready for the socket writer.
    #[must_use]
    pub fn pending_egress(&self) -> &[u8] {
        self.driver.pending_egress()
    }

    /// Acknowledges exactly the number of bytes written by the socket adapter.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed buffer error instead of clamping impossible write counts.
    pub fn consume_written(&mut self, bytes: usize) -> Result<(), PrePlayError<T::Error>> {
        self.driver
            .consume_written::<TargetBoundaryError<T::Error>>(bytes)
            .map_err(map_driver_error)
    }

    /// Number of active ingress bytes not yet committed by target processing.
    #[must_use]
    pub fn buffered_ingress(&self) -> usize {
        self.driver.buffered_ingress()
    }

    /// Number of encoded egress bytes not yet acknowledged by the socket adapter.
    #[must_use]
    pub fn queued_egress(&self) -> usize {
        self.driver.queued_egress()
    }
}

fn candidate_is_admitted(before: SessionState, candidate: SessionState) -> bool {
    if candidate == before {
        return true;
    }
    if candidate.phase() == SessionPhase::Closed {
        return before.phase() != SessionPhase::Closed;
    }

    let mut probe = before;
    probe.advance(candidate.phase()).is_ok() && probe == candidate
}

fn map_driver_error<E>(error: DriverError<TargetBoundaryError<E>>) -> PrePlayError<E> {
    match error {
        DriverError::Buffer(error) => PrePlayError::Buffer(error),
        DriverError::Handler(TargetBoundaryError::Target(error)) => PrePlayError::Target(error),
        DriverError::Handler(TargetBoundaryError::InvalidCandidate { from, to }) => {
            PrePlayError::InvalidCandidate { from, to }
        }
        DriverError::RollbackFailed {
            operation,
            rollback,
        } => PrePlayError::RollbackFailed {
            operation,
            rollback,
        },
        DriverError::AccountingOverflow => PrePlayError::AccountingOverflow,
    }
}

#[cfg(test)]
mod tests {
    use super::{PrePlayAction, PrePlayConnection, PrePlayError, PrePlayProcess, PrePlayTarget};
    use crucible_connection_core::{ConnectionLimits, FrameView};
    use crucible_connection_driver::{ConnectionDriver, OutboundBatch};
    use crucible_packet_core::{PacketCodecError, PacketReader, PacketWriter};
    use crucible_session_core::{SessionPhase, SessionState, SessionStateError};

    const SELECT_STATUS: i32 = 0x51;
    const STATUS_QUERY: i32 = 0x52;
    const SELECT_LOGIN: i32 = 0x53;
    const LOGIN_COMPLETE: i32 = 0x54;
    const CONFIG_COMPLETE: i32 = 0x55;
    const ILLEGAL_REWIND: i32 = 0x56;
    const CLOSE: i32 = 0x57;

    const STATUS_REPLY: i32 = 0x71;
    const LOGIN_REPLY_A: i32 = 0x72;
    const LOGIN_REPLY_B: i32 = 0x73;

    const STATUS_MAGIC: &str = "crucible-status";
    const LOGIN_MAGIC: &str = "crucible-login";
    const LOGIN_PROOF: i64 = 0x1122_3344_5566_7788;
    const MAX_STRING_UNITS: usize = 64;
    const MAX_PACKET_BODY: usize = 128;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum SyntheticError {
        WrongPhase { packet_id: i32, phase: SessionPhase },
        UnknownPacket(i32),
        Codec(PacketCodecError),
        Transition(SessionStateError),
        InvalidMagic,
        InvalidProof,
        InvalidMarker,
    }

    impl From<PacketCodecError> for SyntheticError {
        fn from(value: PacketCodecError) -> Self {
            Self::Codec(value)
        }
    }

    impl From<SessionStateError> for SyntheticError {
        fn from(value: SessionStateError) -> Self {
            Self::Transition(value)
        }
    }

    #[derive(Clone, Debug)]
    struct SyntheticContext {
        status_label: &'static str,
    }

    #[derive(Debug)]
    struct SyntheticAction {
        candidate: SessionState,
        frames: Vec<Vec<u8>>,
    }

    impl OutboundBatch for SyntheticAction {
        type Body = Vec<u8>;

        fn outbound_frames(&self) -> &[Self::Body] {
            &self.frames
        }
    }

    impl PrePlayAction for SyntheticAction {
        fn candidate_session(&self) -> SessionState {
            self.candidate
        }
    }

    struct SyntheticTarget;

    impl PrePlayTarget for SyntheticTarget {
        type Error = SyntheticError;
        type Context = SyntheticContext;
        type Action = SyntheticAction;

        fn decode(
            context: &Self::Context,
            state: SessionState,
            frame: FrameView<'_>,
        ) -> Result<Self::Action, Self::Error> {
            let packet_id = frame.packet_id();
            let mut candidate = state;
            let mut reader = PacketReader::new(frame.payload());
            let frames = match packet_id {
                SELECT_STATUS => {
                    require_phase(state, SessionPhase::Handshake, packet_id)?;
                    if reader.read_string(MAX_STRING_UNITS)? != STATUS_MAGIC {
                        return Err(SyntheticError::InvalidMagic);
                    }
                    reader.finish()?;
                    candidate.advance(SessionPhase::Status)?;
                    Vec::new()
                }
                STATUS_QUERY => {
                    require_phase(state, SessionPhase::Status, packet_id)?;
                    let nonce = reader.read_u64()?;
                    reader.finish()?;
                    vec![packet_body(STATUS_REPLY, |writer| {
                        writer.write_u64(nonce)?;
                        writer.write_string(context.status_label, MAX_STRING_UNITS)
                    })]
                }
                SELECT_LOGIN => {
                    require_phase(state, SessionPhase::Handshake, packet_id)?;
                    if reader.read_string(MAX_STRING_UNITS)? != LOGIN_MAGIC {
                        return Err(SyntheticError::InvalidMagic);
                    }
                    reader.finish()?;
                    candidate.advance(SessionPhase::Login)?;
                    Vec::new()
                }
                LOGIN_COMPLETE => {
                    require_phase(state, SessionPhase::Login, packet_id)?;
                    if reader.read_i64()? != LOGIN_PROOF {
                        return Err(SyntheticError::InvalidProof);
                    }
                    reader.finish()?;
                    candidate.advance(SessionPhase::Configuration)?;
                    vec![
                        packet_body(LOGIN_REPLY_A, |writer| {
                            writer.write_string("configuration-one", MAX_STRING_UNITS)
                        }),
                        packet_body(LOGIN_REPLY_B, |writer| {
                            writer.write_string("configuration-two", MAX_STRING_UNITS)
                        }),
                    ]
                }
                CONFIG_COMPLETE => {
                    require_phase(state, SessionPhase::Configuration, packet_id)?;
                    if !reader.read_bool()? {
                        return Err(SyntheticError::InvalidMarker);
                    }
                    reader.finish()?;
                    candidate.advance(SessionPhase::Play)?;
                    Vec::new()
                }
                ILLEGAL_REWIND => {
                    require_phase(state, SessionPhase::Status, packet_id)?;
                    reader.finish()?;
                    candidate = SessionState::new();
                    Vec::new()
                }
                CLOSE => {
                    reader.finish()?;
                    if !candidate.close() {
                        return Err(SyntheticError::WrongPhase {
                            packet_id,
                            phase: state.phase(),
                        });
                    }
                    Vec::new()
                }
                other => return Err(SyntheticError::UnknownPacket(other)),
            };
            Ok(SyntheticAction { candidate, frames })
        }
    }

    fn require_phase(
        state: SessionState,
        expected: SessionPhase,
        packet_id: i32,
    ) -> Result<(), SyntheticError> {
        if state.phase() == expected {
            Ok(())
        } else {
            Err(SyntheticError::WrongPhase {
                packet_id,
                phase: state.phase(),
            })
        }
    }

    fn context() -> SyntheticContext {
        SyntheticContext {
            status_label: "crucible-synthetic-r0",
        }
    }

    fn generous_limits() -> ConnectionLimits {
        ConnectionLimits::new(MAX_PACKET_BODY, 64 * 1_024, 64 * 1_024)
            .expect("coherent test limits")
    }

    fn tight_limits() -> ConnectionLimits {
        ConnectionLimits::new(32, 256, 33).expect("one maximum frame fits")
    }

    fn packet_body(
        packet_id: i32,
        payload: impl FnOnce(&mut PacketWriter) -> Result<(), PacketCodecError>,
    ) -> Vec<u8> {
        let mut writer = PacketWriter::new(MAX_PACKET_BODY).expect("positive packet bound");
        writer
            .write_var_int(packet_id)
            .expect("synthetic packet id");
        payload(&mut writer).expect("synthetic payload fits");
        writer.into_bytes()
    }

    fn encoded_frame(body: &[u8]) -> Vec<u8> {
        let mut driver = ConnectionDriver::new(generous_limits());
        driver
            .queue_frame::<()>(body)
            .expect("synthetic frame fits");
        driver.pending_egress().to_vec()
    }

    fn select_status_body() -> Vec<u8> {
        packet_body(SELECT_STATUS, |writer| {
            writer.write_string(STATUS_MAGIC, MAX_STRING_UNITS)
        })
    }

    fn status_query_body(nonce: u64) -> Vec<u8> {
        packet_body(STATUS_QUERY, |writer| writer.write_u64(nonce))
    }

    fn select_login_body() -> Vec<u8> {
        packet_body(SELECT_LOGIN, |writer| {
            writer.write_string(LOGIN_MAGIC, MAX_STRING_UNITS)
        })
    }

    fn login_complete_body() -> Vec<u8> {
        packet_body(LOGIN_COMPLETE, |writer| writer.write_i64(LOGIN_PROOF))
    }

    fn config_complete_body() -> Vec<u8> {
        packet_body(CONFIG_COMPLETE, |writer| writer.write_bool(true))
    }

    fn empty_body(packet_id: i32) -> Vec<u8> {
        packet_body(packet_id, |_| Ok(()))
    }

    fn commit(
        connection: &mut PrePlayConnection<SyntheticTarget>,
        body: &[u8],
    ) -> Result<PrePlayProcess, PrePlayError<SyntheticError>> {
        connection.ingest(&encoded_frame(body))?;
        connection.process_one(&context())
    }

    #[test]
    fn every_handshake_split_point_preserves_state_until_complete() {
        let encoded = encoded_frame(&select_status_body());
        for split in 0..encoded.len() {
            let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
            connection
                .ingest(&encoded[..split])
                .expect("prefix fits bounded ingress");
            let buffered_before = connection.buffered_ingress();
            assert_eq!(
                connection.process_one(&context()),
                Ok(PrePlayProcess::Incomplete)
            );
            assert_eq!(connection.phase(), SessionPhase::Handshake);
            assert_eq!(connection.buffered_ingress(), buffered_before);
            assert_eq!(connection.queued_egress(), 0);

            connection
                .ingest(&encoded[split..])
                .expect("suffix fits bounded ingress");
            assert_eq!(
                connection.process_one(&context()),
                Ok(PrePlayProcess::Committed {
                    from: SessionPhase::Handshake,
                    to: SessionPhase::Status,
                    outbound_frames: 0,
                })
            );
            assert_eq!(connection.buffered_ingress(), 0);
        }
    }

    #[test]
    fn status_response_is_exact_and_same_phase_commit_is_admitted() {
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
        commit(&mut connection, &select_status_body()).expect("select status");
        let nonce = 0xDEAD_BEEF_CAFE_BABEu64;
        assert_eq!(
            commit(&mut connection, &status_query_body(nonce)),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Status,
                to: SessionPhase::Status,
                outbound_frames: 1,
            })
        );

        let expected_body = packet_body(STATUS_REPLY, |writer| {
            writer.write_u64(nonce)?;
            writer.write_string(context().status_label, MAX_STRING_UNITS)
        });
        assert_eq!(connection.pending_egress(), encoded_frame(&expected_body));
        let queued = connection.queued_egress();
        connection
            .consume_written(queued)
            .expect("ack exact egress");
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn invalid_target_candidate_is_rejected_without_stream_or_state_commit() {
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
        commit(&mut connection, &select_status_body()).expect("select status");
        let encoded = encoded_frame(&empty_body(ILLEGAL_REWIND));
        connection
            .ingest(&encoded)
            .expect("illegal action frame fits");
        let ingress_before = connection.buffered_ingress();

        assert_eq!(
            connection.process_one(&context()),
            Err(PrePlayError::InvalidCandidate {
                from: SessionPhase::Status,
                to: SessionPhase::Handshake,
            })
        );
        assert_eq!(connection.phase(), SessionPhase::Status);
        assert_eq!(connection.buffered_ingress(), ingress_before);
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn malformed_target_payload_is_transactional() {
        let malformed = packet_body(SELECT_STATUS, |writer| {
            writer.write_string(STATUS_MAGIC, MAX_STRING_UNITS)?;
            writer.write_bool(true)
        });
        let encoded = encoded_frame(&malformed);
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
        connection
            .ingest(&encoded)
            .expect("malformed semantic frame fits");
        let ingress_before = connection.buffered_ingress();

        assert!(matches!(
            connection.process_one(&context()),
            Err(PrePlayError::Target(SyntheticError::Codec(
                PacketCodecError::TrailingBytes { .. }
            )))
        ));
        assert_eq!(connection.phase(), SessionPhase::Handshake);
        assert_eq!(connection.buffered_ingress(), ingress_before);
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn multi_packet_transition_rejects_as_one_batch_under_egress_pressure() {
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(tight_limits());
        commit(&mut connection, &select_login_body()).expect("select login");
        let encoded = encoded_frame(&login_complete_body());
        connection
            .ingest(&encoded)
            .expect("login completion fits ingress");
        let ingress_before = connection.buffered_ingress();

        assert!(matches!(
            connection.process_one(&context()),
            Err(PrePlayError::Buffer(_))
        ));
        assert_eq!(connection.phase(), SessionPhase::Login);
        assert_eq!(connection.buffered_ingress(), ingress_before);
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn login_configuration_play_route_commits_exactly_one_state_per_frame() {
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
        assert_eq!(
            commit(&mut connection, &select_login_body()),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Handshake,
                to: SessionPhase::Login,
                outbound_frames: 0,
            })
        );
        assert_eq!(
            commit(&mut connection, &login_complete_body()),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Login,
                to: SessionPhase::Configuration,
                outbound_frames: 2,
            })
        );
        assert_eq!(
            commit(&mut connection, &config_complete_body()),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Configuration,
                to: SessionPhase::Play,
                outbound_frames: 0,
            })
        );
        assert_eq!(connection.phase(), SessionPhase::Play);
        assert_eq!(connection.buffered_ingress(), 0);
        assert!(connection.queued_egress() > 0);
    }

    #[test]
    fn terminal_session_never_processes_another_frame() {
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
        commit(&mut connection, &select_status_body()).expect("select status");
        commit(&mut connection, &empty_body(CLOSE)).expect("close status session");
        assert_eq!(connection.phase(), SessionPhase::Closed);

        let encoded = encoded_frame(&status_query_body(7));
        connection
            .ingest(&encoded)
            .expect("post-close bytes still bounded");
        let ingress_before = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(&context()),
            Err(PrePlayError::ClosedSession)
        );
        assert_eq!(connection.phase(), SessionPhase::Closed);
        assert_eq!(connection.buffered_ingress(), ingress_before);
    }

    #[test]
    fn ten_thousand_status_transactions_are_fragmentation_stable() {
        const ACTIONS: usize = 10_000;
        let mut connection = PrePlayConnection::<SyntheticTarget>::new(generous_limits());
        commit(&mut connection, &select_status_body()).expect("select status");

        let mut checksum = 0u64;
        for index in 0..ACTIONS {
            let index_u64 = u64::try_from(index).expect("test action index fits u64");
            let nonce_rotation =
                u32::try_from(index % 63).expect("test nonce rotation is below 63");
            let checksum_rotation =
                u32::try_from(index % 64).expect("test checksum rotation is below 64");
            let nonce = index_u64
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .rotate_left(nonce_rotation);
            let encoded = encoded_frame(&status_query_body(nonce));
            let split = index % encoded.len();
            connection
                .ingest(&encoded[..split])
                .expect("query prefix fits");
            assert_eq!(
                connection.process_one(&context()),
                Ok(PrePlayProcess::Incomplete)
            );
            connection
                .ingest(&encoded[split..])
                .expect("query suffix fits");
            assert!(matches!(
                connection.process_one(&context()),
                Ok(PrePlayProcess::Committed {
                    from: SessionPhase::Status,
                    to: SessionPhase::Status,
                    outbound_frames: 1,
                })
            ));
            checksum ^= nonce.rotate_left(checksum_rotation);
            let queued = connection.queued_egress();
            connection.consume_written(queued).expect("drain response");
        }

        assert_ne!(checksum, 0);
        assert_eq!(connection.phase(), SessionPhase::Status);
        assert_eq!(connection.buffered_ingress(), 0);
        assert_eq!(connection.queued_egress(), 0);
    }
}
