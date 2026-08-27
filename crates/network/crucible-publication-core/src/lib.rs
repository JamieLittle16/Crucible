//! Target-neutral bounded progression over immutable outbound packet bodies.
//!
//! This crate handles the narrow class of work where a semantic decision has already committed and
//! a potentially large ordered publication must subsequently enter the existing bounded connection
//! egress over multiple service opportunities. It does not own the publication image, framing,
//! target packet identity, socket I/O, scheduling, compression, or semantic state machine.
//!
//! The production shape is deliberately small:
//!
//! ```text
//! caller-owned immutable bodies
//!          +
//! one-word PublicationCursor
//!          +
//! existing ConnectionDriver bounded egress
//! ```
//!
//! Naturally bounded inbound-response actions remain the responsibility of the driver's existing
//! atomic transaction path. This primitive must not be used to weaken those semantics.

#![forbid(unsafe_code)]

mod staged;

pub use staged::{
    StagedPublicationCursor, StagedPublicationLookup, StagedPublicationPlan, StagedPublicationStep,
    publish_staged_one, publish_staged_plan_one,
};

use crucible_connection_driver::{ConnectionDriver, DriverError};

/// Allocation-free per-connection progress through one ordered immutable publication.
///
/// The cursor stores the index of the next body that has not yet been admitted to bounded egress.
/// It owns no publication bytes and may therefore be copied as ordinary target-local state.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PublicationCursor {
    next: usize,
}

impl PublicationCursor {
    /// Cursor positioned before the first publication body.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// Index of the next body not yet admitted to bounded egress.
    #[must_use]
    pub const fn next_index(self) -> usize {
        self.next
    }

    /// Whether all bodies in `publication` have already been admitted.
    #[must_use]
    pub const fn is_complete<B>(self, publication: &[B]) -> bool {
        self.next >= publication.len()
    }
}

/// Result of one finite publication service step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationStep {
    /// The cursor was already complete; neither cursor nor egress changed.
    Complete,
    /// Exactly one body was admitted and the cursor advanced exactly once.
    Queued {
        /// Index of the body admitted by this step.
        index: usize,
        /// Packet-body bytes admitted, excluding the outer frame-length prefix.
        body_bytes: usize,
    },
}

/// Admits at most one borrowed publication body through the real bounded connection egress.
///
/// `publication` is only borrowed for this call. Ownership and sharing policy remain with the
/// caller, allowing static generated arrays, boxed immutable images, `Arc`-backed composition data,
/// or another representation to compete without changing this primitive.
///
/// The cursor advances only after [`ConnectionDriver::queue_frame`] succeeds. Consequently a wire
/// validation failure or egress-capacity rejection leaves the cursor unchanged and inherits the
/// driver's existing fail-closed egress guarantee. One call performs at most one frame admission,
/// making scheduling/fairness work explicit at the caller.
///
/// # Errors
///
/// Returns the underlying driver error for the next body without advancing `cursor`.
pub fn publish_one<E, B>(
    publication: &[B],
    cursor: &mut PublicationCursor,
    driver: &mut ConnectionDriver,
) -> Result<PublicationStep, DriverError<E>>
where
    B: AsRef<[u8]>,
{
    let index = cursor.next;
    let Some(body) = publication.get(index) else {
        return Ok(PublicationStep::Complete);
    };
    let body = body.as_ref();

    driver.queue_frame::<E>(body)?;

    // `publication.get(index)` succeeding proves `index < publication.len() <= usize::MAX`, so
    // `index` cannot be `usize::MAX` and this increment cannot overflow after egress has committed.
    cursor.next = index + 1;
    Ok(PublicationStep::Queued {
        index,
        body_bytes: body.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};
    use crucible_connection_driver::{ConnectionDriver, DriverError};

    use super::{PublicationCursor, PublicationStep, publish_one};

    fn limits(max_body: usize, egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(max_body, max_body + 5, egress).expect("coherent publication limits")
    }

    fn body(packet_id: u8, payload: &[u8]) -> Vec<u8> {
        let mut body = Vec::with_capacity(payload.len() + 1);
        body.push(packet_id);
        body.extend_from_slice(payload);
        body
    }

    #[test]
    fn cursor_is_one_machine_word() {
        assert_eq!(size_of::<PublicationCursor>(), size_of::<usize>());
        assert_eq!(PublicationCursor::new().next_index(), 0);
    }

    #[test]
    fn empty_publication_is_noop() {
        let publication: [Vec<u8>; 0] = [];
        let mut cursor = PublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(16, 32));

        assert_eq!(
            publish_one::<(), _>(&publication, &mut cursor, &mut driver),
            Ok(PublicationStep::Complete)
        );
        assert!(cursor.is_complete(&publication));
        assert_eq!(cursor.next_index(), 0);
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn success_queues_exactly_one_body_and_advances_once() {
        let publication = [body(1, b"first"), body(2, b"second")];
        let mut cursor = PublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(16, 64));

        assert_eq!(
            publish_one::<(), _>(&publication, &mut cursor, &mut driver),
            Ok(PublicationStep::Queued {
                index: 0,
                body_bytes: 6,
            })
        );
        assert_eq!(cursor.next_index(), 1);
        assert!(!cursor.is_complete(&publication));

        let first_egress = driver.pending_egress().to_vec();
        assert!(!first_egress.is_empty());

        assert_eq!(
            publish_one::<(), _>(&publication, &mut cursor, &mut driver),
            Ok(PublicationStep::Queued {
                index: 1,
                body_bytes: 7,
            })
        );
        assert_eq!(cursor.next_index(), 2);
        assert!(cursor.is_complete(&publication));
        assert!(driver.pending_egress().starts_with(&first_egress));
    }

    #[test]
    fn capacity_rejection_changes_neither_cursor_nor_existing_egress() {
        let publication = [body(1, &[0x11; 15]), body(2, &[0x22; 15])];
        let mut cursor = PublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(16, 17));

        publish_one::<(), _>(&publication, &mut cursor, &mut driver)
            .expect("first exact-fit frame queues");
        let before = driver.pending_egress().to_vec();
        assert_eq!(cursor.next_index(), 1);

        let error = publish_one::<(), _>(&publication, &mut cursor, &mut driver)
            .expect_err("second frame must observe bounded backpressure");
        assert!(matches!(
            error,
            DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })
        ));
        assert_eq!(cursor.next_index(), 1);
        assert_eq!(driver.pending_egress(), before);
    }

    #[test]
    fn wire_rejection_does_not_advance_cursor() {
        let publication = [body(1, &[0xAB; 16])];
        let mut cursor = PublicationCursor::new();
        let mut driver = ConnectionDriver::new(limits(16, 64));

        let error = publish_one::<(), _>(&publication, &mut cursor, &mut driver)
            .expect_err("17-byte body exceeds max packet body");
        assert!(matches!(
            error,
            DriverError::Buffer(ConnectionBufferError::Wire(_))
        ));
        assert_eq!(cursor.next_index(), 0);
        assert_eq!(driver.queued_egress(), 0);
    }
}
