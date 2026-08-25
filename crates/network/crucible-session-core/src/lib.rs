//! Fail-closed target-version-agnostic connection phase state for Crucible.
//!
//! Packet IDs and protocol-version semantics live above this crate. The state shell only owns the
//! one-way lifecycle law that a validated packet handler may explicitly request.

#![forbid(unsafe_code)]

mod liveness;

pub use liveness::{
    KeepAliveReply, LivenessDecision, LivenessError, LivenessPolicy, LivenessPolicyError,
    LivenessState,
};

/// Target-version-agnostic connection phase.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SessionPhase {
    /// Initial phase before the peer selects status or login intent.
    Handshake,
    /// Server-list status/ping phase.
    Status,
    /// Login phase before configuration begins.
    Login,
    /// Modern Java configuration phase before entering play.
    Configuration,
    /// Gameplay packet phase.
    Play,
    /// Terminal state. A closed session can never be reopened.
    Closed,
}

/// Fail-closed session transition error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStateError {
    /// A requested forward transition is not an admitted lifecycle edge.
    InvalidTransition {
        from: SessionPhase,
        to: SessionPhase,
    },
    /// A transition was requested after terminal closure.
    AlreadyClosed,
}

/// Minimal connection-state shell.
///
/// The shell deliberately stores no packet history, callbacks, runtime handles or policy objects.
/// Callers that need audit history should record successful transitions outside this HOT-neutral
/// lifecycle primitive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionState {
    phase: SessionPhase,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    /// Creates a new session in [`SessionPhase::Handshake`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: SessionPhase::Handshake,
        }
    }

    /// Returns the current connection phase.
    #[must_use]
    pub const fn phase(self) -> SessionPhase {
        self.phase
    }

    /// Requests one admitted non-terminal forward transition.
    ///
    /// The only legal edges are:
    ///
    /// - `Handshake -> Status`
    /// - `Handshake -> Login`
    /// - `Login -> Configuration`
    /// - `Configuration -> Play`
    ///
    /// Closure uses [`Self::close`] so terminal behavior is explicit.
    ///
    /// # Errors
    ///
    /// Returns [`SessionStateError::AlreadyClosed`] from the terminal phase, or
    /// [`SessionStateError::InvalidTransition`] for repeated, skipped, backward or direct-to-closed
    /// transitions. Failure never changes the current phase.
    pub fn advance(&mut self, to: SessionPhase) -> Result<(), SessionStateError> {
        let from = self.phase;
        if from == SessionPhase::Closed {
            return Err(SessionStateError::AlreadyClosed);
        }
        let allowed = matches!(
            (from, to),
            (
                SessionPhase::Handshake,
                SessionPhase::Status | SessionPhase::Login
            ) | (SessionPhase::Login, SessionPhase::Configuration)
                | (SessionPhase::Configuration, SessionPhase::Play)
        );
        if !allowed {
            return Err(SessionStateError::InvalidTransition { from, to });
        }
        self.phase = to;
        Ok(())
    }

    /// Closes the session terminally.
    ///
    /// Returns `true` when this call performed the transition and `false` when the session was
    /// already closed. Closure is therefore idempotent, and there is no reopening operation.
    #[must_use = "the return value reports whether closure changed the session"]
    pub fn close(&mut self) -> bool {
        if self.phase == SessionPhase::Closed {
            return false;
        }
        self.phase = SessionPhase::Closed;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionPhase, SessionState, SessionStateError};

    const PHASES: [SessionPhase; 6] = [
        SessionPhase::Handshake,
        SessionPhase::Status,
        SessionPhase::Login,
        SessionPhase::Configuration,
        SessionPhase::Play,
        SessionPhase::Closed,
    ];
    const ADVANCE_TARGETS: [SessionPhase; 5] = [
        SessionPhase::Handshake,
        SessionPhase::Status,
        SessionPhase::Login,
        SessionPhase::Configuration,
        SessionPhase::Play,
    ];

    const fn expected_advance(from: SessionPhase, to: SessionPhase) -> bool {
        matches!(
            (from, to),
            (
                SessionPhase::Handshake,
                SessionPhase::Status | SessionPhase::Login
            ) | (SessionPhase::Login, SessionPhase::Configuration)
                | (SessionPhase::Configuration, SessionPhase::Play)
        )
    }

    fn state_at(phase: SessionPhase) -> SessionState {
        let mut state = SessionState::new();
        match phase {
            SessionPhase::Handshake => {}
            SessionPhase::Status => state.advance(SessionPhase::Status).expect("status edge"),
            SessionPhase::Login => state.advance(SessionPhase::Login).expect("login edge"),
            SessionPhase::Configuration => {
                state.advance(SessionPhase::Login).expect("login edge");
                state
                    .advance(SessionPhase::Configuration)
                    .expect("configuration edge");
            }
            SessionPhase::Play => {
                state.advance(SessionPhase::Login).expect("login edge");
                state
                    .advance(SessionPhase::Configuration)
                    .expect("configuration edge");
                state.advance(SessionPhase::Play).expect("play edge");
            }
            SessionPhase::Closed => {
                assert!(state.close());
            }
        }
        state
    }

    #[test]
    fn every_phase_pair_matches_the_admitted_transition_matrix() {
        for from in PHASES {
            for to in PHASES {
                let mut state = state_at(from);
                let before = state;
                let result = state.advance(to);
                if expected_advance(from, to) {
                    assert_eq!(result, Ok(()), "from={from:?} to={to:?}");
                    assert_eq!(state.phase(), to);
                } else if from == SessionPhase::Closed {
                    assert_eq!(result, Err(SessionStateError::AlreadyClosed));
                    assert_eq!(state, before);
                } else {
                    assert_eq!(
                        result,
                        Err(SessionStateError::InvalidTransition { from, to }),
                        "from={from:?} to={to:?}"
                    );
                    assert_eq!(state, before);
                }
            }
        }
    }

    #[test]
    fn both_complete_lifecycle_routes_are_exact() {
        let mut status = SessionState::new();
        let mut status_history = vec![status.phase()];
        status.advance(SessionPhase::Status).expect("status");
        status_history.push(status.phase());
        assert!(status.close());
        status_history.push(status.phase());
        assert_eq!(
            status_history,
            [
                SessionPhase::Handshake,
                SessionPhase::Status,
                SessionPhase::Closed
            ]
        );

        let mut play = SessionState::new();
        let mut play_history = vec![play.phase()];
        for next in [
            SessionPhase::Login,
            SessionPhase::Configuration,
            SessionPhase::Play,
        ] {
            play.advance(next).expect("legal play route");
            play_history.push(play.phase());
        }
        assert!(play.close());
        play_history.push(play.phase());
        assert_eq!(
            play_history,
            [
                SessionPhase::Handshake,
                SessionPhase::Login,
                SessionPhase::Configuration,
                SessionPhase::Play,
                SessionPhase::Closed
            ]
        );
    }

    #[test]
    fn close_is_idempotent_from_every_phase_and_never_reopens() {
        for phase in PHASES {
            let mut state = state_at(phase);
            let first = state.close();
            assert_eq!(first, phase != SessionPhase::Closed);
            assert_eq!(state.phase(), SessionPhase::Closed);
            assert!(!state.close());
            for target in PHASES {
                assert_eq!(state.advance(target), Err(SessionStateError::AlreadyClosed));
                assert_eq!(state.phase(), SessionPhase::Closed);
            }
        }
    }

    #[test]
    fn failed_transition_never_partially_advances_state() {
        let mut state = SessionState::new();
        for target in [
            SessionPhase::Handshake,
            SessionPhase::Configuration,
            SessionPhase::Play,
            SessionPhase::Closed,
        ] {
            let before = state;
            assert!(state.advance(target).is_err());
            assert_eq!(state, before);
        }
        assert_eq!(state.phase(), SessionPhase::Handshake);
    }

    #[test]
    fn long_adversarial_transition_corpus_is_deterministic_and_fail_closed() {
        const ATTEMPTS: usize = 100_000;
        const EPOCH: usize = 997;
        let mut left = SessionState::new();
        let mut right = SessionState::new();
        let mut rng = 0xD1B5_4A32_u32;
        let mut accepted = 0_usize;
        let mut rejected = 0_usize;
        let mut closed_epochs = 0_usize;

        for attempt in 0..ATTEMPTS {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let target_index = usize::try_from(rng % 5).expect("bounded phase index");
            let target = ADVANCE_TARGETS[target_index];

            let left_before = left;
            let right_before = right;
            let left_result = left.advance(target);
            let right_result = right.advance(target);
            assert_eq!(left_result, right_result);
            assert_eq!(left, right);
            if left_result.is_ok() {
                accepted += 1;
            } else {
                rejected += 1;
                assert_eq!(left, left_before);
                assert_eq!(right, right_before);
            }

            if (attempt + 1).is_multiple_of(EPOCH) {
                assert_eq!(left.close(), right.close());
                assert_eq!(left.phase(), SessionPhase::Closed);
                for reopen_target in ADVANCE_TARGETS {
                    assert_eq!(
                        left.advance(reopen_target),
                        Err(SessionStateError::AlreadyClosed)
                    );
                    assert_eq!(
                        right.advance(reopen_target),
                        Err(SessionStateError::AlreadyClosed)
                    );
                }
                left = SessionState::new();
                right = SessionState::new();
                closed_epochs += 1;
            }
        }

        assert!(accepted > 0);
        assert!(rejected > accepted);
        assert!(closed_epochs > 50);
        assert_eq!(left, right);
        assert!(left.close());
        assert_eq!(left.phase(), SessionPhase::Closed);
    }
}
