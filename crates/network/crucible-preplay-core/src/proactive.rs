//! Target-neutral proactive bounded publication above the pre-play connection driver.
//!
//! Inbound-triggered responses remain on the binder's existing all-or-nothing transaction path.
//! This module covers the different case where a semantic decision has already committed and the
//! target must subsequently publish immutable packet bodies over multiple bounded service
//! opportunities. The target may propose work but never receives the connection driver.

use crucible_publication_core::publish_one as publish_one_body;
pub use crucible_publication_core::{PublicationCursor, PublicationStep};
use crucible_session_core::SessionState;

use super::{
    PrePlayConnection, PrePlayError, PrePlayTarget, TargetBoundaryError, map_driver_error,
};

/// One borrowed immutable publication proposal plus the target-owned state needed to commit it.
///
/// `bodies` are borrowed for this service opportunity only. The binder does not prescribe whether
/// their owner is a static generated array, an immutable composition image, or another qualified
/// representation. `commit` is deliberately `Copy`, preventing this token from becoming a second
/// owner for publication bytes or another hidden queue.
pub struct PrePlayPublication<'a, B, C>
where
    C: Copy,
{
    bodies: &'a [B],
    cursor: PublicationCursor,
    commit: C,
}

impl<'a, B, C> PrePlayPublication<'a, B, C>
where
    C: Copy,
{
    /// Creates one proposal from borrowed immutable bodies and copied target-local progression.
    #[must_use]
    pub fn new(bodies: &'a [B], cursor: PublicationCursor, commit: C) -> Self {
        Self {
            bodies,
            cursor,
            commit,
        }
    }
}

/// Optional proactive-publication capability for a statically bound pre-play target.
///
/// This is intentionally separate from [`PrePlayTarget`]. Status/Login targets that only react to
/// inbound packets do not implement it and therefore acquire no extra state or runtime branch.
/// Implementations must treat [`Self::publication`] as a proposal: target-local state is immutable
/// until the binder has successfully admitted the proposed publication step.
pub trait PrePlayPublisher: PrePlayTarget {
    /// Element type in the immutable publication image.
    type PublicationBody: AsRef<[u8]>;
    /// Small owner-local token adopted only after successful bounded admission.
    type PublicationCommit: Copy;

    /// Proposes the currently active publication, or `None` when no proactive work is ready.
    ///
    /// The returned cursor is only a copy. Returning a proposal never advances live target state.
    /// All validation or fallible target work belongs here, before the binder touches egress.
    ///
    /// # Errors
    ///
    /// Returns a target-specific error without changing egress, session state, or target state.
    fn publication<'a>(
        context: &'a Self::Context,
        session: SessionState,
        target_state: &'a Self::State,
    ) -> Result<
        Option<PrePlayPublication<'a, Self::PublicationBody, Self::PublicationCommit>>,
        Self::Error,
    >;

    /// Adopts progression after the publication primitive completed successfully.
    ///
    /// This hook must be infallible and owner-local. For [`PublicationStep::Queued`] the supplied
    /// cursor has advanced exactly once after the frame entered bounded egress. For
    /// [`PublicationStep::Complete`] no egress changed, but committing the token allows a target to
    /// leave its publication stage without inventing a synthetic packet or a second state machine.
    fn commit_publication(
        state: &mut Self::State,
        commit: Self::PublicationCommit,
        cursor: PublicationCursor,
        step: PublicationStep,
    );
}

/// Result of one proactive pre-play publication service opportunity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrePlayPublicationProcess {
    /// The target has no proactive work ready in the current state.
    Idle,
    /// The target proposed work and the qualified publication primitive completed successfully.
    Progress(PublicationStep),
}

