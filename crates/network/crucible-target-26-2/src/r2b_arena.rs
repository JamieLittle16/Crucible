//! Compact target-local storage for replay-free R2B dynamic bootstrap bodies.
//!
//! Commands and synchronized recipes remain process/composition-owned immutable artifacts. Every
//! other small bootstrap body can be serialized once through a reusable `PacketWriter`, copied into
//! this single contiguous arena, and then borrowed by span during staged publication. The arena owns
//! no semantic ordering and no socket queue.

use crucible_packet_core::PacketWriter;

/// Compact immutable location of one packet body within a dynamic bootstrap arena.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BodySpan {
    start: u32,
    len: u32,
}

/// Fail-closed construction error for one compact dynamic bootstrap arena.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicBootstrapArenaError {
    /// Packet bodies always contain at least their packet-id `VarInt`.
    EmptyBody,
    /// More bodies were supplied than the statically selected profile admits.
    TooManyBodies {
        /// Static maximum number of dynamic bodies in this arena.
        maximum: usize,
    },
    /// The contiguous arena grew beyond the compact 32-bit span representation.
    ArenaTooLarge,
    /// One individual body cannot be represented by the compact 32-bit span length.
    BodyTooLarge,
}

/// One-allocation owner for the selected profile's small player/dimension-dependent packet bodies.
///
/// `N` is fixed by the target/profile at compile time. Spans are stored inline and packet bytes are
/// contiguous, avoiding one heap allocation and allocator metadata entry per dynamic packet.
#[derive(Debug)]
pub(crate) struct DynamicBootstrapArena<const N: usize> {
    bytes: Vec<u8>,
    spans: [BodySpan; N],
    len: usize,
}

impl<const N: usize> DynamicBootstrapArena<N> {
    /// Creates an empty arena with caller-selected byte capacity.
    ///
    /// Capacity is a performance hint only; semantic/body-count bounds remain independent.
    #[must_use]
    pub(crate) fn with_capacity(byte_capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(byte_capacity),
            spans: [BodySpan::default(); N],
            len: 0,
        }
    }

    /// Number of dynamic packet bodies currently sealed into the arena.
    #[must_use]
    pub(crate) const fn body_count(&self) -> usize {
        self.len
    }

    /// Total encoded packet-body bytes retained by this arena.
    #[must_use]
    pub(crate) fn body_bytes(&self) -> usize {
        self.bytes.len()
    }

    /// Appends the current scratch packet body and resets the writer only after success.
    ///
    /// This is the intended construction path: one scratch writer allocation is retained across all
    /// dynamic codecs, while this arena performs the only long-lived byte allocation for those
    /// bodies. The body is copied exactly once into final contiguous storage.
    ///
    /// # Errors
    ///
    /// Fails before changing either arena or scratch writer when the body is empty, the static body
    /// count is exhausted, or compact span arithmetic cannot represent the result.
    pub(crate) fn seal_from(
        &mut self,
        scratch: &mut PacketWriter,
    ) -> Result<usize, DynamicBootstrapArenaError> {
        let body = scratch.as_slice();
        if body.is_empty() {
            return Err(DynamicBootstrapArenaError::EmptyBody);
        }
        if self.len == N {
            return Err(DynamicBootstrapArenaError::TooManyBodies { maximum: N });
        }

        let start = u32::try_from(self.bytes.len())
            .map_err(|_| DynamicBootstrapArenaError::ArenaTooLarge)?;
        let body_len =
            u32::try_from(body.len()).map_err(|_| DynamicBootstrapArenaError::BodyTooLarge)?;
        self.bytes
            .len()
            .checked_add(body.len())
            .and_then(|end| u32::try_from(end).ok())
            .ok_or(DynamicBootstrapArenaError::ArenaTooLarge)?;

        let index = self.len;
        self.bytes.extend_from_slice(body);
        self.spans[index] = BodySpan {
            start,
            len: body_len,
        };
        self.len = index + 1;
        scratch.reset();
        Ok(index)
    }

    /// Borrows one sealed packet body by stable insertion index.
    #[must_use]
    pub(crate) fn body(&self, index: usize) -> Option<&[u8]> {
        let span = *self.spans.get(index)?;
        if index >= self.len {
            return None;
        }
        let start = usize::try_from(span.start).ok()?;
        let len = usize::try_from(span.len).ok()?;
        let end = start.checked_add(len)?;
        self.bytes.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::PacketWriter;

    use super::{DynamicBootstrapArena, DynamicBootstrapArenaError};

    #[test]
    fn arena_seals_multiple_bodies_from_one_reused_scratch_writer() {
        let mut arena = DynamicBootstrapArena::<3>::with_capacity(16);
        let mut scratch = PacketWriter::new(8).expect("bounded scratch");

        scratch.write_var_int(1).expect("packet id");
        scratch.write_u8(0xaa).expect("payload");
        assert_eq!(arena.seal_from(&mut scratch), Ok(0));
        assert!(scratch.is_empty());

        scratch.write_var_int(2).expect("packet id");
        scratch.write_i32(-7).expect("payload");
        assert_eq!(arena.seal_from(&mut scratch), Ok(1));
        assert!(scratch.is_empty());

        assert_eq!(arena.body_count(), 2);
        assert_eq!(arena.body_bytes(), 7);
        assert_eq!(arena.body(0), Some(&[0x01, 0xaa][..]));
        assert_eq!(arena.body(1), Some(&[0x02, 0xff, 0xff, 0xff, 0xf9][..]));
        assert_eq!(arena.body(2), None);
    }

    #[test]
    fn empty_body_rejection_preserves_both_owners() {
        let mut arena = DynamicBootstrapArena::<1>::with_capacity(8);
        let mut scratch = PacketWriter::new(8).expect("bounded scratch");

        assert_eq!(
            arena.seal_from(&mut scratch),
            Err(DynamicBootstrapArenaError::EmptyBody)
        );
        assert_eq!(arena.body_count(), 0);
        assert_eq!(arena.body_bytes(), 0);
        assert!(scratch.is_empty());
    }

    #[test]
    fn body_count_overflow_does_not_clear_uncommitted_scratch() {
        let mut arena = DynamicBootstrapArena::<1>::with_capacity(8);
        let mut scratch = PacketWriter::new(8).expect("bounded scratch");

        scratch.write_var_int(1).expect("first packet");
        arena.seal_from(&mut scratch).expect("first body");

        scratch.write_var_int(2).expect("second packet");
        scratch.write_u8(0xbb).expect("payload");
        let pending = scratch.as_slice().to_vec();
        assert_eq!(
            arena.seal_from(&mut scratch),
            Err(DynamicBootstrapArenaError::TooManyBodies { maximum: 1 })
        );
        assert_eq!(arena.body_count(), 1);
        assert_eq!(arena.body(0), Some(&[0x01][..]));
        assert_eq!(scratch.as_slice(), pending);
    }

    #[test]
    fn zero_body_profile_always_fails_closed() {
        let mut arena = DynamicBootstrapArena::<0>::with_capacity(0);
        let mut scratch = PacketWriter::new(1).expect("bounded scratch");
        scratch.write_var_int(1).expect("packet id");

        assert_eq!(
            arena.seal_from(&mut scratch),
            Err(DynamicBootstrapArenaError::TooManyBodies { maximum: 0 })
        );
        assert_eq!(arena.body_count(), 0);
        assert_eq!(arena.body_bytes(), 0);
        assert_eq!(scratch.as_slice(), &[0x01]);
    }
}
