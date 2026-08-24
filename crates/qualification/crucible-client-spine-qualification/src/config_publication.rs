//! Qualification-only model for bounded immutable Configuration publication.
//!
//! This module intentionally contains no Minecraft packet identity. It exercises the architectural
//! mechanism proposed by `CONFIGURATION_PUBLICATION_LAB.md` against Crucible's real bounded
//! `ConnectionDriver` egress path.

#![forbid(unsafe_code)]

use crucible_connection_driver::{ConnectionDriver, DriverError};

/// One immutable ordered set of already-formed packet bodies.
///
/// Each body includes its synthetic/target packet-ID bytes and payload, but not the outer frame
/// length. The production `ConnectionDriver` remains responsible for exact framing and egress
/// bounds. The image is built once and can be borrowed by arbitrarily many independent cursors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationImage {
    bodies: Box<[Box<[u8]>]>,
    body_bytes: usize,
}

impl PublicationImage {
    /// Freezes owned packet bodies into one immutable publication image.
    ///
    /// # Errors
    ///
    /// Returns an error if aggregate body-byte accounting overflows `usize`.
    pub fn from_bodies(bodies: Vec<Vec<u8>>) -> Result<Self, PublicationImageError> {
        let mut body_bytes = 0usize;
        let mut frozen = Vec::with_capacity(bodies.len());
        for body in bodies {
            body_bytes = body_bytes
                .checked_add(body.len())
                .ok_or(PublicationImageError::BodyBytesOverflow)?;
            frozen.push(body.into_boxed_slice());
        }
        Ok(Self {
            bodies: frozen.into_boxed_slice(),
            body_bytes,
        })
    }

    /// Number of ordered packet bodies in this image.
    #[must_use]
    pub const fn frame_count(&self) -> usize {
        self.bodies.len()
    }

    /// Aggregate packet-body bytes retained by the image, excluding outer frame-length prefixes.
    #[must_use]
    pub const fn body_bytes(&self) -> usize {
        self.body_bytes
    }

    /// Returns one immutable packet body by publication index.
    #[must_use]
    pub fn body(&self, index: usize) -> Option<&[u8]> {
        self.bodies.get(index).map(AsRef::as_ref)
    }

    /// Creates an independent cursor positioned before the first body.
    #[must_use]
    pub const fn cursor(&self) -> PublicationCursor {
        PublicationCursor::new()
    }
}

/// Construction failure for an immutable publication image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationImageError {
    /// Aggregate body-byte accounting overflowed.
    BodyBytesOverflow,
}

/// Tiny per-connection progress state over one immutable publication image.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PublicationCursor {
    next: usize,
}

impl PublicationCursor {
    /// Cursor before the first publication body.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// Index of the next body which has not yet been admitted to bounded egress.
    #[must_use]
    pub const fn next_index(self) -> usize {
        self.next
    }

    /// Whether every body has already been admitted to bounded egress.
    #[must_use]
    pub const fn is_complete(self, image: &PublicationImage) -> bool {
        self.next >= image.frame_count()
    }
}

/// Evidence from one bounded publication attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationStep {
    /// The cursor was already complete; no egress or state changed.
    Complete,
    /// Exactly one publication body was admitted and the cursor advanced exactly once.
    Queued {
        /// Body index admitted by this step.
        index: usize,
        /// Packet-body bytes admitted, excluding the outer frame-length prefix.
        body_bytes: usize,
    },
}

/// Admits at most one immutable publication body through the real bounded driver egress path.
///
/// The cursor advances only after `ConnectionDriver::queue_frame` succeeds. Therefore frame/body
/// validation or egress-capacity rejection leaves both semantic publication position and existing
/// queued egress unchanged according to the driver's already-qualified fail-closed contract.
///
/// # Errors
///
/// Returns the real driver/buffer failure for the next body without advancing `cursor`.
pub fn publish_one<E>(
    image: &PublicationImage,
    cursor: &mut PublicationCursor,
    driver: &mut ConnectionDriver,
) -> Result<PublicationStep, DriverError<E>> {
    let index = cursor.next;
    let Some(body) = image.body(index) else {
        return Ok(PublicationStep::Complete);
    };

    driver.queue_frame::<E>(body)?;
    cursor.next = cursor
        .next
        .checked_add(1)
        .ok_or(DriverError::AccountingOverflow)?;
    Ok(PublicationStep::Queued {
        index,
        body_bytes: body.len(),
    })
}

/// Queues the complete image in order through repeated bounded publication steps.
///
/// This helper is qualification-only and is useful when the configured egress window is known to
/// fit the complete image. It does not weaken the one-step commit law used by constrained-window
/// tests and proposed production pumping.
///
/// # Errors
///
/// Returns the first real driver rejection; the cursor then denotes exactly the bodies which were
/// successfully admitted before that rejection.
pub fn publish_until_blocked<E>(
    image: &PublicationImage,
    cursor: &mut PublicationCursor,
    driver: &mut ConnectionDriver,
) -> Result<usize, DriverError<E>> {
    let mut queued = 0usize;
    while !cursor.is_complete(image) {
        match publish_one::<E>(image, cursor, driver)? {
            PublicationStep::Complete => break,
            PublicationStep::Queued { .. } => {
                queued = queued
                    .checked_add(1)
                    .ok_or(DriverError::AccountingOverflow)?;
            }
        }
    }
    Ok(queued)
}
