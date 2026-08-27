//! Bounded connection-buffer mechanics below Crucible's versioned Minecraft state machine.
//!
//! This crate owns no packet IDs, connection-state transitions, authentication policy or socket
//! runtime. It turns arbitrary TCP byte fragments into borrowed framed packet views and provides a
//! bounded transactional outbound queue over `helve-protocol-core`.
//!
//! The reference mechanism deliberately uses compacting `Vec<u8>` buffers. Complete frame bodies
//! and packet payloads are borrowed directly from ingress storage: parsing does not allocate or copy
//! per frame. Alternate rings, slabs, pools, vectored I/O or runtime-specific buffers must beat this
//! mechanism under whole-cost qualification before replacing it.

#![forbid(unsafe_code)]

use helve_protocol_core::{
    DecodeResult, MAX_FRAME_BODY_LEN, WireError, decode_frame, decode_var_int, encode_frame,
    var_int_len,
};

/// Identifies one bounded byte store when reporting an invalid configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferKind {
    /// Bytes received from the peer but not yet consumed.
    Ingress,
    /// Encoded bytes queued for the socket writer.
    Egress,
}

/// Fail-closed connection-buffer errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionBufferError {
    /// A configured byte limit was zero.
    ZeroLimit { buffer: BufferKind },
    /// The configured maximum frame body exceeds Minecraft's framing ceiling.
    FrameLimitTooLarge { requested: usize, maximum: usize },
    /// A buffer bound cannot hold one frame at the configured maximum body size.
    BufferCannotHoldMaximumFrame {
        buffer: BufferKind,
        configured: usize,
        required: usize,
    },
    /// Appending received bytes would exceed the active ingress bound.
    IngressLimitExceeded {
        buffered: usize,
        incoming: usize,
        maximum: usize,
    },
    /// Queueing one frame or one complete frame batch would exceed the egress bound.
    EgressLimitExceeded {
        queued: usize,
        frame_bytes: usize,
        maximum: usize,
    },
    /// The caller attempted to consume more active bytes than exist.
    InvalidConsume { requested: usize, available: usize },
    /// A transaction attempted to roll egress back to more bytes than are currently queued.
    InvalidEgressRollback { requested: usize, available: usize },
    /// The outer frame is complete but its packet-ID `VarInt` ends prematurely.
    TruncatedPacketId,
    /// Integer arithmetic required to preserve a resource bound overflowed `usize`.
    LengthOverflow,
    /// The underlying Minecraft wire law rejected the bytes.
    Wire(WireError),
}

impl From<WireError> for ConnectionBufferError {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

/// Coherent byte limits for one connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionLimits {
    frame_body_len: usize,
    ingress_buffered: usize,
    egress_queued: usize,
}

impl ConnectionLimits {
    /// Constructs a coherent set of connection byte limits.
    ///
    /// Both ingress and egress bounds must be large enough to hold one maximum-sized encoded frame.
    /// This keeps `max_frame_body_len` meaningful instead of admitting frames that can never fit in
    /// the configured connection buffers.
    ///
    /// # Errors
    ///
    /// Returns an error for zero limits, a frame body above the Minecraft `VarInt21` ceiling, or a
    /// buffer that cannot hold one complete maximum-sized frame.
    pub fn new(
        max_frame_body_len: usize,
        max_ingress_buffered: usize,
        max_egress_queued: usize,
    ) -> Result<Self, ConnectionBufferError> {
        if max_frame_body_len == 0 {
            return Err(ConnectionBufferError::ZeroLimit {
                buffer: BufferKind::Ingress,
            });
        }
        if max_ingress_buffered == 0 {
            return Err(ConnectionBufferError::ZeroLimit {
                buffer: BufferKind::Ingress,
            });
        }
        if max_egress_queued == 0 {
            return Err(ConnectionBufferError::ZeroLimit {
                buffer: BufferKind::Egress,
            });
        }
        if max_frame_body_len > MAX_FRAME_BODY_LEN {
            return Err(ConnectionBufferError::FrameLimitTooLarge {
                requested: max_frame_body_len,
                maximum: MAX_FRAME_BODY_LEN,
            });
        }
        let required = encoded_frame_len(max_frame_body_len)?;
        if max_ingress_buffered < required {
            return Err(ConnectionBufferError::BufferCannotHoldMaximumFrame {
                buffer: BufferKind::Ingress,
                configured: max_ingress_buffered,
                required,
            });
        }
        if max_egress_queued < required {
            return Err(ConnectionBufferError::BufferCannotHoldMaximumFrame {
                buffer: BufferKind::Egress,
                configured: max_egress_queued,
                required,
            });
        }
        Ok(Self {
            frame_body_len: max_frame_body_len,
            ingress_buffered: max_ingress_buffered,
            egress_queued: max_egress_queued,
        })
    }