impl<T> PrePlayConnection<T>
where
    T: PrePlayPublisher,
{
    /// Services at most one proactive publication body through the existing bounded egress.
    ///
    /// The target first observes immutable context/session/state and proposes borrowed publication
    /// bodies plus copied progression. The binder then delegates the sole queue operation to
    /// `crucible-publication-core`. Only after that operation succeeds does the target receive its
    /// infallible commit hook. No inbound bytes are consumed and the generic session state is never
    /// changed by this method.
    ///
    /// # Errors
    ///
    /// Returns fail-closed on a closed session, target proposal rejection, malformed/oversized body,
    /// egress backpressure, or driver accounting failure. No error path commits target-local
    /// publication state.
    pub fn service_publication(
        &mut self,
        context: &T::Context,
    ) -> Result<PrePlayPublicationProcess, PrePlayError<T::Error>> {
        if self.session.phase() == crucible_session_core::SessionPhase::Closed {
            return Err(PrePlayError::ClosedSession);
        }

        let proposal = T::publication(context, self.session, &self.target_state)
            .map_err(PrePlayError::Target)?;
        let Some(PrePlayPublication {
            bodies,
            mut cursor,
            commit,
        }) = proposal
        else {
            return Ok(PrePlayPublicationProcess::Idle);
        };

        let step = publish_one_body::<TargetBoundaryError<T::Error>, _>(
            bodies,
            &mut cursor,
            &mut self.driver,
        )
        .map_err(map_driver_error)?;

        T::commit_publication(&mut self.target_state, commit, cursor, step);
        Ok(PrePlayPublicationProcess::Progress(step))
    }
}

#[cfg(test)]
mod tests {
    use crucible_connection_core::{ConnectionBufferError, ConnectionLimits, FrameView};
    use crucible_connection_driver::{ConnectionDriver, OutboundBatch};
    use crucible_session_core::{SessionPhase, SessionState};

    use super::{
        PrePlayPublication, PrePlayPublicationProcess, PrePlayPublisher, PublicationCursor,
        PublicationStep,
    };
    use crate::{PrePlayAction, PrePlayConnection, PrePlayError, PrePlayTarget};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PublishingError {
        InboundUnsupported,
    }

    #[derive(Debug)]
    struct UnsupportedAction {
        candidate: SessionState,
        frames: Vec<Vec<u8>>,
    }

    impl OutboundBatch for UnsupportedAction {
        type Body = Vec<u8>;

        fn outbound_frames(&self) -> &[Self::Body] {
            &self.frames
        }
    }

    impl PrePlayAction for UnsupportedAction {
        fn candidate_session(&self) -> SessionState {
            self.candidate
        }
    }

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    struct PublishingState {
        cursor: PublicationCursor,
        commits: usize,
        done: bool,
    }

    #[derive(Debug)]
    struct PublishingContext {
        bodies: Vec<Vec<u8>>,
    }

    struct PublishingTarget;

    impl PrePlayTarget for PublishingTarget {
        type Error = PublishingError;
        type Context = PublishingContext;
        type State = PublishingState;
        type Action = UnsupportedAction;

        fn decode(
            _context: &Self::Context,
            _session: SessionState,
            _target_state: &Self::State,
            _frame: FrameView<'_>,
        ) -> Result<Self::Action, Self::Error> {
            Err(PublishingError::InboundUnsupported)
        }
    }

    impl PrePlayPublisher for PublishingTarget {
        type PublicationBody = Vec<u8>;
        type PublicationCommit = usize;

        fn publication<'a>(
            context: &'a Self::Context,
            session: SessionState,
            target_state: &'a Self::State,
        ) -> Result<
            Option<PrePlayPublication<'a, Self::PublicationBody, Self::PublicationCommit>>,
            Self::Error,
        > {
            if session.phase() != SessionPhase::Configuration || target_state.done {
                return Ok(None);
            }
            Ok(Some(PrePlayPublication::new(
                &context.bodies,
                target_state.cursor,
                target_state.commits + 1,
            )))
        }

