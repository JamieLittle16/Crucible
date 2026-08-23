//! Target-neutral bounded I/O adoption for Crucible's pre-play connection core.
//!
//! This crate is intentionally smaller than a socket runtime. It adapts any statically typed
//! `Read + Write` transport to [`PrePlayConnection`] while retaining one read scratch allocation,
//! preserving exact partial-write acknowledgement and bounding semantic work per service call.
//!
//! It owns no Minecraft packet identities, listener policy, executor, readiness mechanism,
//! authentication policy, compression policy or runtime selection.

#![forbid(unsafe_code)]

use std::fmt;
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::ConnectionLimits;
use crucible_preplay_core::{PrePlayConnection, PrePlayError, PrePlayProcess, PrePlayTarget};
use crucible_session_core::SessionPhase;

/// Positive maximum number of semantic actions admitted by one processing/service call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionBudget(NonZeroUsize);

impl ActionBudget {
    /// Constructs a positive action budget.
    #[must_use]
    pub const fn new(actions: usize) -> Option<Self> {
        match NonZeroUsize::new(actions) {
            Some(actions) => Some(Self(actions)),
            None => None,
        }
    }

    /// Returns the admitted action count.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// One nonfatal result from a transport read attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadOutcome {
    /// New bytes were appended to bounded ingress.
    Data(usize),
    /// The transport reported a temporary no-progress condition (`WouldBlock` or `Interrupted`).
    Pending,
    /// The peer reached EOF with no incomplete buffered frame.
    Eof,
}

/// One nonfatal result from a transport write attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteOutcome {
    /// No encoded egress is currently queued.
    Empty,
    /// The transport accepted some queued bytes.
    Progress {
        /// Exact bytes acknowledged through the connection core.
        written: usize,
        /// Encoded egress bytes still queued afterward.
        remaining: usize,
    },
    /// The transport reported a temporary no-progress condition (`WouldBlock` or `Interrupted`).
    Pending,
}

/// Why bounded target processing returned successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessStop {
    /// No complete inbound frame is currently available.
    Incomplete,
    /// The explicit action budget was exhausted.
    ActionBudgetExhausted,
    /// The target committed terminal session closure.
    SessionClosed,
}

/// Evidence from one bounded target-processing call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessReport {
    /// Semantic actions committed during this call.
    pub committed_actions: usize,
    /// Outbound packet frames admitted atomically by those actions.
    pub outbound_frames: usize,
    /// Boundary that stopped target processing.
    pub stop: ProcessStop,
}

/// Why one complete I/O service step returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceStop {
    /// No complete action is available and the transport made no new readable progress.
    InputPending,
    /// Encoded output remains queued after the bounded write attempt.
    OutputPending,
    /// The action budget was exhausted.
    ActionBudgetExhausted,
    /// The peer reached clean EOF after all admitted inbound bytes were consumed.
    PeerEof,
    /// The target session is closed and all currently queued output has been flushed.
    SessionClosed,
}

/// Evidence from one bounded `service_once` call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceReport {
    /// Bytes read from the transport during this call.
    pub read_bytes: usize,
    /// Bytes written to the transport during this call.
    pub written_bytes: usize,
    /// Semantic actions committed during this call.
    pub committed_actions: usize,
    /// Outbound packet frames admitted by committed actions.
    pub outbound_frames: usize,
    /// Ingress bytes still buffered after the call.
    pub buffered_ingress: usize,
    /// Egress bytes still queued after the call.
    pub queued_egress: usize,
    /// Boundary that stopped the service call.
    pub stop: ServiceStop,
}

/// I/O operation associated with a transport error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoOperation {
    /// Transport read.
    Read,
    /// Transport write.
    Write,
}

