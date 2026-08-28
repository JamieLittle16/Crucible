//! Deterministic connection-liveness state independent of wall-clock and packet IDs.
//!
//! The caller supplies monotone milliseconds and target-specific timing policy. The state exposes
//! its next semantic deadline so a scheduler can wake only due sessions instead of scanning every
//! resident connection every simulation tick.

use core::mem::size_of;

const FLAG_KEEP_ALIVE_PENDING: u8 = 1 << 0;
const FLAG_CLOSED: u8 = 1 << 1;
const MAX_MONOTONE_MILLIS: u64 = i64::MAX.unsigned_abs();

/// Target-selected liveness timing policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivenessPolicy {
    keep_alive_interval_ms: u64,
    closed_timeout_ms: u64,
}

/// Invalid liveness policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivenessPolicyError {
    /// A deadline interval must be non-zero.
    ZeroInterval,
    /// The interval is too large for the admitted monotone/wire-time domain.
    IntervalTooLarge,
}

impl LivenessPolicy {
    /// Constructs a target timing policy.
    ///
    /// # Errors
    ///
    /// Returns an error for zero intervals or intervals greater than `i64::MAX` milliseconds.
    pub const fn new(
        keep_alive_interval_ms: u64,
        closed_timeout_ms: u64,
    ) -> Result<Self, LivenessPolicyError> {
        if keep_alive_interval_ms == 0 || closed_timeout_ms == 0 {
            return Err(LivenessPolicyError::ZeroInterval);
        }
        if keep_alive_interval_ms > MAX_MONOTONE_MILLIS || closed_timeout_ms > MAX_MONOTONE_MILLIS {
            return Err(LivenessPolicyError::IntervalTooLarge);
        }
        Ok(Self {
            keep_alive_interval_ms,
            closed_timeout_ms,
        })
    }

    /// Keep-alive challenge interval in milliseconds.
    #[must_use]
    pub const fn keep_alive_interval_ms(self) -> u64 {
        self.keep_alive_interval_ms
    }

    /// Terminal-listener linger timeout in milliseconds.
    #[must_use]
    pub const fn closed_timeout_ms(self) -> u64 {
        self.closed_timeout_ms
    }
}

/// Fail-closed time-domain errors from [`LivenessState`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivenessError {
    /// Supplied monotone milliseconds exceed the signed 64-bit challenge domain.
    TimeOutOfRange { now_ms: u64 },
    /// A caller supplied time older than the latest timestamp retained by this state.
    TimeWentBackwards { previous_ms: u64, now_ms: u64 },
}

/// Result of servicing one liveness deadline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivenessDecision {
    /// No externally visible liveness action is due.
    Idle,
    /// Publish one keep-alive challenge carrying this exact signed 64-bit identifier.
    IssueChallenge { id: i64 },
    /// A previous challenge remained pending through the next keep-alive deadline.
    KeepAliveTimedOut,
    /// A terminal listener remained present through its close timeout.
    ClosedTimedOut,
}

/// Result of receiving one keep-alive response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeepAliveReply {
    /// The response exactly matched the pending challenge and updated measured latency.
    Accepted { latency_ms: i32 },
    /// No challenge was pending or the response identifier did not match it.
    Rejected,
}

/// Compact deterministic per-connection liveness state.
///
/// The state deliberately owns no clock, task, socket, packet object or scheduler node. Callers
/// drive it with monotone milliseconds and may use [`Self::next_deadline_ms`] to maintain an active
/// deadline frontier rather than polling all connections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivenessState {
    keep_alive_anchor_ms: u64,
    pending_challenge: i64,
    closed_since_ms: u64,
    latency_ms: i32,
    flags: u8,
}

impl LivenessState {
    /// Creates live state anchored at connection/listener construction time.
    ///
    /// # Errors
    ///
    /// Returns [`LivenessError::TimeOutOfRange`] when `now_ms` cannot be represented by the signed
    /// 64-bit challenge value used by the target wire contract.
    pub const fn new(now_ms: u64, initial_latency_ms: i32) -> Result<Self, LivenessError> {
        if now_ms > MAX_MONOTONE_MILLIS {
            return Err(LivenessError::TimeOutOfRange { now_ms });
        }
        Ok(Self {
            keep_alive_anchor_ms: now_ms,
            pending_challenge: 0,
            closed_since_ms: 0,
            latency_ms: initial_latency_ms,
            flags: 0,
        })
    }

    /// Current smoothed latency in milliseconds.
    #[must_use]
    pub const fn latency_ms(self) -> i32 {
        self.latency_ms
    }