    /// Maximum decoded frame-body bytes accepted from or emitted to this connection.
    #[must_use]
    pub const fn max_frame_body_len(self) -> usize {
        self.frame_body_len
    }

    /// Maximum unconsumed ingress bytes retained for this connection.
    #[must_use]
    pub const fn max_ingress_buffered(self) -> usize {
        self.ingress_buffered
    }

    /// Maximum unwritten egress bytes retained for this connection.
    #[must_use]
    pub const fn max_egress_queued(self) -> usize {
        self.egress_queued
    }
}

/// Borrowed view of one complete Minecraft packet frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameView<'a> {
    packet_id: i32,
    payload: &'a [u8],
    body_bytes: usize,
    stream_bytes: usize,
}

impl<'a> FrameView<'a> {
    /// Signed packet identity encoded at the start of the frame body.
    ///
    /// Target-version packet law decides whether a particular value is legal in the current state.
    #[must_use]
    pub const fn packet_id(self) -> i32 {
        self.packet_id
    }

    /// Packet payload bytes after the packet-ID `VarInt`.
    #[must_use]
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }

    /// Decoded frame-body length, including the packet-ID bytes.
    #[must_use]
    pub const fn body_bytes(self) -> usize {
        self.body_bytes
    }

    /// Exact number of TCP-stream bytes occupied by this frame, including its length prefix.
    #[must_use]
    pub const fn stream_bytes(self) -> usize {
        self.stream_bytes
    }
}

/// Bounded fragmented-stream ingress storage.
#[derive(Debug)]
pub struct IngressBuffer {
    bytes: Vec<u8>,
    start: usize,
    limits: ConnectionLimits,
}

impl IngressBuffer {
    /// Creates an empty ingress buffer under `limits`.
    #[must_use]
    pub const fn new(limits: ConnectionLimits) -> Self {
        Self {
            bytes: Vec::new(),
            start: 0,
            limits,
        }
    }

    /// Number of unconsumed bytes currently retained.
    #[must_use]
    pub fn buffered_len(&self) -> usize {
        self.bytes.len() - self.start
    }

