//! Qualification model for immutable Configuration publication.
//!
//! This module intentionally contains no Minecraft packet identity. It supplies the lab's immutable
//! shared-image representation while exercising the production `helve-publication-core` cursor
//! directly against Crucible's real bounded `ConnectionDriver` egress path.

#![forbid(unsafe_code)]

use helve_connection_driver::{ConnectionDriver, DriverError};
pub(crate) use helve_publication_core::PublicationStep;

/// One immutable ordered set of already-formed packet bodies used by the qualification laboratory.
///
/// Each body includes its synthetic/target packet-ID bytes and payload, but not the outer frame
/// length. The production `ConnectionDriver` remains responsible for exact framing and egress
/// bounds. This image representation is deliberately lab-local: production callers remain free to
/// use generated static slices or another immutable/shared owner without changing the cursor core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicationImage {
    bodies: Box<[Box<[u8]>]>,
    body_bytes: usize,
}

impl PublicationImage {
    /// Freezes owned packet bodies into one immutable publication image.
    ///
    /// # Errors
    ///
    /// Returns an error if aggregate body-byte accounting overflows `usize`.
    pub(crate) fn from_bodies(bodies: Vec<Vec<u8>>) -> Result<Self, PublicationImageError> {
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
    pub(crate) const fn frame_count(&self) -> usize {
        self.bodies.len()
    }

    /// Aggregate packet-body bytes retained by the image, excluding outer frame-length prefixes.
    #[must_use]
    pub(crate) const fn body_bytes(&self) -> usize {
        self.body_bytes
    }

    /// Immutable ordered body view shared by the candidate and independent reference paths.
    #[must_use]
    pub(crate) fn bodies(&self) -> &[Box<[u8]>] {
        self.bodies.as_ref()
    }
}

/// Construction failure for an immutable publication image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublicationImageError {
    /// Aggregate body-byte accounting overflowed.
    BodyBytesOverflow,
}

/// Lab-facing transparent wrapper over the production one-word cursor.
///
/// The wrapper exists only so the original image-oriented qualification API remains stable. All
/// progression logic is delegated to `helve-publication-core`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PublicationCursor(helve_publication_core::PublicationCursor);

impl PublicationCursor {
    /// Cursor before the first publication body.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self(helve_publication_core::PublicationCursor::new())
    }

    /// Index of the next body which has not yet been admitted to bounded egress.
    #[must_use]
    pub(crate) const fn next_index(self) -> usize {
        self.0.next_index()
    }

    /// Whether every body has already been admitted to bounded egress.
    #[must_use]
    pub(crate) fn is_complete(self, image: &PublicationImage) -> bool {
        self.0.is_complete(image.bodies())
    }
}

/// Admits at most one immutable publication body through the production bounded cursor primitive.
///
/// This wrapper keeps the existing lab/benchmark call shape stable while ensuring every permanent
/// Configuration publication invariant is now exercised against production code rather than a
/// qualification-only progression implementation.
///
/// # Errors
///
/// Returns the real driver/buffer failure for the next body without advancing `cursor`.
pub(crate) fn publish_one<E>(
    image: &PublicationImage,
    cursor: &mut PublicationCursor,
    driver: &mut ConnectionDriver,
) -> Result<PublicationStep, DriverError<E>> {
    helve_publication_core::publish_one(image.bodies(), &mut cursor.0, driver)
}
