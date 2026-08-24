//! Qualification-only laboratory for bounded immutable outbound publication.
//!
//! This module contains no Minecraft packet identities. It tests the mechanism shape proposed for
//! large Configuration publications: one immutable body image plus one tiny per-connection cursor.
//! Production networking remains unchanged until this candidate passes semantic and performance
//! qualification.

use std::num::NonZeroUsize;

use crucible_connection_core::ConnectionBufferError;
use crucible_connection_driver::{ConnectionDriver, DriverError};

/// One immutable ordered publication image of already-formed packet bodies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationImage {
    bodies: Vec<Box<[u8]>>,
    total_body_bytes: usize,
}

impl PublicationImage {
    /// Owns one ordered publication image.
    #[must_use]
    pub fn from_bodies(bodies: Vec<Vec<u8>>) -> Self {
        let total_body_bytes = bodies.iter().map(Vec::len).sum();
        Self {
            bodies: bodies.into_iter().map(Vec::into_boxed_slice).collect(),
            total_body_bytes,
        }
    }

    /// Number of packet bodies in publication order.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether this image contains no packet bodies.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// Total unframed packet-body bytes owned once by this image.
    #[must_use]
    pub const fn total_body_bytes(&self) -> usize {
        self.total_body_bytes
    }

    /// Returns one immutable packet body by publication index.
    #[must_use]
    pub fn body(&self, index: usize) -> Option<&[u8]> {
        self.bodies.get(index).map(AsRef::as_ref)
    }

    /// Qualification reference that deliberately duplicates every publication body.
    #[must_use]
    pub fn rebuild_owned(&self) -> Self {
        Self::from_bodies(self.bodies.iter().map(|body| body.to_vec()).collect())
    }
}

/// Positive maximum packet bodies one publication pump may attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationBudget(NonZeroUsize);

impl PublicationBudget {
    /// Constructs a positive publication frame budget.
    #[must_use]
    pub const fn new(frames: usize) -> Option<Self> {
        match NonZeroUsize::new(frames) {
            Some(frames) => Some(Self(frames)),
            None => None,
        }
    }

    /// Returns the maximum attempts for one pump call.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// Per-connection progress through one immutable publication image.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PublicationCursor {
    next_frame: usize,
}

impl PublicationCursor {
    /// Creates a cursor before the first publication body.
    #[must_use]
    pub const fn new() -> Self {
        Self { next_frame: 0 }
    }

    /// Index of the next body that has not yet been admitted to bounded egress.
    #[must_use]
    pub const fn next_frame(self) -> usize {
        self.next_frame
    }

    /// Whether every body in `image` has been admitted.
    #[must_use]
    pub fn is_complete(self, image: &PublicationImage) -> bool {
        self.next_frame == image.len()
    }
}

/// Why one successful bounded publication pump returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationStop {
    /// Every image body has been admitted to this connection.
    Complete,
    /// The explicit frame budget was exhausted before publication completed.
    FrameBudgetExhausted,
    /// The next valid frame cannot currently fit in bounded egress.
    EgressBlocked,
}

/// Evidence from one bounded publication pump.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PublicationReport {
    /// Packet bodies admitted during this call.
    pub admitted_frames: usize,
    /// Unframed body bytes admitted during this call.
    pub admitted_body_bytes: usize,
    /// Boundary that stopped progress.
    pub stop: PublicationStop,
}

