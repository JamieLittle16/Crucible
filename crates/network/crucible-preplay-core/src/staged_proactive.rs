//! Target-neutral staged proactive publication through the private pre-play connection driver.
//!
//! Targets may assemble an allocation-free plan view from immutable runtime context and target-local
//! state, but they never receive the connection driver. The binder remains the sole owner of bounded
//! egress admission and commits target-local progression only after one body or one explicit stage
//! boundary succeeds.

use crucible_publication_core::{
    StagedPublicationCursor, StagedPublicationPlan, StagedPublicationStep, publish_staged_plan_one,
};
use crucible_session_core::SessionState;

use super::{PrePlayConnection, PrePlayError, PrePlayTarget, TargetBoundaryError, map_driver_error};

/// One owned zero-allocation plan view plus copied progression proposed for a service opportunity.
///
/// `P` is normally a tiny value containing references into immutable context and target-local state;
/// it must not own a second queue or duplicate packet bodies. The binder consumes this proposal in
/// the same call and drops the plan view before target-local state is mutated.
#[derive(Debug)]
pub struct PrePlayStagedPublication<P, C> {
    plan: P,
    cursor: StagedPublicationCursor,
    commit: C,
}

impl<P, C> PrePlayStagedPublication<P, C>
where
    C: Copy,
{
    /// Creates one staged proposal from an immutable plan view and copied owner-local progression.
    #[must_use]
    pub const fn new(plan: P, cursor: StagedPublicationCursor, commit: C) -> Self {
        Self {
            plan,
            cursor,
            commit,
        }
    }
}

/// Optional staged proactive-publication capability for a statically bound pre-play target.
///
/// The generic associated plan type allows a target to return a stack-only view borrowing both
/// process/composition context and target-local dynamic bytes. No allocation, trait object, runtime
/// registry or temporary body vector is required on the service path.
pub trait PrePlayStagedPublisher: PrePlayTarget {
    /// Immutable plan view for one service opportunity.
    type StagedPlan<'a>: StagedPublicationPlan
    where
        Self: 'a,
        Self::Context: 'a,
        Self::State: 'a;

    /// Small owner-local token adopted only after successful bounded admission.
    type StagedCommit: Copy;

    /// Proposes currently active staged work, or `None` when no staged publication is ready.
    ///
    /// The returned plan is an immutable view and the cursor is only a copy. This method must not
    /// mutate live target state or queue bytes.
    ///
    /// # Errors
    ///
    /// Returns a target-specific validation error before bounded egress is touched.
    fn staged_publication<'a>(
        context: &'a Self::Context,
        session: SessionState,
        target_state: &'a Self::State,
    ) -> Result<
        Option<PrePlayStagedPublication<Self::StagedPlan<'a>, Self::StagedCommit>>,
        Self::Error,
    >;

    /// Adopts staged progression after the generic publication primitive completed successfully.
    ///
    /// This hook must be infallible and owner-local. The supplied cursor reflects exactly the one
    /// body or stage boundary committed by the service operation.
    fn commit_staged_publication(
        state: &mut Self::State,
        commit: Self::StagedCommit,
        cursor: StagedPublicationCursor,
        step: StagedPublicationStep,
    );
}

/// Result of one staged proactive-publication service opportunity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrePlayStagedPublicationProcess {
    /// The target has no staged proactive work ready in the current state.
    Idle,
    /// The target proposed work and exactly one body/stage-boundary operation completed.
    Progress(StagedPublicationStep),
}

impl<T> PrePlayConnection<T>
where
    T: PrePlayStagedPublisher,
{
    /// Services at most one staged body or one explicit stage boundary through existing egress.
    ///
    /// The target receives immutable context/session/state only. The binder then performs the sole
    /// queue operation through `crucible-publication-core`, drops the borrowed plan view, and only
    /// then adopts owner-local progression. No inbound bytes or generic session state change here.
    ///
    /// # Errors
    ///
    /// Returns fail-closed on a closed session, target proposal rejection, malformed/oversized body,
    /// egress backpressure or driver accounting failure. No error path commits staged progression.
    pub fn service_staged_publication(
        &mut self,
        context: &T::Context,
    ) -> Result<PrePlayStagedPublicationProcess, PrePlayError<T::Error>> {
        if self.session.phase() == crucible_session_core::SessionPhase::Closed {
            return Err(PrePlayError::ClosedSession);
        }

        let proposal = T::staged_publication(context, self.session, &self.target_state)
            .map_err(PrePlayError::Target)?;
        let Some(PrePlayStagedPublication {
            plan,
            mut cursor,
            commit,
        }) = proposal
        else {
            return Ok(PrePlayStagedPublicationProcess::Idle);
        };

        let step = publish_staged_plan_one::<TargetBoundaryError<T::Error>, _>(
            &plan,
            &mut cursor,
            &mut self.driver,
        )
        .map_err(map_driver_error)?;

        // The plan may borrow `target_state`; release that immutable borrow before owner-local
        // progression is adopted mutably.
        drop(plan);
        T::commit_staged_publication(&mut self.target_state, commit, cursor, step);
        Ok(PrePlayStagedPublicationProcess::Progress(step))
    }
}

#[cfg(test)]
mod tests {
    use crucible_connection_core::{ConnectionBufferError, ConnectionLimits, FrameView};
    use crucible_connection_driver::OutboundBatch;
    use crucible_publication_core::{
        StagedPublicationCursor, StagedPublicationLookup, StagedPublicationPlan,
        StagedPublicationStep,
    };
    use crucible_session_core::{SessionPhase, SessionState};