        fn commit_publication(
            state: &mut Self::State,
            commit: Self::PublicationCommit,
            cursor: PublicationCursor,
            step: PublicationStep,
        ) {
            state.cursor = cursor;
            state.commits = commit;
            if step == PublicationStep::Complete {
                state.done = true;
            }
        }
    }

    fn limits(max_body: usize, egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(max_body, 256, egress).expect("coherent proactive test limits")
    }

    fn configuration_connection(limits: ConnectionLimits) -> PrePlayConnection<PublishingTarget> {
        let mut connection = PrePlayConnection::<PublishingTarget>::new(limits);
        connection
            .session
            .advance(SessionPhase::Login)
            .expect("handshake to login");
        connection
            .session
            .advance(SessionPhase::Configuration)
            .expect("login to configuration");
        connection
    }

    #[test]
    fn publisher_is_idle_outside_its_ready_phase() {
        let context = PublishingContext {
            bodies: vec![vec![0x01, 0x02]],
        };
        let mut connection = PrePlayConnection::<PublishingTarget>::new(limits(16, 64));

        assert_eq!(
            connection.service_publication(&context),
            Ok(PrePlayPublicationProcess::Idle)
        );
        assert_eq!(*connection.target_state(), PublishingState::default());
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn successful_step_queues_exactly_one_body_before_target_commit() {
        let context = PublishingContext {
            bodies: vec![vec![0x11, 0x22], vec![0x33]],
        };
        let test_limits = limits(16, 64);
        let mut connection = configuration_connection(test_limits);

        assert_eq!(
            connection.service_publication(&context),
            Ok(PrePlayPublicationProcess::Progress(
                PublicationStep::Queued {
                    index: 0,
                    body_bytes: 2,
                }
            ))
        );
        assert_eq!(connection.target_state().cursor.next_index(), 1);
        assert_eq!(connection.target_state().commits, 1);
        assert!(!connection.target_state().done);

        let mut expected = ConnectionDriver::new(test_limits);
        expected
            .queue_frame::<()>(&context.bodies[0])
            .expect("expected body fits");
        assert_eq!(connection.pending_egress(), expected.pending_egress());
    }

    #[test]
    fn egress_backpressure_commits_neither_cursor_nor_token() {
        let body = vec![0x5A; 16];
        let context = PublishingContext {
            bodies: vec![body.clone(), body],
        };
        let mut connection = configuration_connection(limits(16, 17));

        assert!(matches!(
            connection.service_publication(&context),
            Ok(PrePlayPublicationProcess::Progress(
                PublicationStep::Queued { index: 0, .. }
            ))
        ));
        let state_before = *connection.target_state();
        let egress_before = connection.pending_egress().to_vec();

        assert!(matches!(
            connection.service_publication(&context),
            Err(PrePlayError::Buffer(
                ConnectionBufferError::EgressLimitExceeded { .. }
            ))
        ));
        assert_eq!(*connection.target_state(), state_before);
        assert_eq!(connection.pending_egress(), egress_before);
    }

    #[test]
    fn wire_rejection_leaves_target_and_egress_unchanged() {
        let context = PublishingContext {
            bodies: vec![vec![0xA5; 17]],
        };
        let mut connection = configuration_connection(limits(16, 64));

        assert!(matches!(
            connection.service_publication(&context),
            Err(PrePlayError::Buffer(ConnectionBufferError::Wire(_)))
        ));
        assert_eq!(*connection.target_state(), PublishingState::default());
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn complete_step_commits_stage_exit_without_queuing_a_frame() {
        let context = PublishingContext {
            bodies: vec![vec![0x01], vec![0x02, 0x03]],
        };
        let mut connection = configuration_connection(limits(16, 64));

        for expected_index in 0..context.bodies.len() {
            assert!(matches!(
                connection.service_publication(&context),
                Ok(PrePlayPublicationProcess::Progress(PublicationStep::Queued {
                    index,
                    ..
                })) if index == expected_index
            ));
            let queued = connection.queued_egress();
            connection
                .consume_written(queued)
                .expect("drain publication frame");
        }

        assert_eq!(
            connection.service_publication(&context),
            Ok(PrePlayPublicationProcess::Progress(
                PublicationStep::Complete
            ))
        );
        assert_eq!(connection.target_state().cursor.next_index(), 2);
        assert_eq!(connection.target_state().commits, 3);
        assert!(connection.target_state().done);
        assert_eq!(connection.queued_egress(), 0);

        assert_eq!(
            connection.service_publication(&context),
            Ok(PrePlayPublicationProcess::Idle)
        );
        assert_eq!(connection.target_state().commits, 3);
    }

    #[test]
    fn closed_session_never_services_proactive_output() {
        let context = PublishingContext {
            bodies: vec![vec![0x01]],
        };
        let mut connection = configuration_connection(limits(16, 64));
        assert!(connection.session.close());

        assert_eq!(
            connection.service_publication(&context),
            Err(PrePlayError::ClosedSession)
        );
        assert_eq!(*connection.target_state(), PublishingState::default());
        assert_eq!(connection.queued_egress(), 0);
    }
}