/// Fail-closed pre-play I/O adapter error.
#[derive(Debug, Eq, PartialEq)]
pub enum PrePlayIoError<E> {
    /// The target-neutral pre-play connection rejected an operation.
    Connection(PrePlayError<E>),
    /// A transport operation failed with a non-transient I/O error.
    Io {
        /// Operation that failed.
        operation: IoOperation,
        /// Stable I/O error classification.
        kind: io::ErrorKind,
        /// Human-readable OS/runtime detail retained only on the cold error path.
        message: String,
    },
    /// A writer returned success with zero bytes while egress was non-empty.
    ZeroWrite {
        /// Bytes that were pending when zero progress was reported.
        pending: usize,
    },
    /// The peer reached EOF while an incomplete framed packet remained buffered.
    TruncatedEof {
        /// Uncommitted ingress bytes retained for diagnostics.
        buffered_ingress: usize,
    },
    /// Adapter-side action/frame evidence accounting overflowed.
    AccountingOverflow,
}

impl<E> From<PrePlayError<E>> for PrePlayIoError<E> {
    fn from(value: PrePlayError<E>) -> Self {
        Self::Connection(value)
    }
}

/// Retained target-neutral I/O state for one pre-play connection.
///
/// The only owned scratch allocation is created once at construction. Packet/frame payloads remain
/// borrowed from the existing bounded connection storage and are never copied into this adapter.
pub struct PrePlayIo<T>
where
    T: PrePlayTarget,
{
    connection: PrePlayConnection<T>,
    read_scratch: Box<[u8]>,
    peer_eof: bool,
}

impl<T> fmt::Debug for PrePlayIo<T>
where
    T: PrePlayTarget,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrePlayIo")
            .field("connection", &self.connection)
            .field("read_scratch_bytes", &self.read_scratch.len())
            .field("peer_eof", &self.peer_eof)
            .finish()
    }
}