    /// Whether no unconsumed ingress bytes remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffered_len() == 0
    }

    /// Appends one socket-read fragment transactionally with respect to the logical byte bound.
    ///
    /// Consumed prefix bytes may be compacted before the append. Active bytes are never discarded,
    /// and rejected input leaves the logical buffered stream unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when active bytes plus `incoming` would exceed the configured ingress bound
    /// or when the size calculation overflows.
    pub fn push(&mut self, incoming: &[u8]) -> Result<(), ConnectionBufferError> {
        if incoming.is_empty() {
            return Ok(());
        }
        let buffered = self.buffered_len();
        let required = buffered
            .checked_add(incoming.len())
            .ok_or(ConnectionBufferError::LengthOverflow)?;
        if required > self.limits.ingress_buffered {
            return Err(ConnectionBufferError::IngressLimitExceeded {
                buffered,
                incoming: incoming.len(),
                maximum: self.limits.ingress_buffered,
            });
        }
        self.compact_before_append(incoming.len())?;
        self.bytes.extend_from_slice(incoming);
        Ok(())
    }

    /// Returns the next complete frame as a borrowed zero-copy view.
    ///
    /// `Ok(None)` means the current TCP fragment is incomplete but could still become valid. A
    /// complete outer frame whose packet-ID `VarInt` is truncated is malformed rather than
    /// incomplete, because no later stream byte belongs to that frame body.
    ///
    /// # Errors
    ///
    /// Returns underlying framing/VarInt errors or [`ConnectionBufferError::TruncatedPacketId`].
    pub fn peek_frame(&self) -> Result<Option<FrameView<'_>>, ConnectionBufferError> {
        let active = &self.bytes[self.start..];
        let DecodeResult::Complete {
            value: body,
            consumed: stream_bytes,
        } = decode_frame(active, self.limits.frame_body_len)?
        else {
            return Ok(None);
        };

        let DecodeResult::Complete {
            value: packet_id,
            consumed: packet_id_bytes,
        } = decode_var_int(body)?
        else {
            return Err(ConnectionBufferError::TruncatedPacketId);
        };
        let payload = body
            .get(packet_id_bytes..)
            .ok_or(ConnectionBufferError::TruncatedPacketId)?;
        Ok(Some(FrameView {
            packet_id,
            payload,
            body_bytes: body.len(),
            stream_bytes,
        }))
    }

    /// Consumes exactly `bytes` active stream bytes after the caller has processed a borrowed view.
    ///
    /// # Errors
    ///
    /// Returns an error instead of silently clamping when `bytes` exceeds the active buffer.
    pub fn consume(&mut self, bytes: usize) -> Result<(), ConnectionBufferError> {
        let available = self.buffered_len();
        if bytes > available {
            return Err(ConnectionBufferError::InvalidConsume {
                requested: bytes,
                available,
            });
        }
        self.start += bytes;
        if self.start == self.bytes.len() {
            self.bytes.clear();
            self.start = 0;
        }
        Ok(())
    }

    fn compact_before_append(&mut self, incoming: usize) -> Result<(), ConnectionBufferError> {
        if self.start == 0 {
            return Ok(());
        }
        let physical_after = self
            .bytes
            .len()
            .checked_add(incoming)
            .ok_or(ConnectionBufferError::LengthOverflow)?;
        let half_consumed = self.start >= self.bytes.len() / 2;
        if physical_after > self.limits.ingress_buffered || half_consumed {
            let active = self.buffered_len();
            self.bytes.copy_within(self.start.., 0);
            self.bytes.truncate(active);
            self.start = 0;
        }
        Ok(())
    }
}

/// Bounded encoded-byte queue for a socket writer.
#[derive(Debug)]
pub struct EgressBuffer {
    bytes: Vec<u8>,
    start: usize,
    limits: ConnectionLimits,
}

impl EgressBuffer {
    /// Creates an empty egress queue under `limits`.
    #[must_use]
    pub const fn new(limits: ConnectionLimits) -> Self {
        Self {
            bytes: Vec::new(),
            start: 0,
            limits,
        }
    }

