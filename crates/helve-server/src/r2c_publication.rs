//! Source-independent R2C publication admission through the continuing R2B Play owner.
//!
//! This module deliberately knows no Minecraft 26.2 world packet identities or payload law. The
//! target encoder remains responsible for producing complete Play packet bodies. R2C only needs a
//! narrow way to admit those already-formed bodies through the exact bounded driver that survived
//! the R2B `WorldProjection` handoff.

use std::convert::Infallible;

use helve_connection_driver::DriverError;
use helve_publication_core::{PublicationCursor, PublicationStep, publish_one};

use crate::r2b_server::{R2bPlayError, R2bPlaySession};

impl R2bPlaySession {
    /// Atomically admits one target-encoded Play publication batch to the continuing connection.
    ///
    /// Every body must already contain the target-owned packet-ID `VarInt` followed by its payload.
    /// This method performs no packet lookup, world projection, semantic defaulting, or secondary
    /// buffering. It reuses the exact [`helve_connection_driver::ConnectionDriver`] retained across
    /// the R2B `WorldProjection` handoff.
    ///
    /// The complete batch is admitted or rejected as one bounded-egress transaction. A capacity or
    /// frame-law failure leaves the logical egress queue unchanged, so an R2C publication cursor may
    /// advance only after this method succeeds.
    ///
    /// This raw-body seam is intentionally crate-visible only. Target/world composition inside
    /// `helve-server` may use it, but the public [`R2bPlaySession`] API must not become an arbitrary
    /// packet-injection surface.
    ///
    /// # Errors
    ///
    /// Returns the existing fail-closed Play/driver error when the batch violates framing or egress
    /// bounds. Rejection does not partially append the batch.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "R2C projector integration is source-admission blocked; remove when it becomes the production caller"
        )
    )]
    pub(crate) fn admit_play_publication_batch<B>(
        &mut self,
        bodies: &[B],
    ) -> Result<(), R2bPlayError>
    where
        B: AsRef<[u8]>,
    {
        self.driver
            .queue_batch::<Infallible, B>(bodies)
            .map_err(|error| map_publication_driver_error(&error))
    }

    /// Services at most one body from an ordered immutable Play publication.
    ///
    /// The caller owns both the immutable publication and the allocation-free one-word cursor. This
    /// wrapper deliberately reuses [`helve_publication_core::publish_one`] rather than introducing an
    /// R2C-specific cursor or queue. A successful call admits at most one already target-encoded
    /// body through the exact continuing Play driver and advances `cursor` exactly once. Egress
    /// backpressure or frame rejection leaves both the cursor and existing egress unchanged.
    ///
    /// This is the fairness-oriented path for a potentially large world publication. Atomic groups
    /// that genuinely require all-or-nothing egress admission may use
    /// [`Self::admit_play_publication_batch`] instead. Like the atomic raw-body seam, this method is
    /// crate-visible only so external users cannot bypass target-owned packet semantics.
    ///
    /// # Errors
    ///
    /// Returns the existing fail-closed Play/driver error without advancing `cursor` when the next
    /// body violates framing or bounded-egress capacity.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "R2C projector integration is source-admission blocked; remove when it becomes the production caller"
        )
    )]
    pub(crate) fn service_play_publication_one<B>(
        &mut self,
        publication: &[B],
        cursor: &mut PublicationCursor,
    ) -> Result<PublicationStep, R2bPlayError>
    where
        B: AsRef<[u8]>,
    {
        publish_one::<Infallible, B>(publication, cursor, &mut self.driver)
            .map_err(|error| map_publication_driver_error(&error))
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only the intentionally dormant internal R2C publication seams use this mapper"
    )
)]
fn map_publication_driver_error(error: &DriverError<Infallible>) -> R2bPlayError {
    match error {
        DriverError::Buffer(error) => R2bPlayError::Buffer(*error),
        DriverError::Handler(never) => match *never {},
        DriverError::RollbackFailed {
            operation,
            rollback,
        } => R2bPlayError::RollbackFailed {
            operation: *operation,
            rollback: *rollback,
        },
        DriverError::AccountingOverflow => R2bPlayError::AccountingOverflow,
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use helve_connection_core::ConnectionLimits;
    use helve_connection_driver::ConnectionDriver;
    use helve_publication_core::{PublicationCursor, PublicationStep};
    use helve_session_core::LivenessState;
    use helve_target_26_2::r2b::TeleportTransaction;

    use crate::r2b_server::{R2bPlayError, R2bPlaySession};

    fn limits(max_egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(8, 128, max_egress).expect("valid test connection limits")
    }

    fn session(max_egress: usize) -> R2bPlaySession {
        R2bPlaySession {
            driver: ConnectionDriver::new(limits(max_egress)),
            read_scratch: vec![0_u8; 32].into_boxed_slice(),
            teleport: TeleportTransaction::new(),
            liveness: LivenessState::new(0, 0).expect("zero is a valid liveness origin"),
        }
    }

    fn independently_framed(max_egress: usize, bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut driver = ConnectionDriver::new(limits(max_egress));
        driver
            .queue_batch::<Infallible, _>(bodies)
            .expect("independent framing fits");
        driver.pending_egress().to_vec()
    }

    #[test]
    fn publication_batch_reuses_continuing_driver_and_preserves_body_order() {
        let bodies = vec![vec![0x11, 0xAA], vec![0x22, 0xBB, 0xCC], vec![0x33]];
        let expected = independently_framed(128, &bodies);
        let mut session = session(128);

        assert_eq!(session.queued_egress(), 0);
        session
            .admit_play_publication_batch(&bodies)
            .expect("bounded publication batch");

        assert_eq!(session.pending_play_egress(), expected);
        assert_eq!(session.queued_egress(), expected.len());
    }

    #[test]
    fn publication_batch_backpressure_is_atomic_against_existing_egress() {
        let mut session = session(10);
        let prefix = [vec![0x01, 0xAA]];
        session
            .admit_play_publication_batch(&prefix)
            .expect("prefix fits");
        let before = session.pending_play_egress().to_vec();

        let rejected = [vec![0x10, 1, 2], vec![0x11, 3, 4]];
        let error = session
            .admit_play_publication_batch(&rejected)
            .expect_err("complete batch exceeds remaining bounded egress");

        assert!(matches!(error, R2bPlayError::Buffer(_)));
        assert_eq!(session.pending_play_egress(), before);
        assert_eq!(session.queued_egress(), before.len());
    }

    #[test]
    fn publication_cursor_queues_at_most_one_body_per_service_opportunity() {
        let publication = [vec![0x10, 1, 2], vec![0x11, 3, 4]];
        let mut cursor = PublicationCursor::new();
        let mut session = session(128);

        assert_eq!(
            session.service_play_publication_one(&publication, &mut cursor),
            Ok(PublicationStep::Queued {
                index: 0,
                body_bytes: 3,
            })
        );
        assert_eq!(cursor.next_index(), 1);
        let first_egress = session.pending_play_egress().to_vec();

        assert_eq!(
            session.service_play_publication_one(&publication, &mut cursor),
            Ok(PublicationStep::Queued {
                index: 1,
                body_bytes: 3,
            })
        );
        assert_eq!(cursor.next_index(), 2);
        assert!(cursor.is_complete(&publication));
        assert!(session.pending_play_egress().starts_with(&first_egress));

        assert_eq!(
            session.service_play_publication_one(&publication, &mut cursor),
            Ok(PublicationStep::Complete)
        );
        assert_eq!(cursor.next_index(), 2);
    }

    #[test]
    fn publication_cursor_backpressure_does_not_advance_progress() {
        let publication = [vec![0x10, 1, 2, 3, 4], vec![0x11, 5, 6, 7, 8]];
        let mut cursor = PublicationCursor::new();
        let mut session = session(10);

        session
            .service_play_publication_one(&publication, &mut cursor)
            .expect("first framed body fits");
        assert_eq!(cursor.next_index(), 1);
        let before = session.pending_play_egress().to_vec();

        let error = session
            .service_play_publication_one(&publication, &mut cursor)
            .expect_err("second framed body observes bounded backpressure");
        assert!(matches!(error, R2bPlayError::Buffer(_)));
        assert_eq!(cursor.next_index(), 1);
        assert_eq!(session.pending_play_egress(), before);
    }
}