impl<T> PrePlayIo<T>
where
    T: PrePlayTarget,
{
    /// Creates one empty pre-play connection plus one retained read scratch allocation.
    #[must_use]
    pub fn new(limits: ConnectionLimits, read_scratch_bytes: NonZeroUsize) -> Self {
        Self::from_connection(
            PrePlayConnection::new(limits),
            read_scratch_bytes,
        )
    }

    /// Adopts an existing target-bound connection and allocates its retained read scratch once.
    #[must_use]
    pub fn from_connection(
        connection: PrePlayConnection<T>,
        read_scratch_bytes: NonZeroUsize,
    ) -> Self {
        Self {
            connection,
            read_scratch: vec![0_u8; read_scratch_bytes.get()].into_boxed_slice(),
            peer_eof: false,
        }
    }

    /// Returns the admitted target/session connection state.
    #[must_use]
    pub const fn connection(&self) -> &PrePlayConnection<T> {
        &self.connection
    }

    /// Returns whether transport EOF has already been observed.
    #[must_use]
    pub const fn peer_eof(&self) -> bool {
        self.peer_eof
    }

    /// Performs at most one transport read into retained scratch and bounded ingress.
    ///
    /// `WouldBlock` and `Interrupted` are reported as [`ReadOutcome::Pending`] so readiness/event
    /// loops can retry without treating scheduling state as a protocol failure.
    ///
    /// # Errors
    ///
    /// Returns fail-closed on non-transient I/O failure, bounded-ingress rejection, or EOF while
    /// incomplete ingress remains buffered. Callers should process already-buffered complete frames
    /// before requesting another read; [`Self::service_once`] enforces that ordering.
    pub fn read_once<R>(&mut self, reader: &mut R) -> Result<ReadOutcome, PrePlayIoError<T::Error>>
    where
        R: Read + ?Sized,
    {
        if self.peer_eof {
            return Ok(ReadOutcome::Eof);
        }

        match reader.read(&mut self.read_scratch) {
            Ok(0) => {
                self.peer_eof = true;
                let buffered_ingress = self.connection.buffered_ingress();
                if buffered_ingress == 0 {
                    Ok(ReadOutcome::Eof)
                } else {
                    Err(PrePlayIoError::TruncatedEof { buffered_ingress })
                }
            }
            Ok(read) => {
                self.connection.ingest(&self.read_scratch[..read])?;
                Ok(ReadOutcome::Data(read))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(ReadOutcome::Pending)
            }
            Err(error) => Err(io_error(IoOperation::Read, error)),
        }
    }

    /// Performs at most one transport write directly from the connection's contiguous egress view.
    ///
    /// No second outbound staging buffer is introduced. The exact kernel/runtime write count is
    /// acknowledged back into `PrePlayConnection` before this method returns success.
    ///
    /// # Errors
    ///
    /// Returns fail-closed on non-transient I/O failure, a zero-byte successful write with queued
    /// output, or an impossible write count rejected by the connection core.
    pub fn write_once<W>(
        &mut self,
        writer: &mut W,
    ) -> Result<WriteOutcome, PrePlayIoError<T::Error>>
    where
        W: Write + ?Sized,
    {
        let pending = self.connection.pending_egress().len();
        if pending == 0 {
            return Ok(WriteOutcome::Empty);
        }

        let write_result = {
            let bytes = self.connection.pending_egress();
            writer.write(bytes)
        };
        match write_result {
            Ok(0) => Err(PrePlayIoError::ZeroWrite { pending }),
            Ok(written) => {
                self.connection.consume_written(written)?;
                Ok(WriteOutcome::Progress {
                    written,
                    remaining: self.connection.queued_egress(),
                })
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(WriteOutcome::Pending)
            }
            Err(error) => Err(io_error(IoOperation::Write, error)),
        }
    }

    /// Processes complete already-buffered target actions under an explicit finite budget.
    ///
    /// # Errors
    ///
    /// Propagates the transactional target/buffer/session error from `PrePlayConnection`, or an
    /// adapter-side evidence-counter overflow. No target error consumes the current frame/session.
    pub fn process_available(
        &mut self,
        context: &T::Context,
        budget: ActionBudget,
    ) -> Result<ProcessReport, PrePlayIoError<T::Error>> {
        self.process_limit(context, budget.get())
    }

    /// Services one bounded unit of transport/target work.
    ///
    /// The service order is intentionally simple and deterministic:
    ///
    /// 1. make at most one write attempt for already-queued egress;
    /// 2. process already-buffered complete actions up to the remaining action budget;
    /// 3. if target processing needs input, make at most one read attempt;
    /// 4. process newly completed actions with the remaining action budget;
    /// 5. make at most one final write attempt for newly queued egress.
    ///
    /// Therefore one call performs at most one transport read, two transport writes, and exactly the
    /// caller-declared maximum semantic actions. Runtime/event-loop policy remains outside this crate.
    ///
    /// # Errors
    ///
    /// Propagates any fail-closed transport, connection, target/session, EOF or accounting error.
    pub fn service_once<RW>(
        &mut self,
        transport: &mut RW,
        context: &T::Context,
        budget: ActionBudget,
    ) -> Result<ServiceReport, PrePlayIoError<T::Error>>
    where
        RW: Read + Write + ?Sized,
    {
        let mut read_bytes = 0usize;
        let mut written_bytes = 0usize;
        let mut committed_actions = 0usize;
        let mut outbound_frames = 0usize;
        let mut remaining_actions = budget.get();

        match self.write_once(transport)? {
            WriteOutcome::Progress { written, remaining } => {
                written_bytes = add(written_bytes, written)?;
                if remaining != 0 {
                    return Ok(self.service_report(
                        read_bytes,
                        written_bytes,
                        committed_actions,
                        outbound_frames,
                        ServiceStop::OutputPending,
                    ));
                }
            }
            WriteOutcome::Pending => {
                return Ok(self.service_report(
                    read_bytes,
                    written_bytes,
                    committed_actions,
                    outbound_frames,
                    ServiceStop::OutputPending,
                ));
            }
            WriteOutcome::Empty => {}
        }

        let first = self.process_limit(context, remaining_actions)?;
        committed_actions = add(committed_actions, first.committed_actions)?;
        outbound_frames = add(outbound_frames, first.outbound_frames)?;
        remaining_actions = remaining_actions
            .checked_sub(first.committed_actions)
            .ok_or(PrePlayIoError::AccountingOverflow)?;

        let mut stop = match first.stop {
            ProcessStop::SessionClosed => ServiceStop::SessionClosed,
            ProcessStop::ActionBudgetExhausted => ServiceStop::ActionBudgetExhausted,
            ProcessStop::Incomplete if self.peer_eof => ServiceStop::PeerEof,
            ProcessStop::Incomplete => {
                match self.read_once(transport)? {
                    ReadOutcome::Data(read) => {
                        read_bytes = add(read_bytes, read)?;
                        if remaining_actions == 0 {
                            ServiceStop::ActionBudgetExhausted
                        } else {
                            let second = self.process_limit(context, remaining_actions)?;
                            committed_actions = add(committed_actions, second.committed_actions)?;
                            outbound_frames = add(outbound_frames, second.outbound_frames)?;
                            match second.stop {
                                ProcessStop::Incomplete => ServiceStop::InputPending,
                                ProcessStop::ActionBudgetExhausted => {
                                    ServiceStop::ActionBudgetExhausted
                                }
                                ProcessStop::SessionClosed => ServiceStop::SessionClosed,
                            }
                        }
                    }
                    ReadOutcome::Pending => ServiceStop::InputPending,
                    ReadOutcome::Eof => ServiceStop::PeerEof,
                }
            }
        };

        match self.write_once(transport)? {
            WriteOutcome::Progress { written, remaining } => {
                written_bytes = add(written_bytes, written)?;
                if remaining != 0 {
                    stop = ServiceStop::OutputPending;
                }
            }
            WriteOutcome::Pending => stop = ServiceStop::OutputPending,
            WriteOutcome::Empty => {}
        }

        if stop == ServiceStop::SessionClosed && self.connection.queued_egress() != 0 {
            stop = ServiceStop::OutputPending;
        }
        if stop == ServiceStop::PeerEof && self.connection.queued_egress() != 0 {
            stop = ServiceStop::OutputPending;
        }

        Ok(self.service_report(
            read_bytes,
            written_bytes,
            committed_actions,
            outbound_frames,
            stop,
        ))
    }

    fn process_limit(
        &mut self,
        context: &T::Context,
        max_actions: usize,
    ) -> Result<ProcessReport, PrePlayIoError<T::Error>> {
        if self.connection.phase() == SessionPhase::Closed {
            return Ok(ProcessReport {
                committed_actions: 0,
                outbound_frames: 0,
                stop: ProcessStop::SessionClosed,
            });
        }
        if max_actions == 0 {
            return Ok(ProcessReport {
                committed_actions: 0,
                outbound_frames: 0,
                stop: ProcessStop::ActionBudgetExhausted,
            });
        }

        let mut committed_actions = 0usize;
        let mut outbound_frames = 0usize;
        while committed_actions < max_actions {
            match self.connection.process_one(context)? {
                PrePlayProcess::Incomplete => {
                    return Ok(ProcessReport {
                        committed_actions,
                        outbound_frames,
                        stop: ProcessStop::Incomplete,
                    });
                }
                PrePlayProcess::Committed {
                    outbound_frames: admitted,
                    ..
                } => {
                    committed_actions = add(committed_actions, 1)?;
                    outbound_frames = add(outbound_frames, admitted)?;
                    if self.connection.phase() == SessionPhase::Closed {
                        return Ok(ProcessReport {
                            committed_actions,
                            outbound_frames,
                            stop: ProcessStop::SessionClosed,
                        });
                    }
                }
            }
        }

        Ok(ProcessReport {
            committed_actions,
            outbound_frames,
            stop: ProcessStop::ActionBudgetExhausted,
        })
    }

    const fn service_report(
        &self,
        read_bytes: usize,
        written_bytes: usize,
        committed_actions: usize,
        outbound_frames: usize,
        stop: ServiceStop,
    ) -> ServiceReport {
        ServiceReport {
            read_bytes,
            written_bytes,
            committed_actions,
            outbound_frames,
            buffered_ingress: self.connection.buffered_ingress(),
            queued_egress: self.connection.queued_egress(),
            stop,
        }
    }
}

