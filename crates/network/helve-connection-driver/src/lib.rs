//! Runtime-neutral bounded connection driving for Crucible.
//!
//! This crate sits above `helve-connection-core` and below target-version packet semantics. It
//! provides a small, statically dispatched processing loop over borrowed frame views while keeping
//! ingress/egress bounds and exact stream consumption explicit.
//!
//! It deliberately does not choose a socket runtime, packet registry, protocol version, session
//! state machine, authentication policy, compression policy, allocator, or scheduler.

#![forbid(unsafe_code)]

use std::num::NonZeroUsize;

use helve_connection_core::{
    ConnectionBufferError, ConnectionLimits, EgressBuffer, FrameView, IngressBuffer,
};

/// Maximum number of complete frames one driver poll may commit.
///
/// Making zero unrepresentable prevents callers from accidentally installing a permanently
/// non-progressing poll loop while still keeping fairness policy explicit at the call site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameBudget(NonZeroUsize);

impl FrameBudget {
    /// Constructs a positive frame budget.
    #[must_use]
    pub const fn new(frames: usize) -> Option<Self> {
        match NonZeroUsize::new(frames) {
            Some(frames) => Some(Self(frames)),
            None => None,
        }
    }

    /// Returns the admitted maximum frames for one processing call.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// Handler-requested control after one frame has been processed successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameFlow {
    /// Continue processing while complete frames and budget remain.
    Continue,
    /// Commit the current frame, then return control to the caller immediately.
    Yield,
}

/// Why a successful processing call returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    /// No complete frame is currently available; more ingress may make progress possible.
    Incomplete,
    /// The explicit per-poll frame budget was exhausted.
    BudgetExhausted,
    /// The handler requested a yield after successfully processing the current frame.
    HandlerYield,
}

/// Evidence from one bounded frame-processing call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessReport {
    /// Complete frames committed during this call.
    pub frames: usize,
    /// Exact ingress stream bytes consumed by committed frames.
    pub stream_bytes: usize,
    /// Boundary that stopped processing.
    pub stop: StopReason,
}

/// A candidate semantic action exposes its complete required outbound frame bodies before commit.
///
/// This trait is statically dispatched and target-neutral. Implementations may use a fixed array,
/// `Vec`, small domain-specific container, or any other owned representation whose elements borrow
/// as frame-body byte slices. The driver never stores the batch beyond the transaction call.
pub trait OutboundBatch {
    /// One already-formed packet body, including packet-ID `VarInt` and payload.
    type Body: AsRef<[u8]>;

    /// Complete outbound frame-body batch required by this candidate action.
    fn outbound_frames(&self) -> &[Self::Body];
}

/// Result of attempting one atomic inbound-action/outbound-admission transaction.
#[derive(Debug, Eq, PartialEq)]
pub enum TransactionResult<A> {
    /// No complete inbound frame is currently available.
    Incomplete,
    /// The inbound frame was consumed and the complete outbound batch was admitted atomically.
    Committed(A),
}

/// Fail-closed connection-driver errors.
#[derive(Debug, Eq, PartialEq)]
pub enum DriverError<E> {
    /// The admitted bounded-buffer layer rejected an operation.
    Buffer(ConnectionBufferError),
    /// Target/session logic rejected the current borrowed frame.
    ///
    /// The current frame remains logically unconsumed. The handler owns rollback of any external
    /// state it mutates before returning this error; the driver guarantees only stream progression.
    Handler(E),
    /// An unexpected inbound-commit failure was followed by an unexpected failure to remove the
    /// just-admitted outbound tail. This indicates an internal transaction invariant breach.
    RollbackFailed {
        operation: ConnectionBufferError,
        rollback: ConnectionBufferError,
    },
    /// Internal accounting overflowed while accumulating a processing report.
    AccountingOverflow,
}

impl<E> From<ConnectionBufferError> for DriverError<E> {
    fn from(value: ConnectionBufferError) -> Self {
        Self::Buffer(value)
    }
}

/// One bounded connection's runtime-neutral byte-driving state.
#[derive(Debug)]
pub struct ConnectionDriver {
    ingress: IngressBuffer,
    egress: EgressBuffer,
}

