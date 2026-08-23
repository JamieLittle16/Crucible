//! Runtime-neutral bounded connection driving for Crucible.
//!
//! This crate sits above `crucible-connection-core` and below target-version packet semantics. It
//! provides a small, statically dispatched processing loop over borrowed frame views while keeping
//! ingress/egress bounds and exact stream consumption explicit.
//!
//! It deliberately does not choose a socket runtime, packet registry, protocol version, session
//! state machine, authentication policy, compression policy, allocator, or scheduler.

#![forbid(unsafe_code)]

use std::num::NonZeroUsize;

use crucible_connection_core::{
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
    /// stream-consumption semantics independent from response policy.
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
    use super::{ConnectionDriver, DriverError, FrameBudget, FrameFlow, StopReason};
    use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};

    fn limits(max_buffer: usize) -> ConnectionLimits {
        ConnectionLimits::new(1_024, max_buffer, max_buffer).expect("valid test limits")
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