    /// Number of encoded bytes not yet acknowledged as written.
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.bytes.len() - self.start
    }

    /// Whether no encoded bytes remain queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queued_len() == 0
    }

    /// Returns the contiguous bytes currently available to write to the socket.
    #[must_use]
    pub fn pending(&self) -> &[u8] {
        &self.bytes[self.start..]
    }

    /// Encodes and queues one complete frame transactionally with respect to the egress bound.
    ///
    /// # Errors
    ///
    /// Returns a wire error for an invalid frame body, an egress-bound error when the encoded frame
    /// does not fit, or a length-overflow error. Rejection leaves the logical queued bytes unchanged.
    pub fn queue_frame(&mut self, body: &[u8]) -> Result<(), ConnectionBufferError> {
        validate_frame_body(body, self.limits.frame_body_len)?;
        let frame_bytes = encoded_frame_len(body.len())?;
        let queued = self.queued_len();
        let required = queued
            .checked_add(frame_bytes)
            .ok_or(ConnectionBufferError::LengthOverflow)?;
        if required > self.limits.egress_queued {
            return Err(ConnectionBufferError::EgressLimitExceeded {
                queued,
                frame_bytes,
                maximum: self.limits.egress_queued,
            });
        }
        self.compact_before_append(frame_bytes)?;
        encode_frame(body, self.limits.frame_body_len, &mut self.bytes)?;
        Ok(())
    }

    /// Atomically validates, admits, and queues a complete batch of frame bodies.
    ///
    /// The complete encoded byte cost is computed before any mutation. Capacity is checked for the
    /// batch as a whole, consumed-prefix compaction occurs at most once, and an unexpected encoder
    /// failure rolls the appended tail back to the exact pre-batch logical queue.
    ///
    /// This is a control-boundary primitive for semantic actions that require multiple outbound
    /// packets. It introduces no secondary staging queue and does not change the single-frame HOT
    /// path in [`Self::queue_frame`].
    ///
    /// # Errors
    ///
    /// Returns the first invalid frame-body/wire error, a length-overflow error, or one aggregate
    /// egress-bound error. Every rejection leaves the logical queued bytes unchanged.
    pub fn queue_batch<B>(&mut self, bodies: &[B]) -> Result<(), ConnectionBufferError>
    where
        B: AsRef<[u8]>,
    {
        let mut batch_bytes = 0usize;
        for body in bodies {
            let body = body.as_ref();
            validate_frame_body(body, self.limits.frame_body_len)?;
            batch_bytes = batch_bytes
                .checked_add(encoded_frame_len(body.len())?)
                .ok_or(ConnectionBufferError::LengthOverflow)?;
        }
        if batch_bytes == 0 {
            return Ok(());
        }

        let queued = self.queued_len();
        let required = queued
            .checked_add(batch_bytes)
            .ok_or(ConnectionBufferError::LengthOverflow)?;
        if required > self.limits.egress_queued {
            return Err(ConnectionBufferError::EgressLimitExceeded {
                queued,
                frame_bytes: batch_bytes,
                maximum: self.limits.egress_queued,
            });
        }

        self.compact_before_append(batch_bytes)?;
        let append_start = self.bytes.len();
        for body in bodies {
            if let Err(error) =
                encode_frame(body.as_ref(), self.limits.frame_body_len, &mut self.bytes)
            {
                self.bytes.truncate(append_start);
                return Err(error.into());
            }
        }
        Ok(())
    }

    /// Restores the logical egress queue to an earlier retained byte length by dropping only tail
    /// bytes.
    ///
    /// This narrow primitive exists so a higher-level transaction coordinator can undo a newly
    /// admitted outbound batch if a later invariant-preserving commit step unexpectedly fails. It
    /// never reconstructs consumed socket bytes and cannot increase the logical queue.
    ///
    /// # Errors
    ///
    /// Returns an error if `queued_len` exceeds the currently queued logical byte count or if index
    /// arithmetic overflows. Rejection leaves the queue unchanged.
    pub fn rollback_queued_to(&mut self, queued_len: usize) -> Result<(), ConnectionBufferError> {
        let available = self.queued_len();
        if queued_len > available {
            return Err(ConnectionBufferError::InvalidEgressRollback {
                requested: queued_len,
                available,
            });
        }
        if queued_len == 0 {
            self.bytes.clear();
            self.start = 0;
            return Ok(());
        }
        let physical_len = self
            .start
            .checked_add(queued_len)
            .ok_or(ConnectionBufferError::LengthOverflow)?;
        self.bytes.truncate(physical_len);
        Ok(())
    }

    /// Consumes exactly the number of bytes reported written by the socket adapter.
    ///
    /// # Errors
    ///
    /// Returns an error instead of silently clamping an impossible write count.
    pub fn consume_written(&mut self, bytes: usize) -> Result<(), ConnectionBufferError> {
        let available = self.queued_len();
        if bytes > available {
            return Err(ConnectionBufferError::InvalidConsume {
                requested: bytes,
                available,
            });
        }
        self.start += bytes;
        if self.start == self.bytes.len() {
            self.bytes.clear();
            self.start = 0;
        }
        Ok(())
    }

    fn compact_before_append(&mut self, frame_bytes: usize) -> Result<(), ConnectionBufferError> {
        if self.start == 0 {
            return Ok(());
        }
        let physical_after = self
            .bytes
            .len()
            .checked_add(frame_bytes)
            .ok_or(ConnectionBufferError::LengthOverflow)?;
        let half_consumed = self.start >= self.bytes.len() / 2;
        if physical_after > self.limits.egress_queued || half_consumed {
            let active = self.queued_len();
            self.bytes.copy_within(self.start.., 0);
            self.bytes.truncate(active);
            self.start = 0;
        }
        Ok(())
    }
}

