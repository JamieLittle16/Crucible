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

use helve_connection_core::ConnectionLimits;
use helve_preplay_core::{PrePlayConnection, PrePlayError, PrePlayProcess, PrePlayTarget};
use helve_session_core::SessionPhase;

mod publication;
pub use publication::{PublicationServiceReport, PublicationServiceStop};

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
        Self::from_connection(PrePlayConnection::new(limits), read_scratch_bytes)
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
            Err(error) => Err(io_error(IoOperation::Read, &error)),
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
            Err(error) => Err(io_error(IoOperation::Write, &error)),
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
            ProcessStop::Incomplete => match self.read_once(transport)? {
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
            },
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

    fn service_report(
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

fn io_error<E>(operation: IoOperation, error: &io::Error) -> PrePlayIoError<E> {
    PrePlayIoError::Io {
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}