impl ConnectionDriver {
    /// Creates an empty driver under one coherent connection limit set.
    #[must_use]
    pub const fn new(limits: ConnectionLimits) -> Self {
        Self {
            ingress: IngressBuffer::new(limits),
            egress: EgressBuffer::new(limits),
        }
    }

    /// Appends one arbitrary socket-read fragment to bounded ingress storage.
    ///
    /// # Errors
    ///
    /// Returns the underlying fail-closed ingress error. Rejection leaves the logical active
    /// ingress stream unchanged.
    pub fn ingest<E>(&mut self, incoming: &[u8]) -> Result<(), DriverError<E>> {
        self.ingress.push(incoming).map_err(DriverError::Buffer)
    }

    /// Processes complete borrowed frames up to `budget` using statically dispatched handler code.
    ///
    /// The handler sees the exact borrowed packet ID/payload view produced by the admitted
    /// connection buffer. The driver consumes the corresponding stream bytes only after the handler
    /// returns `Ok`. Therefore a handler error cannot silently advance ingress.
    ///
    /// This method does not expose egress to the handler. Target-specific code can accumulate its
    /// response decision in caller-owned state and queue encoded frame bodies separately, keeping
    /// stream-consumption semantics independent from response policy. Semantic actions whose commit
    /// requires outbound capacity should instead use [`Self::process_one_transactional`].
    ///
    /// # Errors
    ///
    /// Returns a buffer error for malformed wire input, the handler's error without consuming the
    /// current frame, or an accounting-overflow error.
    pub fn process_available<E, F>(
        &mut self,
        budget: FrameBudget,
        mut handler: F,
    ) -> Result<ProcessReport, DriverError<E>>
    where
        F: FnMut(FrameView<'_>) -> Result<FrameFlow, E>,
    {
        let mut frames = 0usize;
        let mut stream_bytes = 0usize;

        loop {
            if frames == budget.get() {
                return Ok(ProcessReport {
                    frames,
                    stream_bytes,
                    stop: StopReason::BudgetExhausted,
                });
            }

            let Some(frame) = self.ingress.peek_frame().map_err(DriverError::Buffer)? else {
                return Ok(ProcessReport {
                    frames,
                    stream_bytes,
                    stop: StopReason::Incomplete,
                });
            };

            let consumed = frame.stream_bytes();
            let flow = handler(frame).map_err(DriverError::Handler)?;
            self.ingress
                .consume(consumed)
                .map_err(DriverError::Buffer)?;
            frames = frames
                .checked_add(1)
                .ok_or(DriverError::AccountingOverflow)?;
            stream_bytes = stream_bytes
                .checked_add(consumed)
                .ok_or(DriverError::AccountingOverflow)?;

            if flow == FrameFlow::Yield {
                return Ok(ProcessReport {
                    frames,
                    stream_bytes,
                    stop: StopReason::HandlerYield,
                });
            }
        }
    }

    /// Processes at most one complete inbound frame as an atomic semantic/outbound transaction.
    ///
    /// The handler receives a borrowed frame and must return an owned candidate action. No external
    /// semantic state should be adopted by the caller until this method returns
    /// [`TransactionResult::Committed`]. The action exposes its entire required outbound batch via
    /// [`OutboundBatch`]. The driver then:
    ///
    /// 1. validates and admits the complete outbound batch against the exact bounded egress queue;
    /// 2. consumes the corresponding inbound stream bytes only after admission succeeds;
    /// 3. returns the candidate action for caller-side semantic adoption.
    ///
    /// A handler failure or outbound-capacity failure leaves the current inbound frame unconsumed.
    /// An impossible failure while consuming the already-peeked frame triggers exact egress-tail
    /// rollback before the error is returned.
    ///
    /// # Errors
    ///
    /// Returns a wire/buffer error without committing ingress, the handler's error without changing
    /// connection buffers, or [`DriverError::RollbackFailed`] if two internal invariants fail while
    /// trying to restore atomicity.
    pub fn process_one_transactional<E, A, F>(
        &mut self,
        handler: F,
    ) -> Result<TransactionResult<A>, DriverError<E>>
    where
        A: OutboundBatch,
        F: FnOnce(FrameView<'_>) -> Result<A, E>,
    {
        let Some(frame) = self.ingress.peek_frame().map_err(DriverError::Buffer)? else {
            return Ok(TransactionResult::Incomplete);
        };
        let consumed = frame.stream_bytes();
        let action = handler(frame).map_err(DriverError::Handler)?;
        let queued_before = self.egress.queued_len();
        self.egress
            .queue_batch(action.outbound_frames())
            .map_err(DriverError::Buffer)?;

        if let Err(operation) = self.ingress.consume(consumed) {
            if let Err(rollback) = self.egress.rollback_queued_to(queued_before) {
                return Err(DriverError::RollbackFailed {
                    operation,
                    rollback,
                });
            }
            return Err(DriverError::Buffer(operation));
        }
        Ok(TransactionResult::Committed(action))
    }

    /// Queues one already-formed packet body into bounded egress framing.
    ///
    /// The body must contain the packet-ID `VarInt` followed by the target packet payload. Target
    /// packet encoding remains outside this version-agnostic driver.
    ///
    /// # Errors
    ///
    /// Returns the underlying transactional egress/wire error.
    pub fn queue_frame<E>(&mut self, body: &[u8]) -> Result<(), DriverError<E>> {
        self.egress.queue_frame(body).map_err(DriverError::Buffer)
    }

    /// Atomically queues a complete batch of already-formed packet bodies.
    ///
    /// # Errors
    ///
    /// Returns the underlying batch validation/capacity error. Rejection leaves logical egress
    /// unchanged.
    pub fn queue_batch<E, B>(&mut self, bodies: &[B]) -> Result<(), DriverError<E>>
    where
        B: AsRef<[u8]>,
    {
        self.egress.queue_batch(bodies).map_err(DriverError::Buffer)
    }

    /// Contiguous encoded bytes currently ready for a socket write.
    #[must_use]
    pub fn pending_egress(&self) -> &[u8] {
        self.egress.pending()
    }

    /// Acknowledges exactly `bytes` reported written by the socket adapter.
    ///
    /// # Errors
    ///
    /// Returns an error instead of clamping an impossible write count.
    pub fn consume_written<E>(&mut self, bytes: usize) -> Result<(), DriverError<E>> {
        self.egress
            .consume_written(bytes)
            .map_err(DriverError::Buffer)
    }

    /// Number of active ingress bytes not yet committed by frame processing.
    #[must_use]
    pub fn buffered_ingress(&self) -> usize {
        self.ingress.buffered_len()
    }

    /// Number of encoded egress bytes not yet acknowledged as written.
    #[must_use]
    pub fn queued_egress(&self) -> usize {
        self.egress.queued_len()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionDriver, DriverError, FrameBudget, FrameFlow, OutboundBatch, StopReason,
        TransactionResult,
    };
    use helve_connection_core::{ConnectionBufferError, ConnectionLimits};

    #[derive(Debug, Eq, PartialEq)]
    struct PreparedAction {
        marker: u8,
        frames: Vec<Vec<u8>>,
    }

    impl OutboundBatch for PreparedAction {
        type Body = Vec<u8>;

        fn outbound_frames(&self) -> &[Self::Body] {
            &self.frames
        }
    }

    fn limits(max_buffer: usize) -> ConnectionLimits {
        ConnectionLimits::new(1_024, max_buffer, max_buffer).expect("valid test limits")
    }

    fn small_frame_limits(max_buffer: usize) -> ConnectionLimits {
        ConnectionLimits::new(16, max_buffer, max_buffer).expect("valid small-frame limits")
    }

    fn budget(frames: usize) -> FrameBudget {
        FrameBudget::new(frames).expect("positive test budget")
    }

    fn body(packet_id: u8, payload: &[u8]) -> Vec<u8> {
        assert!(packet_id < 0x80, "test helper uses one-byte packet ids");
        let mut body = Vec::with_capacity(1 + payload.len());
        body.push(packet_id);
        body.extend_from_slice(payload);
        body
    }

    fn encoded_stream(frames: &[(u8, &[u8])]) -> Vec<u8> {
        let capacity = frames
            .iter()
            .map(|(_, payload)| payload.len() + 3)
            .sum::<usize>()
            .max(4_096);
        let mut encoder = ConnectionDriver::new(limits(capacity));
        for &(packet_id, payload) in frames {
            encoder
                .queue_frame::<()>(body(packet_id, payload).as_slice())
                .expect("encode test frame");
        }
        encoder.pending_egress().to_vec()
    }

    fn run_mixed_stream(encoded: &[u8], fragment: usize, frame_budget: usize) -> (u64, usize) {
        let mut driver = ConnectionDriver::new(limits(encoded.len().max(4_096)));
        let mut checksum = 0xCBF2_9CE4_8422_2325u64;
        let mut frames = 0usize;
        for chunk in encoded.chunks(fragment) {
            driver.ingest::<()>(chunk).expect("fragment fits");
            loop {
                let report = driver
                    .process_available(budget(frame_budget), |frame| {
                        checksum ^= u64::from(frame.packet_id().cast_unsigned());
                        checksum = checksum.wrapping_mul(0x100_0000_01B3);
                        for &byte in frame.payload() {
                            checksum ^= u64::from(byte);
                            checksum = checksum.wrapping_mul(0x100_0000_01B3);
                        }
                        frames += 1;
                        Ok::<_, ()>(FrameFlow::Continue)
                    })
                    .expect("valid stream");
                if report.stop != StopReason::BudgetExhausted {
                    break;
                }
            }
        }
        loop {
            let report = driver
                .process_available(budget(frame_budget), |frame| {
                    checksum ^= u64::from(frame.packet_id().cast_unsigned());
                    checksum = checksum.wrapping_mul(0x100_0000_01B3);
                    for &byte in frame.payload() {
                        checksum ^= u64::from(byte);
                        checksum = checksum.wrapping_mul(0x100_0000_01B3);
                    }
                    frames += 1;
                    Ok::<_, ()>(FrameFlow::Continue)
                })
                .expect("drain stream");
            if report.stop != StopReason::BudgetExhausted {
                break;
            }
        }
        assert_eq!(driver.buffered_ingress(), 0);
        (checksum, frames)
    }

    #[test]
    fn zero_frame_budget_is_unrepresentable() {
        assert_eq!(FrameBudget::new(0), None);
        assert_eq!(FrameBudget::new(1).map(FrameBudget::get), Some(1));
    }

    #[test]
    fn byte_at_a_time_fragmentation_commits_only_complete_frame() {
        let encoded = encoded_stream(&[(7, b"crucible")]);
        let mut driver = ConnectionDriver::new(limits(4_096));

        for &byte in &encoded[..encoded.len() - 1] {
            driver.ingest::<()>(&[byte]).expect("bounded ingress");
            let report = driver
                .process_available(budget(8), |_| Ok::<_, ()>(FrameFlow::Continue))
                .expect("valid incomplete stream");
            assert_eq!(report.frames, 0);
            assert_eq!(report.stop, StopReason::Incomplete);
        }

        driver
            .ingest::<()>(&encoded[encoded.len() - 1..])
            .expect("terminal byte");
        let mut observed = None;
        let report = driver
            .process_available(budget(8), |frame| {
                observed = Some((frame.packet_id(), frame.payload().to_vec()));
                Ok::<_, ()>(FrameFlow::Continue)
            })
            .expect("complete frame");
        assert_eq!(observed, Some((7, b"crucible".to_vec())));
        assert_eq!(report.frames, 1);
        assert_eq!(report.stop, StopReason::Incomplete);
        assert_eq!(driver.buffered_ingress(), 0);
    }

    #[test]
    fn coalesced_frames_obey_explicit_poll_budget() {
        let encoded = encoded_stream(&[(1, b"a"), (2, b"b"), (3, b"c")]);
        let mut driver = ConnectionDriver::new(limits(4_096));
        driver.ingest::<()>(&encoded).expect("coalesced ingress");

        let mut first = Vec::new();
        let report = driver
            .process_available(budget(2), |frame| {
                first.push(frame.packet_id());
                Ok::<_, ()>(FrameFlow::Continue)
            })
            .expect("first poll");
        assert_eq!(first, vec![1, 2]);
        assert_eq!(report.frames, 2);
        assert_eq!(report.stop, StopReason::BudgetExhausted);
        assert!(driver.buffered_ingress() > 0);

        let mut second = Vec::new();
        let report = driver
            .process_available(budget(2), |frame| {
                second.push(frame.packet_id());
                Ok::<_, ()>(FrameFlow::Continue)
            })
            .expect("second poll");
        assert_eq!(second, vec![3]);
        assert_eq!(report.frames, 1);
        assert_eq!(report.stop, StopReason::Incomplete);
        assert_eq!(driver.buffered_ingress(), 0);
    }

    #[test]
    fn handler_error_preserves_current_frame_and_borrowed_storage() {
        let encoded = encoded_stream(&[(11, b"retry-me")]);
        let mut driver = ConnectionDriver::new(limits(4_096));
        driver.ingest::<()>(&encoded).expect("ingress");

        let mut first_pointer = None;
        let error = driver
            .process_available(budget(1), |frame| {
                first_pointer = Some(frame.payload().as_ptr() as usize);
                Err::<FrameFlow, _>("reject")
            })
            .expect_err("handler rejection must surface");
        assert_eq!(error, DriverError::Handler("reject"));
        assert_eq!(driver.buffered_ingress(), encoded.len());

        let mut second_pointer = None;
        let mut observed = None;
        let report = driver
            .process_available(budget(1), |frame| {
                second_pointer = Some(frame.payload().as_ptr() as usize);
                observed = Some((frame.packet_id(), frame.payload().to_vec()));
                Ok::<_, &str>(FrameFlow::Yield)
            })
            .expect("retry succeeds");
        assert_eq!(first_pointer, second_pointer);
        assert_eq!(observed, Some((11, b"retry-me".to_vec())));
        assert_eq!(report.stop, StopReason::HandlerYield);
        assert_eq!(driver.buffered_ingress(), 0);
    }

    #[test]
    fn handler_yield_commits_exactly_current_frame() {
        let encoded = encoded_stream(&[(4, b"first"), (5, b"second")]);
        let mut driver = ConnectionDriver::new(limits(4_096));
        driver.ingest::<()>(&encoded).expect("ingress");

        let mut ids = Vec::new();
        let report = driver
            .process_available(budget(8), |frame| {
                ids.push(frame.packet_id());
                Ok::<_, ()>(FrameFlow::Yield)
            })
            .expect("yield");
        assert_eq!(ids, vec![4]);
        assert_eq!(report.frames, 1);
        assert_eq!(report.stop, StopReason::HandlerYield);
        assert!(driver.buffered_ingress() > 0);
    }

    #[test]
    fn transactional_action_admits_complete_batch_before_consuming_ingress() {
        let encoded = encoded_stream(&[(9, b"request")]);
        let mut driver = ConnectionDriver::new(limits(4_096));
        driver.ingest::<()>(&encoded).expect("request ingress");

        let result = driver
            .process_one_transactional(|frame| {
                assert_eq!(frame.packet_id(), 9);
                assert_eq!(frame.payload(), b"request");
                Ok::<_, ()>(PreparedAction {
                    marker: 42,
                    frames: vec![body(10, b"first"), body(11, b"second")],
                })
            })
            .expect("transaction commits");
        let TransactionResult::Committed(action) = result else {
            panic!("complete frame must commit");
        };
        assert_eq!(action.marker, 42);
        assert_eq!(driver.buffered_ingress(), 0);
        assert!(driver.queued_egress() > 0);

        let outbound = driver.pending_egress().to_vec();
        let mut decoder = ConnectionDriver::new(limits(4_096));
        decoder.ingest::<()>(&outbound).expect("outbound loopback");
        let mut ids = Vec::new();
        let report = decoder
            .process_available(budget(8), |frame| {
                ids.push(frame.packet_id());
                Ok::<_, ()>(FrameFlow::Continue)
            })
            .expect("decode committed batch");
        assert_eq!(ids, vec![10, 11]);
        assert_eq!(report.frames, 2);
        assert_eq!(decoder.buffered_ingress(), 0);
    }

    #[test]
    fn transactional_capacity_rejection_preserves_ingress_and_existing_egress() {
        let limits = small_frame_limits(17);
        let request = {
            let mut encoder = ConnectionDriver::new(limits);
            encoder
                .queue_frame::<()>(&body(9, b"request"))
                .expect("request frame fits");
            encoder.pending_egress().to_vec()
        };
        let mut driver = ConnectionDriver::new(limits);
        driver
            .queue_frame::<()>(&body(1, b"old"))
            .expect("existing egress");
        let egress_before = driver.pending_egress().to_vec();
        driver.ingest::<()>(&request).expect("request ingress");
        let ingress_before = driver.buffered_ingress();

        let error = driver
            .process_one_transactional(|_| {
                Ok::<_, ()>(PreparedAction {
                    marker: 7,
                    frames: vec![body(10, b"1234567890"), body(11, b"abcdefghij")],
                })
            })
            .expect_err("aggregate egress capacity must reject action");
        assert!(matches!(
            error,
            DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(driver.buffered_ingress(), ingress_before);
        assert_eq!(driver.pending_egress(), egress_before);
    }

    #[test]
    fn transactional_handler_error_changes_neither_ingress_nor_egress() {
        let encoded = encoded_stream(&[(9, b"request")]);
        let mut driver = ConnectionDriver::new(limits(4_096));
        driver.ingest::<()>(&encoded).expect("request ingress");
        driver
            .queue_frame::<()>(&body(1, b"existing"))
            .expect("existing egress");
        let ingress_before = driver.buffered_ingress();
        let egress_before = driver.pending_egress().to_vec();

        let error = driver
            .process_one_transactional::<_, PreparedAction, _>(|_| Err("semantic rejection"))
            .expect_err("handler rejects");
        assert_eq!(error, DriverError::Handler("semantic rejection"));
        assert_eq!(driver.buffered_ingress(), ingress_before);
        assert_eq!(driver.pending_egress(), egress_before);
    }

    #[test]
    fn transactional_incomplete_input_is_a_noop() {
        let mut driver = ConnectionDriver::new(limits(4_096));
        let result = driver
            .process_one_transactional::<(), PreparedAction, _>(|_| {
                panic!("handler must not run without a complete frame")
            })
            .expect("incomplete is not an error");
        assert_eq!(result, TransactionResult::Incomplete);
        assert_eq!(driver.buffered_ingress(), 0);
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn direct_batch_queue_matches_individual_frame_queue() {
        let frames = [body(1, b"a"), body(2, b"bc"), body(3, b"def")];
        let mut individual = ConnectionDriver::new(limits(4_096));
        for frame in &frames {
            individual
                .queue_frame::<()>(frame)
                .expect("individual frame");
        }
        let mut batch = ConnectionDriver::new(limits(4_096));
        batch.queue_batch::<(), _>(&frames).expect("batch frames");
        assert_eq!(batch.pending_egress(), individual.pending_egress());
    }

    #[test]
    fn egress_backpressure_and_partial_write_accounting_remain_exact() {
        let mut driver = ConnectionDriver::new(limits(1_030));
        driver
            .queue_frame::<()>(&body(1, &[0xAB; 900]))
            .expect("first frame fits");
        let before = driver.pending_egress().to_vec();
        let error = driver
            .queue_frame::<()>(&body(2, &[0xCD; 900]))
            .expect_err("second frame exceeds bounded queue");
        assert!(matches!(
            error,
            DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(driver.pending_egress(), before.as_slice());

        let half = before.len() / 2;
        driver
            .consume_written::<()>(half)
            .expect("partial socket write");
        assert_eq!(driver.queued_egress(), before.len() - half);
        assert_eq!(driver.pending_egress(), &before[half..]);

        let error = driver
            .consume_written::<()>(driver.queued_egress() + 1)
            .expect_err("impossible socket count rejected");
        assert!(matches!(
            error,
            DriverError::Buffer(ConnectionBufferError::InvalidConsume { .. })
        ));
    }

    #[test]
    fn long_mixed_stream_is_deterministic_across_fragmentation_and_budgets() {
        const FRAME_COUNT: usize = 20_000;
        let payloads = (0..FRAME_COUNT)
            .map(|index| {
                let width = index % 31;
                (0..width)
                    .map(|offset| {
                        u8::try_from((index * 17 + offset * 29) & 0xFF).expect("masked byte")
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut descriptions = Vec::with_capacity(FRAME_COUNT);
        for (index, payload) in payloads.iter().enumerate() {
            let packet_id = u8::try_from(index % 97).expect("bounded packet id");
            descriptions.push((packet_id, payload.as_slice()));
        }
        let encoded = encoded_stream(&descriptions);

        let bytewise = run_mixed_stream(&encoded, 1, 1);
        let medium = run_mixed_stream(&encoded, 37, 7);
        let coalesced = run_mixed_stream(&encoded, encoded.len(), 257);
        assert_eq!(bytewise, medium);
        assert_eq!(medium, coalesced);
        assert_eq!(coalesced.1, FRAME_COUNT);
    }
}