fn validate_frame_body(body: &[u8], max_body_len: usize) -> Result<(), ConnectionBufferError> {
    if body.is_empty() {
        return Err(WireError::ZeroLengthFrame.into());
    }
    let maximum = max_body_len.min(MAX_FRAME_BODY_LEN);
    if body.len() > maximum {
        return Err(WireError::ByteLengthLimitExceeded {
            length: body.len(),
            max: maximum,
        }
        .into());
    }
    Ok(())
}

fn encoded_frame_len(body_len: usize) -> Result<usize, ConnectionBufferError> {
    let signed = i32::try_from(body_len).map_err(|_| {
        ConnectionBufferError::Wire(WireError::LengthDoesNotFitVarInt { length: body_len })
    })?;
    var_int_len(signed)
        .checked_add(body_len)
        .ok_or(ConnectionBufferError::LengthOverflow)
}

#[cfg(test)]
mod tests {
    use helve_protocol_core::{WireError, encode_frame, encode_var_int};

    use super::{BufferKind, ConnectionBufferError, ConnectionLimits, EgressBuffer, IngressBuffer};

    fn limits() -> ConnectionLimits {
        ConnectionLimits::new(1_024, 4_096, 4_096).expect("valid test limits")
    }

    fn packet_body(packet_id: i32, payload: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        encode_var_int(packet_id, &mut body);
        body.extend_from_slice(payload);
        body
    }

    fn framed(packet_id: i32, payload: &[u8]) -> Vec<u8> {
        let body = packet_body(packet_id, payload);
        let mut encoded = Vec::new();
        encode_frame(&body, 1_024, &mut encoded).expect("valid frame");
        encoded
    }

    #[test]
    fn fragmented_frame_is_borrowed_only_when_complete() {
        let encoded = framed(0x35, b"crucible");
        let mut ingress = IngressBuffer::new(limits());
        for &byte in &encoded[..encoded.len() - 1] {
            ingress.push(&[byte]).expect("bounded append");
            assert_eq!(ingress.peek_frame().expect("valid fragment"), None);
        }
        ingress
            .push(&encoded[encoded.len() - 1..])
            .expect("terminal byte");
        let frame = ingress
            .peek_frame()
            .expect("valid frame")
            .expect("complete frame");
        assert_eq!(frame.packet_id(), 0x35);
        assert_eq!(frame.payload(), b"crucible");
        assert_eq!(frame.stream_bytes(), encoded.len());
        assert_eq!(frame.body_bytes(), packet_body(0x35, b"crucible").len());
    }

    #[test]
    fn coalesced_frames_are_consumed_exactly() {
        let first = framed(1, b"abc");
        let second = framed(2, b"xyz");
        let mut ingress = IngressBuffer::new(limits());
        let mut stream = first.clone();
        stream.extend_from_slice(&second);
        ingress.push(&stream).expect("coalesced stream");

        let first_view = ingress.peek_frame().expect("first valid").expect("first");
        assert_eq!(first_view.packet_id(), 1);
        assert_eq!(first_view.payload(), b"abc");
        let consumed = first_view.stream_bytes();
        ingress.consume(consumed).expect("consume first");

        let second_view = ingress.peek_frame().expect("second valid").expect("second");
        assert_eq!(second_view.packet_id(), 2);
        assert_eq!(second_view.payload(), b"xyz");
        let consumed = second_view.stream_bytes();
        ingress.consume(consumed).expect("consume second");
        assert!(ingress.is_empty());
    }

    #[test]
    fn complete_outer_frame_with_truncated_packet_id_is_malformed() {
        let mut encoded = Vec::new();
        encode_frame(&[0x80], 1_024, &mut encoded).expect("outer frame valid");
        let mut ingress = IngressBuffer::new(limits());
        ingress.push(&encoded).expect("bounded frame");
        assert_eq!(
            ingress.peek_frame(),
            Err(ConnectionBufferError::TruncatedPacketId)
        );
    }