/// Attempts bounded publication progress without consuming any inbound frame.
///
/// The cursor advances only after `ConnectionDriver::queue_frame` succeeds. A temporary egress
/// capacity failure therefore returns `EgressBlocked` with the exact next frame retained. Any other
/// framing/body error is fail-closed and leaves the failing frame uncommitted.
///
/// # Errors
///
/// Returns a non-capacity driver error when an image body violates the configured wire/frame law.
pub fn pump_publication(
    driver: &mut ConnectionDriver,
    image: &PublicationImage,
    cursor: &mut PublicationCursor,
    budget: PublicationBudget,
) -> Result<PublicationReport, DriverError<()>> {
    if cursor.is_complete(image) {
        return Ok(PublicationReport {
            admitted_frames: 0,
            admitted_body_bytes: 0,
            stop: PublicationStop::Complete,
        });
    }

    let mut admitted_frames = 0usize;
    let mut admitted_body_bytes = 0usize;

    while admitted_frames < budget.get() {
        let Some(body) = image.body(cursor.next_frame) else {
            return Ok(PublicationReport {
                admitted_frames,
                admitted_body_bytes,
                stop: PublicationStop::Complete,
            });
        };

        match driver.queue_frame::<()>(body) {
            Ok(()) => {
                cursor.next_frame += 1;
                admitted_frames += 1;
                admitted_body_bytes = admitted_body_bytes
                    .checked_add(body.len())
                    .ok_or(DriverError::AccountingOverflow)?;
            }
            Err(DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })) => {
                return Ok(PublicationReport {
                    admitted_frames,
                    admitted_body_bytes,
                    stop: PublicationStop::EgressBlocked,
                });
            }
            Err(error) => return Err(error),
        }

        if cursor.is_complete(image) {
            return Ok(PublicationReport {
                admitted_frames,
                admitted_body_bytes,
                stop: PublicationStop::Complete,
            });
        }
    }

    Ok(PublicationReport {
        admitted_frames,
        admitted_body_bytes,
        stop: PublicationStop::FrameBudgetExhausted,
    })
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use crucible_connection_core::ConnectionLimits;
    use crucible_connection_driver::{ConnectionDriver, DriverError};

    use super::{
        PublicationBudget, PublicationCursor, PublicationImage, PublicationStop, pump_publication,
    };

    fn budget(frames: usize) -> PublicationBudget {
        PublicationBudget::new(frames).expect("positive test budget")
    }

    fn body(id: u8, payload_len: usize) -> Vec<u8> {
        let mut body = vec![id];
        body.extend((0..payload_len).map(|offset| id.wrapping_add(offset as u8)));
        body
    }

    fn image() -> PublicationImage {
        PublicationImage::from_bodies(vec![body(1, 5), body(2, 7), body(3, 9), body(4, 11)])
    }

    fn limits(frame_body: usize, egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(frame_body, frame_body + 8, egress).expect("coherent test limits")
    }

    fn reference_stream(image: &PublicationImage, limits: ConnectionLimits) -> Vec<u8> {
        let mut driver = ConnectionDriver::new(limits);
        for index in 0..image.len() {
            driver
                .queue_frame::<()>(image.body(index).expect("body exists"))
                .expect("reference body fits");
        }
        driver.pending_egress().to_vec()
    }

    #[test]
    fn per_connection_cursor_is_exactly_one_usize() {
        assert_eq!(size_of::<PublicationCursor>(), size_of::<usize>());
    }

    #[test]
    fn complete_publication_matches_repeated_reference_queueing_byte_for_byte() {
        let image = image();
        let limits = limits(32, 256);
        let expected = reference_stream(&image, limits);
        let mut driver = ConnectionDriver::new(limits);
        let mut cursor = PublicationCursor::new();

        let report = pump_publication(&mut driver, &image, &mut cursor, budget(16))
            .expect("valid image publishes");
        assert_eq!(report.stop, PublicationStop::Complete);
        assert_eq!(report.admitted_frames, image.len());
        assert_eq!(report.admitted_body_bytes, image.total_body_bytes());
        assert!(cursor.is_complete(&image));
        assert_eq!(driver.pending_egress(), expected);
    }

    #[test]
    fn explicit_budget_yields_at_exact_next_frame() {
        let image = image();
        let mut driver = ConnectionDriver::new(limits(32, 256));
        let mut cursor = PublicationCursor::new();

        let first = pump_publication(&mut driver, &image, &mut cursor, budget(2))
            .expect("first pump succeeds");
        assert_eq!(first.stop, PublicationStop::FrameBudgetExhausted);
        assert_eq!(first.admitted_frames, 2);
        assert_eq!(cursor.next_frame(), 2);

        let second = pump_publication(&mut driver, &image, &mut cursor, budget(2))
            .expect("second pump succeeds");
        assert_eq!(second.stop, PublicationStop::Complete);
        assert_eq!(cursor.next_frame(), 4);
    }

    #[test]
    fn egress_backpressure_never_advances_the_blocked_frame() {
        let image = PublicationImage::from_bodies(vec![body(1, 14), body(2, 14), body(3, 14)]);
        let limits = limits(16, 34);
        let mut driver = ConnectionDriver::new(limits);
        let mut cursor = PublicationCursor::new();

        let first = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect("capacity stop is nonfatal");
        assert_eq!(first.stop, PublicationStop::EgressBlocked);
        assert_eq!(first.admitted_frames, 2);
        assert_eq!(cursor.next_frame(), 2);
        let queued_before = driver.pending_egress().to_vec();

        let blocked_again = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect("repeated capacity stop is nonfatal");
        assert_eq!(blocked_again.stop, PublicationStop::EgressBlocked);
        assert_eq!(blocked_again.admitted_frames, 0);
        assert_eq!(cursor.next_frame(), 2);
        assert_eq!(driver.pending_egress(), queued_before);

        let drain = driver.queued_egress();
        driver.consume_written::<()>(drain).expect("drain queued prefix");
        let resumed = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect("resume after drain");
        assert_eq!(resumed.stop, PublicationStop::Complete);
        assert_eq!(resumed.admitted_frames, 1);
        assert!(cursor.is_complete(&image));
    }

    #[test]
    fn partial_drain_resumes_without_requeueing_committed_frames() {
        let image = PublicationImage::from_bodies(vec![body(1, 14), body(2, 14), body(3, 14)]);
        let limits = limits(16, 34);
        let mut driver = ConnectionDriver::new(limits);
        let mut cursor = PublicationCursor::new();
        let _ = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect("initial bounded publication");
        assert_eq!(cursor.next_frame(), 2);

        let first_encoded_frame_bytes = 16;
        driver
            .consume_written::<()>(first_encoded_frame_bytes)
            .expect("partial drain of one exact frame");
        let resumed = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect("third frame now fits");
        assert_eq!(resumed.stop, PublicationStop::Complete);
        assert_eq!(resumed.admitted_frames, 1);
        assert_eq!(cursor.next_frame(), 3);
    }

    #[test]
    fn invalid_later_body_preserves_cursor_and_existing_egress() {
        let image = PublicationImage::from_bodies(vec![body(1, 4), body(2, 40)]);
        let mut driver = ConnectionDriver::new(limits(16, 128));
        let mut cursor = PublicationCursor::new();

        let error = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect_err("oversized second body must fail closed");
        assert!(matches!(error, DriverError::Buffer(_)));
        assert_eq!(cursor.next_frame(), 1);

        let reference = PublicationImage::from_bodies(vec![body(1, 4)]);
        assert_eq!(
            driver.pending_egress(),
            reference_stream(&reference, limits(16, 128))
        );
    }

    #[test]
    fn two_connections_share_bytes_but_have_independent_progress() {
        let image = image();
        let first_ptr = image.body(0).expect("body exists").as_ptr();
        let limits = limits(32, 256);
        let mut first_driver = ConnectionDriver::new(limits);
        let mut second_driver = ConnectionDriver::new(limits);
        let mut first = PublicationCursor::new();
        let mut second = PublicationCursor::new();

        pump_publication(&mut first_driver, &image, &mut first, budget(1))
            .expect("first connection progresses");
        assert_eq!(first.next_frame(), 1);
        assert_eq!(second.next_frame(), 0);
        assert_eq!(image.body(0).expect("same shared body").as_ptr(), first_ptr);

        pump_publication(&mut second_driver, &image, &mut second, budget(8))
            .expect("second connection progresses independently");
        assert!(second.is_complete(&image));
        assert_eq!(first.next_frame(), 1);
    }

    #[test]
    fn completed_cursor_is_idempotent_and_queues_nothing() {
        let image = image();
        let mut driver = ConnectionDriver::new(limits(32, 256));
        let mut cursor = PublicationCursor::new();
        pump_publication(&mut driver, &image, &mut cursor, budget(8)).expect("complete image");
        let queued = driver.pending_egress().to_vec();

        let again = pump_publication(&mut driver, &image, &mut cursor, budget(8))
            .expect("completed pump is a no-op");
        assert_eq!(again.stop, PublicationStop::Complete);
        assert_eq!(again.admitted_frames, 0);
        assert_eq!(again.admitted_body_bytes, 0);
        assert_eq!(driver.pending_egress(), queued);
    }
}