    /// Whether an exact keep-alive challenge currently awaits acknowledgement.
    #[must_use]
    pub const fn keep_alive_pending(self) -> bool {
        self.flags & FLAG_KEEP_ALIVE_PENDING != 0
    }

    /// Pending challenge identifier, when one exists.
    #[must_use]
    pub const fn pending_challenge(self) -> Option<i64> {
        if self.keep_alive_pending() {
            Some(self.pending_challenge)
        } else {
            None
        }
    }

    /// Whether the listener has entered terminal/closed linger state.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        self.flags & FLAG_CLOSED != 0
    }

    /// Exact next time at which servicing this state can produce an action.
    ///
    /// A pending keep-alive retains timeout priority even after the listener closes, matching the
    /// source branch order. A closed listener with no pending challenge becomes actionable only
    /// after both the keep-alive gate and the closed-listener timeout are due.
    #[must_use]
    pub const fn next_deadline_ms(self, policy: LivenessPolicy) -> u64 {
        let keep_alive_deadline = self.keep_alive_anchor_ms + policy.keep_alive_interval_ms;
        if self.keep_alive_pending() || !self.is_closed() {
            keep_alive_deadline
        } else {
            let closed_deadline = self.closed_since_ms + policy.closed_timeout_ms;
            if closed_deadline > keep_alive_deadline {
                closed_deadline
            } else {
                keep_alive_deadline
            }
        }
    }

    /// Services the state at one caller-supplied monotone time.
    ///
    /// At a keep-alive deadline, an outstanding challenge times out first. Otherwise a closed
    /// listener suppresses challenge publication and observes its terminal linger timeout. A live
    /// listener issues a challenge and advances the keep-alive anchor to the issue time.
    ///
    /// # Errors
    ///
    /// Fails without mutation for out-of-range or backwards time relative to retained state.
    pub fn service(
        &mut self,
        now_ms: u64,
        policy: LivenessPolicy,
    ) -> Result<LivenessDecision, LivenessError> {
        self.validate_now(now_ms)?;

        if now_ms - self.keep_alive_anchor_ms < policy.keep_alive_interval_ms {
            return Ok(LivenessDecision::Idle);
        }
        if self.keep_alive_pending() {
            return Ok(LivenessDecision::KeepAliveTimedOut);
        }
        if self.is_closed() {
            if now_ms - self.closed_since_ms >= policy.closed_timeout_ms {
                return Ok(LivenessDecision::ClosedTimedOut);
            }
            return Ok(LivenessDecision::Idle);
        }

        let id = i64::try_from(now_ms).map_err(|_| LivenessError::TimeOutOfRange { now_ms })?;
        self.keep_alive_anchor_ms = now_ms;
        self.pending_challenge = id;
        self.flags |= FLAG_KEEP_ALIVE_PENDING;
        Ok(LivenessDecision::IssueChallenge { id })
    }

    /// Applies one keep-alive response at caller-supplied monotone time.
    ///
    /// A rejected response does not mutate state. The caller owns target policy for rejection (for
    /// example, a dedicated 26.2 server disconnects while the integrated-server owner is exempt).
    ///
    /// The accepted latency update intentionally uses Java-`int` wrapping arithmetic and truncating
    /// division so the state machine remains exact even outside ordinary latency ranges.
    ///
    /// # Errors
    ///
    /// Fails without mutation for out-of-range or backwards time relative to retained state.
    pub fn receive_keep_alive(
        &mut self,
        now_ms: u64,
        id: i64,
    ) -> Result<KeepAliveReply, LivenessError> {
        self.validate_now(now_ms)?;
        if !self.keep_alive_pending() || id != self.pending_challenge {
            return Ok(KeepAliveReply::Rejected);
        }

        let elapsed_ms = java_i32_narrow(now_ms - self.keep_alive_anchor_ms);
        self.latency_ms = self.latency_ms.wrapping_mul(3).wrapping_add(elapsed_ms) / 4;
        self.flags &= !FLAG_KEEP_ALIVE_PENDING;
        Ok(KeepAliveReply::Accepted {
            latency_ms: self.latency_ms,
        })
    }

    /// Enters terminal listener linger state once.
    ///
    /// Returns `true` when this call recorded the close edge and `false` when already closed. The
    /// first close timestamp is retained exactly. Closing does not erase a pending keep-alive;
    /// pending timeout retains the source-defined branch priority.
    ///
    /// # Errors
    ///
    /// Fails without mutation for out-of-range or backwards time relative to retained state.
    pub fn close(&mut self, now_ms: u64) -> Result<bool, LivenessError> {
        self.validate_now(now_ms)?;
        if self.is_closed() {
            return Ok(false);
        }
        self.closed_since_ms = now_ms;
        self.flags |= FLAG_CLOSED;
        Ok(true)
    }

    fn validate_now(self, now_ms: u64) -> Result<(), LivenessError> {
        if now_ms > MAX_MONOTONE_MILLIS {
            return Err(LivenessError::TimeOutOfRange { now_ms });
        }
        let previous_ms = if self.is_closed() {
            self.closed_since_ms.max(self.keep_alive_anchor_ms)
        } else {
            self.keep_alive_anchor_ms
        };
        if now_ms < previous_ms {
            return Err(LivenessError::TimeWentBackwards {
                previous_ms,
                now_ms,
            });
        }
        Ok(())
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "Minecraft 26.2 explicitly narrows elapsed long milliseconds to Java int before latency smoothing"
)]
const fn java_i32_narrow(value: u64) -> i32 {
    value as i32
}