    #[test]
    fn overlong_packet_id_propagates_wire_error() {
        let mut encoded = Vec::new();
        encode_frame(&[0x80, 0x80, 0x80, 0x80, 0x80], 1_024, &mut encoded)
            .expect("outer frame valid");
        let mut ingress = IngressBuffer::new(limits());
        ingress.push(&encoded).expect("bounded frame");
        assert_eq!(
            ingress.peek_frame(),
            Err(ConnectionBufferError::Wire(WireError::VarIntTooLong))
        );
    }

    #[test]
    fn ingress_limit_rejection_is_transactional() {
        let limits = ConnectionLimits::new(8, 9, 9).expect("one max frame fits");
        let mut ingress = IngressBuffer::new(limits);
        ingress.push(b"12345678").expect("within bound");
        assert_eq!(
            ingress.push(b"ab"),
            Err(ConnectionBufferError::IngressLimitExceeded {
                buffered: 8,
                incoming: 2,
                maximum: 9,
            })
        );
        assert_eq!(ingress.buffered_len(), 8);
    }

    #[test]
    fn egress_bound_and_partial_write_are_exact() {
        let limits = ConnectionLimits::new(8, 16, 10).expect("valid limits");
        let mut egress = EgressBuffer::new(limits);
        let body = packet_body(1, b"abc");
        egress.queue_frame(&body).expect("first frame");
        let first = egress.pending().to_vec();
        assert!(!first.is_empty());
        egress.consume_written(2).expect("partial write");
        assert_eq!(egress.pending(), &first[2..]);

        let queued_before = egress.pending().to_vec();
        let too_large_for_remaining = packet_body(2, b"1234567");
        assert!(matches!(
            egress.queue_frame(&too_large_for_remaining),
            Err(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(egress.pending(), queued_before);
    }

    #[test]
    fn batch_queue_matches_sequential_encoding_exactly() {
        let bodies = [
            packet_body(1, b"alpha"),
            packet_body(2, b"beta"),
            packet_body(3, b"gamma"),
        ];
        let mut sequential = EgressBuffer::new(limits());
        for body in &bodies {
            sequential.queue_frame(body).expect("sequential frame");
        }
        let mut batch = EgressBuffer::new(limits());
        batch.queue_batch(&bodies).expect("batch fits");
        assert_eq!(batch.pending(), sequential.pending());
    }

    #[test]
    fn batch_admission_is_exact_fit_and_one_byte_over_fail_closed() {
        let tight = ConnectionLimits::new(8, 16, 9).expect("valid tight limits");
        let existing = packet_body(1, b"a");
        let batch_bodies = [packet_body(2, b"b"), packet_body(3, b"c")];
        let mut egress = EgressBuffer::new(tight);
        egress
            .queue_frame(&existing)
            .expect("three-byte existing frame");
        egress
            .queue_batch(&batch_bodies)
            .expect("exact nine-byte fit");
        assert_eq!(egress.queued_len(), 9);

        let before = egress.pending().to_vec();
        assert!(matches!(
            egress.queue_batch(&[packet_body(4, b"d")]),
            Err(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(egress.pending(), before);
    }

    #[test]
    fn malformed_later_batch_body_leaves_existing_egress_untouched() {
        let mut egress = EgressBuffer::new(limits());
        egress
            .queue_frame(&packet_body(1, b"existing"))
            .expect("existing frame");
        let before = egress.pending().to_vec();
        let bodies = [
            packet_body(2, b"valid"),
            Vec::new(),
            packet_body(3, b"later"),
        ];
        assert_eq!(
            egress.queue_batch(&bodies),
            Err(ConnectionBufferError::Wire(WireError::ZeroLengthFrame))
        );
        assert_eq!(egress.pending(), before);
    }

    #[test]
    fn batch_rejection_after_partial_write_preserves_active_prefix() {
        let tight = ConnectionLimits::new(8, 16, 10).expect("valid tight limits");
        let mut egress = EgressBuffer::new(tight);
        egress
            .queue_frame(&packet_body(1, b"abc"))
            .expect("initial frame");
        egress.consume_written(2).expect("partial write");
        let before = egress.pending().to_vec();
        let bodies = [packet_body(2, b"abcd"), packet_body(3, b"efgh")];
        assert!(matches!(
            egress.queue_batch(&bodies),
            Err(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(egress.pending(), before);
    }

    #[test]
    fn explicit_egress_rollback_drops_only_new_tail_after_compaction() {
        let mut egress = EgressBuffer::new(limits());
        egress
            .queue_frame(&packet_body(1, b"first"))
            .expect("first");
        let first_len = egress.queued_len();
        egress
            .queue_frame(&packet_body(2, b"second"))
            .expect("second");
        egress.consume_written(first_len).expect("consume first");
        let retained = egress.pending().to_vec();
        let queued_before = egress.queued_len();
        egress
            .queue_batch(&[packet_body(3, b"third"), packet_body(4, b"fourth")])
            .expect("batch after consumed prefix");
        egress
            .rollback_queued_to(queued_before)
            .expect("rollback new tail");
        assert_eq!(egress.pending(), retained);
        assert_eq!(
            egress.rollback_queued_to(queued_before + 1),
            Err(ConnectionBufferError::InvalidEgressRollback {
                requested: queued_before + 1,
                available: queued_before,
            })
        );
    }

    #[test]
    fn empty_batch_is_a_noop() {
        let mut egress = EgressBuffer::new(limits());
        egress
            .queue_frame(&packet_body(1, b"existing"))
            .expect("existing");
        let before = egress.pending().to_vec();
        let empty: [Vec<u8>; 0] = [];
        egress.queue_batch(&empty).expect("empty batch");
        assert_eq!(egress.pending(), before);
    }

    #[test]
    fn compaction_preserves_active_ingress_and_egress_bytes() {
        let mut ingress = IngressBuffer::new(limits());
        let first = framed(1, b"a");
        let second = framed(2, b"second");
        let mut both = first.clone();
        both.extend_from_slice(&second);
        ingress.push(&both).expect("initial stream");
        ingress.consume(first.len()).expect("consume first");
        ingress
            .push(&framed(3, b"third"))
            .expect("append after prefix");
        let view = ingress
            .peek_frame()
            .expect("valid")
            .expect("second retained");
        assert_eq!(view.packet_id(), 2);
        assert_eq!(view.payload(), b"second");

        let mut egress = EgressBuffer::new(limits());
        let body1 = packet_body(1, b"one");
        let body2 = packet_body(2, b"two");
        egress.queue_frame(&body1).expect("one");
        let first_len = egress.pending().len();
        egress.consume_written(first_len).expect("drain one");
        egress.queue_frame(&body2).expect("two");
        let pending = egress.pending().to_vec();
        assert!(!pending.is_empty());
        egress.consume_written(pending.len()).expect("drain two");
        assert!(egress.is_empty());
    }

    #[test]
    fn invalid_consumption_and_limit_sets_fail_closed() {
        let mut ingress = IngressBuffer::new(limits());
        ingress.push(b"abc").expect("append");
        assert_eq!(
            ingress.consume(4),
            Err(ConnectionBufferError::InvalidConsume {
                requested: 4,
                available: 3,
            })
        );
        assert_eq!(ingress.buffered_len(), 3);

        assert_eq!(
            ConnectionLimits::new(1_024, 1, 4_096),
            Err(ConnectionBufferError::BufferCannotHoldMaximumFrame {
                buffer: BufferKind::Ingress,
                configured: 1,
                required: 1_026,
            })
        );
        assert_eq!(
            ConnectionLimits::new(super::MAX_FRAME_BODY_LEN + 1, 4_000_000, 4_000_000),
            Err(ConnectionBufferError::FrameLimitTooLarge {
                requested: super::MAX_FRAME_BODY_LEN + 1,
                maximum: super::MAX_FRAME_BODY_LEN,
            })
        );
    }

    #[test]
    fn queued_frame_bytes_roundtrip_through_ingress_without_copy_contract_changes() {
        let body = packet_body(7, b"payload");
        let mut egress = EgressBuffer::new(limits());
        egress.queue_frame(&body).expect("queue");

        let mut ingress = IngressBuffer::new(limits());
        ingress.push(egress.pending()).expect("loopback");
        let frame = ingress.peek_frame().expect("valid").expect("complete");
        assert_eq!(frame.packet_id(), 7);
        assert_eq!(frame.payload(), b"payload");
    }
}