    use super::{
        PrePlayStagedPublication, PrePlayStagedPublicationProcess, PrePlayStagedPublisher,
    };
    use crate::{PrePlayAction, PrePlayConnection, PrePlayError, PrePlayTarget};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum StagedError {
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
    struct StagedState {
        cursor: StagedPublicationCursor,
        commits: usize,
        done: bool,
    }

    #[derive(Debug)]
    struct StagedContext {
        stages: Vec<Vec<Vec<u8>>>,
    }

    #[derive(Clone, Copy)]
    struct ContextPlan<'a> {
        context: &'a StagedContext,
    }

    impl StagedPublicationPlan for ContextPlan<'_> {
        fn lookup(&self, stage: usize, body: usize) -> StagedPublicationLookup<'_> {
            let Some(stage) = self.context.stages.get(stage) else {
                return StagedPublicationLookup::Complete;
            };
            match stage.get(body) {
                Some(body) => StagedPublicationLookup::Body(body),
                None => StagedPublicationLookup::StageComplete,
            }
        }
    }

    struct StagedTarget;

    impl PrePlayTarget for StagedTarget {
        type Error = StagedError;
        type Context = StagedContext;
        type State = StagedState;
        type Action = UnsupportedAction;

        fn decode(
            _context: &Self::Context,
            _session: SessionState,
            _target_state: &Self::State,
            _frame: FrameView<'_>,
        ) -> Result<Self::Action, Self::Error> {
            Err(StagedError::InboundUnsupported)
        }
    }

    impl PrePlayStagedPublisher for StagedTarget {
        type StagedPlan<'a> = ContextPlan<'a>;
        type StagedCommit = usize;

        fn staged_publication<'a>(
            context: &'a Self::Context,
            session: SessionState,
            target_state: &'a Self::State,
        ) -> Result<
            Option<PrePlayStagedPublication<Self::StagedPlan<'a>, Self::StagedCommit>>,
            Self::Error,
        > {
            if session.phase() != SessionPhase::Configuration || target_state.done {
                return Ok(None);
            }
            Ok(Some(PrePlayStagedPublication::new(
                ContextPlan { context },
                target_state.cursor,
                target_state.commits + 1,
            )))
        }

        fn commit_staged_publication(
            state: &mut Self::State,
            commit: Self::StagedCommit,
            cursor: StagedPublicationCursor,
            step: StagedPublicationStep,
        ) {
            state.cursor = cursor;
            state.commits = commit;
            if step == StagedPublicationStep::Complete {
                state.done = true;
            }
        }
    }

    fn limits(max_body: usize, egress: usize) -> ConnectionLimits {
        ConnectionLimits::new(max_body, 256, egress).expect("coherent staged test limits")
    }

    fn configuration_connection(limits: ConnectionLimits) -> PrePlayConnection<StagedTarget> {
        let mut connection = PrePlayConnection::<StagedTarget>::new(limits);
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
    fn staged_publisher_is_idle_outside_ready_phase() {
        let context = StagedContext {
            stages: vec![vec![vec![0x01]]],
        };
        let mut connection = PrePlayConnection::<StagedTarget>::new(limits(16, 64));

        assert_eq!(
            connection.service_staged_publication(&context),
            Ok(PrePlayStagedPublicationProcess::Idle)
        );
        assert_eq!(*connection.target_state(), StagedState::default());
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn staged_service_never_crosses_an_empty_stage_in_one_call() {
        let context = StagedContext {
            stages: vec![vec![vec![0x11]], Vec::new(), vec![vec![0x31, 0x32]]],
        };
        let mut connection = configuration_connection(limits(16, 64));

        assert!(matches!(
            connection.service_staged_publication(&context),
            Ok(PrePlayStagedPublicationProcess::Progress(
                StagedPublicationStep::Queued { stage: 0, index: 0, .. }
            ))
        ));
        assert_eq!(connection.target_state().cursor.body_index(), 1);

        assert_eq!(
            connection.service_staged_publication(&context),
            Ok(PrePlayStagedPublicationProcess::Progress(
                StagedPublicationStep::StageComplete { stage: 0 }
            ))
        );
        assert_eq!(connection.target_state().cursor.stage_index(), 1);

        assert_eq!(
            connection.service_staged_publication(&context),
            Ok(PrePlayStagedPublicationProcess::Progress(
                StagedPublicationStep::StageComplete { stage: 1 }
            ))
        );
        assert_eq!(connection.target_state().cursor.stage_index(), 2);
    }

    #[test]
    fn staged_backpressure_commits_neither_cursor_nor_token() {
        let context = StagedContext {
            stages: vec![vec![vec![0x41; 16], vec![0x42; 16]]],
        };
        let mut connection = configuration_connection(limits(16, 17));

        assert!(matches!(
            connection.service_staged_publication(&context),
            Ok(PrePlayStagedPublicationProcess::Progress(
                StagedPublicationStep::Queued { index: 0, .. }
            ))
        ));
        let state_before = *connection.target_state();
        let egress_before = connection.pending_egress().to_vec();

        assert!(matches!(
            connection.service_staged_publication(&context),
            Err(PrePlayError::Buffer(
                ConnectionBufferError::EgressLimitExceeded { .. }
            ))
        ));
        assert_eq!(*connection.target_state(), state_before);
        assert_eq!(connection.pending_egress(), egress_before);
    }
}