const _: () = assert!(size_of::<LivenessState>() == 32);

#[cfg(test)]
mod tests {
    use super::{
        KeepAliveReply, LivenessDecision, LivenessError, LivenessPolicy, LivenessPolicyError,
        LivenessState,
    };

    const POLICY: LivenessPolicy = match LivenessPolicy::new(15_000, 15_000) {
        Ok(policy) => policy,
        Err(_) => panic!("test policy must be valid"),
    };

    #[test]
    fn policy_rejects_zero_and_unrepresentable_intervals() {
        assert_eq!(
            LivenessPolicy::new(0, 1),
            Err(LivenessPolicyError::ZeroInterval)
        );
        assert_eq!(
            LivenessPolicy::new(1, 0),
            Err(LivenessPolicyError::ZeroInterval)
        );
        assert_eq!(
            LivenessPolicy::new(i64::MAX.unsigned_abs() + 1, 1),
            Err(LivenessPolicyError::IntervalTooLarge)
        );
    }

    #[test]
    fn exact_deadline_issues_timestamp_challenge() {
        let mut state = LivenessState::new(1_000, 0).expect("valid start");
        assert_eq!(state.next_deadline_ms(POLICY), 16_000);
        assert_eq!(state.service(15_999, POLICY), Ok(LivenessDecision::Idle));
        assert_eq!(
            state.service(16_000, POLICY),
            Ok(LivenessDecision::IssueChallenge { id: 16_000 })
        );
        assert_eq!(state.pending_challenge(), Some(16_000));
        assert_eq!(state.next_deadline_ms(POLICY), 31_000);
    }