fn add<E>(left: usize, right: usize) -> Result<usize, PrePlayIoError<E>> {
    left.checked_add(right)
        .ok_or(PrePlayIoError::AccountingOverflow)
}

fn io_error<E>(operation: IoOperation, error: io::Error) -> PrePlayIoError<E> {
    PrePlayIoError::Io {
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::num::NonZeroUsize;
    use std::thread;
    use std::time::Duration;

    use crucible_connection_core::{ConnectionLimits, FrameView};
    use crucible_connection_driver::{ConnectionDriver, OutboundBatch};
    use crucible_packet_core::{PacketCodecError, PacketReader, PacketWriter};
    use crucible_preplay_core::{PrePlayAction, PrePlayTarget};
    use crucible_session_core::{SessionPhase, SessionState, SessionStateError};

    use super::{
        ActionBudget, PrePlayIo, PrePlayIoError, ProcessStop, ServiceStop, WriteOutcome,
    };

    const SELECT_STATUS: i32 = 0x41;
    const STATUS_QUERY: i32 = 0x42;
    const CLOSE: i32 = 0x43;
    const STATUS_REPLY: i32 = 0x61;
    const STATUS_MAGIC: &str = "crucible-io-status";
    const STATUS_LABEL: &str = "crucible-io-adapter";
    const MAX_PACKET_BODY: usize = 512;
    const TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum SyntheticError {
        WrongPhase,
        Codec(PacketCodecError),
        Transition(SessionStateError),
        UnknownPacket(i32),
        InvalidMagic,
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

    #[derive(Debug)]
    struct Action {
        candidate: SessionState,
        frames: Vec<Vec<u8>>,
    }

    impl OutboundBatch for Action {
        type Body = Vec<u8>;

        fn outbound_frames(&self) -> &[Self::Body] {
            &self.frames
        }
    }

    impl PrePlayAction for Action {
        fn candidate_session(&self) -> SessionState {
            self.candidate
        }
    }

    struct SyntheticTarget;

    impl PrePlayTarget for SyntheticTarget {
        type Error = SyntheticError;
        type Context = str;
        type Action = Action;

        fn decode(
            context: &Self::Context,
            state: SessionState,
            frame: FrameView<'_>,
        ) -> Result<Self::Action, Self::Error> {
            let mut reader = PacketReader::new(frame.payload());
            let mut candidate = state;
            let frames = match frame.packet_id() {
                SELECT_STATUS => {
                    if state.phase() != SessionPhase::Handshake {
                        return Err(SyntheticError::WrongPhase);
                    }
                    if reader.read_string(64)? != STATUS_MAGIC {
                        return Err(SyntheticError::InvalidMagic);
                    }
                    reader.finish()?;
                    candidate.advance(SessionPhase::Status)?;
                    Vec::new()
                }
                STATUS_QUERY => {
                    if state.phase() != SessionPhase::Status {
                        return Err(SyntheticError::WrongPhase);
                    }
                    let nonce = reader.read_i64()?;
                    reader.finish()?;
                    vec![packet_body(STATUS_REPLY, |writer| {
                        writer.write_i64(nonce)?;
                        writer.write_string(context, 64)
                    })]
                }
                CLOSE => {
                    reader.finish()?;
                    if !candidate.close() {
                        return Err(SyntheticError::WrongPhase);
                    }
                    Vec::new()
                }
                packet => return Err(SyntheticError::UnknownPacket(packet)),
            };
            Ok(Action { candidate, frames })
        }
    }

    #[derive(Debug)]
    struct MemoryTransport {
        input: Vec<u8>,
        read_cursor: usize,
        read_schedule: Vec<usize>,
        read_calls: usize,
        write_limit: usize,
        zero_write: bool,
        output: Vec<u8>,
    }

    impl MemoryTransport {
        fn new(input: Vec<u8>) -> Self {
            Self {
                input,
                read_cursor: 0,
                read_schedule: Vec::new(),
                read_calls: 0,
                write_limit: usize::MAX,
                zero_write: false,
                output: Vec::new(),
            }
        }

        fn scheduled(input: Vec<u8>, read_schedule: Vec<usize>) -> Self {
            Self {
                read_schedule,
                ..Self::new(input)
            }
        }
    }

    impl Read for MemoryTransport {
        fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
            if self.read_cursor == self.input.len() {
                return Ok(0);
            }
            let scheduled = self
                .read_schedule
                .get(self.read_calls)
                .copied()
                .unwrap_or(usize::MAX);
            self.read_calls = self.read_calls.saturating_add(1);
            let available = self.input.len() - self.read_cursor;
            let count = available.min(destination.len()).min(scheduled.max(1));
            destination[..count]
                .copy_from_slice(&self.input[self.read_cursor..self.read_cursor + count]);
            self.read_cursor += count;
            Ok(count)
        }
    }

    impl Write for MemoryTransport {
        fn write(&mut self, source: &[u8]) -> io::Result<usize> {
            if self.zero_write && !source.is_empty() {
                return Ok(0);
            }
            let count = source.len().min(self.write_limit);
            self.output.extend_from_slice(&source[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn limits() -> ConnectionLimits {
        ConnectionLimits::new(MAX_PACKET_BODY, 256 * 1_024, 256 * 1_024)
            .expect("coherent test limits")
    }

    fn scratch() -> NonZeroUsize {
        NonZeroUsize::new(17).expect("positive scratch")
    }

    fn budget(actions: usize) -> ActionBudget {
        ActionBudget::new(actions).expect("positive action budget")
    }

    fn packet_body(
        packet_id: i32,
        encode: impl FnOnce(&mut PacketWriter) -> Result<(), PacketCodecError>,
    ) -> Vec<u8> {
        let mut writer = PacketWriter::new(MAX_PACKET_BODY).expect("positive body bound");
        writer.write_var_int(packet_id).expect("synthetic id fits");
        encode(&mut writer).expect("synthetic payload fits");
        writer.into_bytes()
    }

    fn frame(body: &[u8]) -> Vec<u8> {
        let mut driver = ConnectionDriver::new(limits());
        driver.queue_frame::<()>(body).expect("frame fits");
        driver.pending_egress().to_vec()
    }

    fn select_status() -> Vec<u8> {
        frame(&packet_body(SELECT_STATUS, |writer| {
            writer.write_string(STATUS_MAGIC, 64)
        }))
    }

    fn status_query(nonce: i64) -> Vec<u8> {
        frame(&packet_body(STATUS_QUERY, |writer| writer.write_i64(nonce)))
    }

    fn close() -> Vec<u8> {
        frame(&packet_body(CLOSE, |_| Ok(())))
    }

    fn expected_status_reply(nonce: i64) -> Vec<u8> {
        frame(&packet_body(STATUS_REPLY, |writer| {
            writer.write_i64(nonce)?;
            writer.write_string(STATUS_LABEL, 64)
        }))
    }

    fn service_until_terminal(
        io: &mut PrePlayIo<SyntheticTarget>,
        transport: &mut MemoryTransport,
        action_budget: usize,
    ) -> Result<ServiceStop, PrePlayIoError<SyntheticError>> {
        for _ in 0..100_000 {
            let report = io.service_once(transport, STATUS_LABEL, budget(action_budget))?;
            match report.stop {
                ServiceStop::SessionClosed | ServiceStop::PeerEof => return Ok(report.stop),
                ServiceStop::InputPending
                | ServiceStop::OutputPending
                | ServiceStop::ActionBudgetExhausted => {}
            }
        }
        panic!("synthetic service failed to converge");
    }

    #[test]
    fn zero_action_budget_is_unrepresentable() {
        assert_eq!(ActionBudget::new(0), None);
        assert_eq!(ActionBudget::new(1).map(ActionBudget::get), Some(1));
    }

    #[test]
    fn every_handshake_split_point_reaches_identical_status_state() {
        let encoded = select_status();
        for split in 1..encoded.len() {
            let mut transport = MemoryTransport::scheduled(
                encoded.clone(),
                vec![split, encoded.len() - split],
            );
            let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());
            assert_eq!(
                service_until_terminal(&mut io, &mut transport, 8),
                Ok(ServiceStop::PeerEof)
            );
            assert_eq!(io.connection().phase(), SessionPhase::Status);
            assert_eq!(io.connection().buffered_ingress(), 0);
        }
    }

    #[test]
    fn coalesced_actions_respect_explicit_action_budget() {
        let nonce = 0x1122_3344_5566_7788_i64;
        let mut input = select_status();
        input.extend_from_slice(&status_query(nonce));
        let mut transport = MemoryTransport::new(input);
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        let first = io
            .service_once(&mut transport, STATUS_LABEL, budget(1))
            .expect("first bounded service");
        assert_eq!(first.committed_actions, 1);
        assert_eq!(first.stop, ServiceStop::ActionBudgetExhausted);
        assert_eq!(io.connection().phase(), SessionPhase::Status);
        assert!(first.buffered_ingress > 0);

        let second = io
            .service_once(&mut transport, STATUS_LABEL, budget(1))
            .expect("second bounded service");
        assert_eq!(second.committed_actions, 1);
        assert_eq!(second.outbound_frames, 1);
        assert_eq!(transport.output, expected_status_reply(nonce));
    }

    #[test]
    fn one_byte_partial_writes_are_acknowledged_exactly() {
        let nonce = -0x1020_3040_5060_708_i64;
        let mut input = select_status();
        input.extend_from_slice(&status_query(nonce));
        input.extend_from_slice(&close());
        let mut transport = MemoryTransport::new(input);
        transport.write_limit = 1;
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        assert_eq!(
            service_until_terminal(&mut io, &mut transport, 4),
            Ok(ServiceStop::SessionClosed)
        );
        assert_eq!(transport.output, expected_status_reply(nonce));
        assert_eq!(io.connection().queued_egress(), 0);
    }

    #[test]
    fn zero_write_fails_instead_of_spinning() {
        let nonce = 7_i64;
        let mut input = select_status();
        input.extend_from_slice(&status_query(nonce));
        let mut transport = MemoryTransport::new(input);
        transport.zero_write = true;
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        let error = loop {
            match io.service_once(&mut transport, STATUS_LABEL, budget(4)) {
                Err(error) => break error,
                Ok(_) => {}
            }
        };
        assert!(matches!(error, PrePlayIoError::ZeroWrite { pending } if pending > 0));
        assert_eq!(io.connection().phase(), SessionPhase::Status);
        assert!(io.connection().queued_egress() > 0);
    }

    #[test]
    fn eof_with_partial_frame_fails_closed() {
        let mut encoded = select_status();
        encoded.pop();
        let mut transport = MemoryTransport::new(encoded);
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        let error = loop {
            match io.service_once(&mut transport, STATUS_LABEL, budget(4)) {
                Err(error) => break error,
                Ok(report) => assert_ne!(report.stop, ServiceStop::PeerEof),
            }
        };
        assert!(matches!(
            error,
            PrePlayIoError::TruncatedEof {
                buffered_ingress
            } if buffered_ingress > 0
        ));
        assert_eq!(io.connection().phase(), SessionPhase::Handshake);
    }

    #[test]
    fn target_error_preserves_current_frame_and_session() {
        let malformed = frame(&packet_body(SELECT_STATUS, |writer| {
            writer.write_string("wrong", 64)
        }));
        let mut transport = MemoryTransport::new(malformed.clone());
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        let error = loop {
            match io.service_once(&mut transport, STATUS_LABEL, budget(4)) {
                Err(error) => break error,
                Ok(_) => {}
            }
        };
        assert_eq!(
            error,
            PrePlayIoError::Connection(crucible_preplay_core::PrePlayError::Target(
                SyntheticError::InvalidMagic
            ))
        );
        assert_eq!(io.connection().phase(), SessionPhase::Handshake);
        assert_eq!(io.connection().buffered_ingress(), malformed.len());
        assert_eq!(io.connection().queued_egress(), 0);
    }

    #[test]
    fn terminal_close_stops_before_following_frame() {
        let mut input = select_status();
        input.extend_from_slice(&close());
        let trailing = status_query(123);
        input.extend_from_slice(&trailing);
        let mut transport = MemoryTransport::new(input);
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        assert_eq!(
            service_until_terminal(&mut io, &mut transport, 8),
            Ok(ServiceStop::SessionClosed)
        );
        assert_eq!(io.connection().phase(), SessionPhase::Closed);
        assert_eq!(io.connection().buffered_ingress(), trailing.len());
    }

    #[test]
    fn processing_only_api_is_explicitly_bounded() {
        let mut stream = select_status();
        stream.extend_from_slice(&status_query(1));
        stream.extend_from_slice(&status_query(2));
        let mut transport = MemoryTransport::new(stream);
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());
        assert!(matches!(io.read_once(&mut transport), Ok(super::ReadOutcome::Data(_))));

        let report = io
            .process_available(STATUS_LABEL, budget(2))
            .expect("bounded processing");
        assert_eq!(report.committed_actions, 2);
        assert_eq!(report.stop, ProcessStop::ActionBudgetExhausted);
        assert!(io.connection().buffered_ingress() > 0);
    }

    #[test]
    fn ten_thousand_status_actions_reuse_one_retained_adapter() {
        const QUERIES: usize = 10_000;
        let mut input = select_status();
        let mut expected = Vec::new();
        for index in 0..QUERIES {
            let nonce = i64::try_from(index).expect("bounded test nonce");
            input.extend_from_slice(&status_query(nonce));
            expected.extend_from_slice(&expected_status_reply(nonce));
        }
        input.extend_from_slice(&close());
        let mut transport = MemoryTransport::new(input);
        transport.read_schedule = vec![3; 50_000];
        transport.write_limit = 11;
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        assert_eq!(
            service_until_terminal(&mut io, &mut transport, 7),
            Ok(ServiceStop::SessionClosed)
        );
        assert_eq!(transport.output, expected);
        assert_eq!(io.connection().buffered_ingress(), 0);
        assert_eq!(io.connection().queued_egress(), 0);
    }

    #[test]
    fn synthetic_target_runs_through_real_loopback_tcp() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
        let address = listener.local_addr().expect("loopback address");
        let nonce = 0x0102_0304_0506_0708_i64;
        let mut request = select_status();
        request.extend_from_slice(&status_query(nonce));
        request.extend_from_slice(&close());
        let expected = expected_status_reply(nonce);

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept loopback client");
            stream
                .set_read_timeout(Some(TIMEOUT))
                .expect("server read timeout");
            stream
                .set_write_timeout(Some(TIMEOUT))
                .expect("server write timeout");
            let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());
            loop {
                let report = io
                    .service_once(&mut stream, STATUS_LABEL, budget(4))
                    .expect("loopback service");
                if report.stop == ServiceStop::SessionClosed {
                    break;
                }
            }
        });

        let mut client = TcpStream::connect(address).expect("connect loopback");
        client
            .set_read_timeout(Some(TIMEOUT))
            .expect("client read timeout");
        client
            .set_write_timeout(Some(TIMEOUT))
            .expect("client write timeout");
        for byte in request {
            client.write_all(&[byte]).expect("fragmented client write");
        }
        let mut observed = vec![0_u8; expected.len()];
        client.read_exact(&mut observed).expect("read exact status reply");
        assert_eq!(observed, expected);
        server.join().expect("loopback server finishes");
    }

    #[test]
    fn write_once_exposes_partial_progress_without_copying_to_second_queue() {
        let nonce = 99_i64;
        let mut input = select_status();
        input.extend_from_slice(&status_query(nonce));
        let mut transport = MemoryTransport::new(input);
        transport.write_limit = 2;
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

        loop {
            let report = io
                .service_once(&mut transport, STATUS_LABEL, budget(2))
                .expect("service response");
            if io.connection().queued_egress() != 0 {
                break;
            }
            if report.stop == ServiceStop::PeerEof {
                panic!("response was never queued");
            }
        }
        let before = io.connection().queued_egress();
        assert!(before > 2);
        assert_eq!(
            io.write_once(&mut transport),
            Ok(WriteOutcome::Progress {
                written: 2,
                remaining: before - 2,
            })
        );
    }
}