    #[test]
    fn pending_challenge_times_out_at_next_deadline() {
        let mut state = LivenessState::new(0, 0).expect("valid start");
        assert_eq!(
            state.service(15_000, POLICY),
            Ok(LivenessDecision::IssueChallenge { id: 15_000 })
        );
        assert_eq!(state.service(29_999, POLICY), Ok(LivenessDecision::Idle));
        let before = state;
        assert_eq!(
            state.service(30_000, POLICY),
            Ok(LivenessDecision::KeepAliveTimedOut)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn exact_reply_updates_latency_and_does_not_reset_schedule() {
        let mut state = LivenessState::new(0, 40).expect("valid start");
        assert_eq!(
            state.service(15_000, POLICY),
            Ok(LivenessDecision::IssueChallenge { id: 15_000 })
        );
        assert_eq!(
            state.receive_keep_alive(15_120, 15_000),
            Ok(KeepAliveReply::Accepted { latency_ms: 60 })
        );
        assert!(!state.keep_alive_pending());
        assert_eq!(state.latency_ms(), 60);
        assert_eq!(state.next_deadline_ms(POLICY), 30_000);
        assert_eq!(state.service(29_999, POLICY), Ok(LivenessDecision::Idle));
        assert_eq!(
            state.service(30_000, POLICY),
            Ok(LivenessDecision::IssueChallenge { id: 30_000 })
        );
    }

    #[test]
    fn wrong_or_unexpected_reply_is_rejected_without_mutation() {
        let mut state = LivenessState::new(0, 7).expect("valid start");
        let before_idle = state;
        assert_eq!(
            state.receive_keep_alive(1_000, 99),
            Ok(KeepAliveReply::Rejected)
        );
        assert_eq!(state, before_idle);

        state.service(15_000, POLICY).expect("challenge");
        let before_wrong = state;
        assert_eq!(
            state.receive_keep_alive(15_010, 14_999),
            Ok(KeepAliveReply::Rejected)
        );
        assert_eq!(state, before_wrong);
    }

    #[test]
    fn close_without_pending_suppresses_challenge_and_uses_linger_deadline() {
        let mut state = LivenessState::new(0, 0).expect("valid start");
        assert_eq!(state.close(1_000), Ok(true));
        assert_eq!(state.close(2_000), Ok(false));
        assert!(state.is_closed());
        assert_eq!(state.next_deadline_ms(POLICY), 16_000);
        assert_eq!(state.service(15_000, POLICY), Ok(LivenessDecision::Idle));
        assert_eq!(
            state.service(16_000, POLICY),
            Ok(LivenessDecision::ClosedTimedOut)
        );
    }

    #[test]
    fn pending_timeout_retains_priority_after_close() {
        let mut state = LivenessState::new(0, 0).expect("valid start");
        state.service(15_000, POLICY).expect("challenge");
        assert_eq!(state.close(16_000), Ok(true));
        assert_eq!(state.next_deadline_ms(POLICY), 30_000);
        assert_eq!(
            state.service(30_000, POLICY),
            Ok(LivenessDecision::KeepAliveTimedOut)
        );
    }

    #[test]
    fn backwards_and_out_of_range_time_fail_without_mutation() {
        let mut state = LivenessState::new(10_000, 0).expect("valid start");
        let before = state;
        assert_eq!(
            state.service(9_999, POLICY),
            Err(LivenessError::TimeWentBackwards {
                previous_ms: 10_000,
                now_ms: 9_999,
            })
        );
        assert_eq!(state, before);
        let too_large = i64::MAX.unsigned_abs() + 1;
        assert_eq!(
            state.receive_keep_alive(too_large, 0),
            Err(LivenessError::TimeOutOfRange { now_ms: too_large })
        );
        assert_eq!(state, before);
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct ReferenceState {
        anchor: u64,
        pending: Option<i64>,
        closed_since: Option<u64>,
        latency: i32,
    }

    impl ReferenceState {
        fn new(now: u64, latency: i32) -> Self {
            Self {
                anchor: now,
                pending: None,
                closed_since: None,
                latency,
            }
        }

        fn service(&mut self, now: u64) -> LivenessDecision {
            if now - self.anchor < 15_000 {
                return LivenessDecision::Idle;
            }
            if self.pending.is_some() {
                return LivenessDecision::KeepAliveTimedOut;
            }
            if let Some(closed) = self.closed_since {
                return if now - closed >= 15_000 {
                    LivenessDecision::ClosedTimedOut
                } else {
                    LivenessDecision::Idle
                };
            }
            self.anchor = now;
            self.pending = Some(i64::try_from(now).expect("reference time fits i64"));
            LivenessDecision::IssueChallenge {
                id: i64::try_from(now).expect("reference time fits i64"),
            }
        }

        fn reply(&mut self, now: u64, id: i64) -> KeepAliveReply {
            if self.pending != Some(id) {
                return KeepAliveReply::Rejected;
            }
            let elapsed = super::java_i32_narrow(now - self.anchor);
            self.latency = self.latency.wrapping_mul(3).wrapping_add(elapsed) / 4;
            self.pending = None;
            KeepAliveReply::Accepted {
                latency_ms: self.latency,
            }
        }

        fn close(&mut self, now: u64) -> bool {
            if self.closed_since.is_some() {
                return false;
            }
            self.closed_since = Some(now);
            true
        }
    }

    #[test]
    fn hundred_thousand_event_trace_matches_independent_reference() {
        const EVENTS: usize = 100_000;
        let mut candidate = LivenessState::new(0, 17).expect("valid start");
        let mut reference = ReferenceState::new(0, 17);
        let mut now = 0_u64;
        let mut rng = 0xA5D3_7F91_C62B_480D_u64;

        for event in 0..EVENTS {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            now += rng % 901;

            if event != 0 && event.is_multiple_of(5_003) {
                assert_eq!(candidate.close(now), Ok(reference.close(now)));
            } else if rng.trailing_zeros() >= 2 {
                let fallback_id = i64::from_ne_bytes(rng.to_ne_bytes()) ^ 0x55AA;
                let expected_id = reference.pending.unwrap_or(fallback_id);
                let id = if rng & 0x10 == 0 {
                    expected_id
                } else {
                    expected_id.wrapping_add(1)
                };
                assert_eq!(
                    candidate.receive_keep_alive(now, id),
                    Ok(reference.reply(now, id))
                );
            } else {
                assert_eq!(candidate.service(now, POLICY), Ok(reference.service(now)));
            }

            assert_eq!(candidate.latency_ms(), reference.latency);
            assert_eq!(candidate.pending_challenge(), reference.pending);
            assert_eq!(candidate.is_closed(), reference.closed_since.is_some());

            if reference.closed_since.is_some() && event.is_multiple_of(5_003) {
                candidate = LivenessState::new(now, reference.latency).expect("restart candidate");
                reference = ReferenceState::new(now, reference.latency);
            }
        }
    }
}
