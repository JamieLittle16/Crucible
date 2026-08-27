//! Deterministic oracle for Crucible ownership, migration and staged-effect laws.
//!
//! This crate is qualification infrastructure, not a production scheduler. It deliberately keeps
//! semantic authority separate from worker topology so later threaded executors can be checked
//! against a small deterministic model.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

/// Stable identity of one independently owned mutable semantic domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomainId(pub u16);

/// Logical executor identity used only as an ownership placement in the simulator.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkerId(pub u16);

/// Monotone identity of one ownership incarnation of a domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OwnershipGeneration(pub u64);

/// Monotone semantic revision inside one ownership generation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomainRevision(pub u64);

/// Generation/revision identity observed by freshness-sensitive work.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DomainStamp {
    /// Ownership generation observed by the work.
    pub generation: OwnershipGeneration,
    /// Semantic revision observed by the work.
    pub revision: DomainRevision,
}

/// Unforgeable-by-API proof of the worker/generation currently authorized to mutate a domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MutationToken {
    domain: DomainId,
    worker: WorkerId,
    generation: OwnershipGeneration,
}

impl MutationToken {
    /// Domain whose ordinary mutation authority this token represents.
    #[must_use]
    pub const fn domain(self) -> DomainId {
        self.domain
    }

    /// Logical worker currently executing that authority.
    #[must_use]
    pub const fn worker(self) -> WorkerId {
        self.worker
    }

    /// Ownership generation bound into the token.
    #[must_use]
    pub const fn generation(self) -> OwnershipGeneration {
        self.generation
    }
}

/// Stable identifier for one deferred-work product.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeferredId(pub u64);

/// Stable identifier for one typed cross-domain effect.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EffectId(pub u64);

/// Freshness contract attached to deferred work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredFreshness {
    /// Installation requires the exact generation and revision observed at preparation time.
    ExactStamp,
    /// Installation may tolerate intervening mutations but not an ownership-generation change.
    SameGeneration,
}

/// Prepared work that may execute anywhere but installs only through current authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeferredWork {
    id: DeferredId,
    domain: DomainId,
    observed: DomainStamp,
    delta: i64,
    freshness: DeferredFreshness,
}

impl DeferredWork {
    /// Stable identity used to reject duplicate installation.
    #[must_use]
    pub const fn id(self) -> DeferredId {
        self.id
    }

    /// Domain whose semantic state this work would modify.
    #[must_use]
    pub const fn domain(self) -> DomainId {
        self.domain
    }

    /// State identity captured when work was prepared.
    #[must_use]
    pub const fn observed(self) -> DomainStamp {
        self.observed
    }

    /// Freshness contract required for installation.
    #[must_use]
    pub const fn freshness(self) -> DeferredFreshness {
        self.freshness
    }
}

/// Typed semantic payload carried across an ownership-domain boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectPayload {
    /// Add one signed delta to the target's scalar semantic test state.
    Add(i64),
    /// Raise the target to at least this value.
    RaiseToAtLeast(i64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CrossDomainEffect {
    id: EffectId,
    source: DomainId,
    target: DomainId,
    payload: EffectPayload,
}

/// Exact handoff proof produced by the first half of a migration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationHandoff {
    domain: DomainId,
    from: WorkerId,
    to: WorkerId,
    generation: OwnershipGeneration,
    revision: DomainRevision,
}

/// One topology-independent semantic image captured only at a closed stage boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDigest {
    /// Number of fully completed semantic stages.
    pub completed_stages: u64,
    /// Ordered `(domain, value, generation, revision)` tuples. Worker IDs are intentionally absent.
    pub domains: Vec<(DomainId, i64, OwnershipGeneration, DomainRevision)>,
}

/// Fail-closed simulator errors. Illegal schedules are evidence failures rather than hidden repairs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipError {
    /// At least one mutable domain is required.
    EmptyDomainSet,
    /// A domain identity appeared more than once at construction.
    DuplicateDomain { domain: DomainId },
    /// An operation referenced a domain absent from this simulation.
    UnknownDomain { domain: DomainId },
    /// Ordinary stage work was attempted outside an open semantic stage.
    StageClosed,
    /// A new stage was requested before the current one was closed.
    StageAlreadyOpen,
    /// A stage cannot open while any domain remains between migration begin/commit.
    MigrationStillOpen { domain: DomainId },
    /// Migration is legal only at a closed stage boundary.
    MigrationRequiresClosedStage,
    /// A token does not match the current active owner/generation.
    NotCurrentAuthority { domain: DomainId },
    /// The domain is deliberately without ordinary mutation authority during migration.
    DomainMigrating { domain: DomainId },
    /// A migration commit did not match the exact handoff currently in progress.
    HandoffMismatch { domain: DomainId },
    /// One deferred identity was prepared more than once.
    DuplicateDeferredPreparation { id: DeferredId },
    /// One deferred product was installed more than once.
    DuplicateDeferredInstall { id: DeferredId },
    /// Deferred work failed its declared generation/revision freshness contract.
    StaleDeferred { id: DeferredId },
    /// A cross-domain effect identity was emitted more than once.
    DuplicateEffect { id: EffectId },
    /// Foreign effects must actually cross an authority-domain boundary.
    SelfTargetedEffect { domain: DomainId },
    /// Signed semantic test state overflowed rather than wrapping silently.
    ValueOverflow { domain: DomainId },
    /// A generation or revision counter exhausted rather than wrapping silently.
    CounterOverflow { domain: DomainId },
    /// The global semantic-stage counter exhausted rather than wrapping silently.
    StageCounterOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthorityState {
    Active {
        owner: WorkerId,
        generation: OwnershipGeneration,
    },
    Migrating {
        from: WorkerId,
        to: WorkerId,
        generation: OwnershipGeneration,
        revision: DomainRevision,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DomainState {
    authority: AuthorityState,
    revision: DomainRevision,
    value: i64,
}

/// Deterministic semantic oracle for ownership and staged cross-domain execution.
#[derive(Debug)]
pub struct OwnershipSimulator {
    domains: BTreeMap<DomainId, DomainState>,
    stage: u64,
    stage_open: bool,
    stage_snapshot: BTreeMap<DomainId, i64>,
    pending_effects: Vec<CrossDomainEffect>,
    seen_effects: BTreeSet<EffectId>,
    seen_deferred: BTreeSet<DeferredId>,
    installed_deferred: BTreeSet<DeferredId>,
}

impl OwnershipSimulator {
    /// Creates a simulator with one active authority for every supplied domain.
    ///
    /// Each domain starts at ownership generation `1`, revision `0`, and stage `0` is immediately
    /// open with a stable snapshot of the initial values.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty domain set or duplicate domain identity.
    pub fn new(
        domains: impl IntoIterator<Item = (DomainId, WorkerId, i64)>,
    ) -> Result<Self, OwnershipError> {
        let mut states = BTreeMap::new();
        for (domain, owner, value) in domains {
            let previous = states.insert(
                domain,
                DomainState {
                    authority: AuthorityState::Active {
                        owner,
                        generation: OwnershipGeneration(1),
                    },
                    revision: DomainRevision::default(),
                    value,
                },
            );
            if previous.is_some() {
                return Err(OwnershipError::DuplicateDomain { domain });
            }
        }
        if states.is_empty() {
            return Err(OwnershipError::EmptyDomainSet);
        }
        let stage_snapshot = states
            .iter()
            .map(|(&domain, state)| (domain, state.value))
            .collect();
        Ok(Self {
            domains: states,
            stage: 0,
            stage_open: true,
            stage_snapshot,
            pending_effects: Vec::new(),
            seen_effects: BTreeSet::new(),
            seen_deferred: BTreeSet::new(),
            installed_deferred: BTreeSet::new(),
        })
    }

    /// Current semantic stage number.
    #[must_use]
    pub const fn stage(&self) -> u64 {
        self.stage
    }

    /// Whether ordinary staged work may currently execute.
    #[must_use]
    pub const fn is_stage_open(&self) -> bool {
        self.stage_open
    }

    /// Returns the current topology-dependent owner token for one active domain.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown domain or while that domain is in migration handoff.
    pub fn token(&self, domain: DomainId) -> Result<MutationToken, OwnershipError> {
        let state = self.domain(domain)?;
        match state.authority {
            AuthorityState::Active { owner, generation } => Ok(MutationToken {
                domain,
                worker: owner,
                generation,
            }),
            AuthorityState::Migrating { .. } => Err(OwnershipError::DomainMigrating { domain }),
        }
    }

    /// Validates a token against current authority without performing a mutation.
    ///
    /// # Errors
    ///
    /// Returns an error when the token is stale, foreign, or the domain is mid-migration.
    pub fn validate_token(&self, token: MutationToken) -> Result<(), OwnershipError> {
        self.require_token(token).map(|_| ())
    }

    /// Current semantic value of one domain.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist.
    pub fn value(&self, domain: DomainId) -> Result<i64, OwnershipError> {
        Ok(self.domain(domain)?.value)
    }

    /// Current generation/revision identity of one domain.
    ///
    /// # Errors
    ///
    /// Returns an error when the domain does not exist or is mid-migration.
    pub fn stamp(&self, domain: DomainId) -> Result<DomainStamp, OwnershipError> {
        let state = self.domain(domain)?;
        let AuthorityState::Active { generation, .. } = state.authority else {
            return Err(OwnershipError::DomainMigrating { domain });
        };
        Ok(DomainStamp {
            generation,
            revision: state.revision,
        })
    }

    /// Reads the stage-stable value captured before any operation in the current stage.
    ///
    /// # Errors
    ///
    /// Returns an error outside an open stage or for an unknown domain.
    pub fn stage_value(&self, domain: DomainId) -> Result<i64, OwnershipError> {
        self.require_open_stage()?;
        self.stage_snapshot
            .get(&domain)
            .copied()
            .ok_or(OwnershipError::UnknownDomain { domain })
    }

    /// Performs one ordinary local semantic mutation through current authority.
    ///
    /// # Errors
    ///
    /// Returns an error outside an open stage, for stale/foreign authority, or on arithmetic/counter
    /// overflow. Rejected mutations leave the semantic image unchanged.
    pub fn mutate(
        &mut self,
        token: MutationToken,
        delta: i64,
    ) -> Result<DomainStamp, OwnershipError> {
        self.require_open_stage()?;
        self.require_token(token)?;
        self.apply_delta(token.domain, delta)?;
        self.stamp(token.domain)
    }

    /// Prepares freshness-sensitive work without installing it.
    ///
    /// # Errors
    ///
    /// Returns an error outside an open stage, for stale authority, or when an ID is reused.
    pub fn prepare_deferred(
        &mut self,
        token: MutationToken,
        id: DeferredId,
        delta: i64,
        freshness: DeferredFreshness,
    ) -> Result<DeferredWork, OwnershipError> {
        self.require_open_stage()?;
        self.require_token(token)?;
        if !self.seen_deferred.insert(id) {
            return Err(OwnershipError::DuplicateDeferredPreparation { id });
        }
        Ok(DeferredWork {
            id,
            domain: token.domain,
            observed: self.stamp(token.domain)?,
            delta,
            freshness,
        })
    }

    /// Installs prepared work only through current authority and declared freshness.
    ///
    /// # Errors
    ///
    /// Returns an error outside an open stage, for stale/foreign authority, duplicate installation,
    /// stale work, or arithmetic/counter overflow. Rejected installation leaves state unchanged.
    pub fn install_deferred(
        &mut self,
        token: MutationToken,
        work: DeferredWork,
    ) -> Result<DomainStamp, OwnershipError> {
        self.require_open_stage()?;
        self.require_token(token)?;
        if token.domain != work.domain {
            return Err(OwnershipError::NotCurrentAuthority {
                domain: work.domain,
            });
        }
        if self.installed_deferred.contains(&work.id) {
            return Err(OwnershipError::DuplicateDeferredInstall { id: work.id });
        }
        let current = self.stamp(work.domain)?;
        let fresh = match work.freshness {
            DeferredFreshness::ExactStamp => current == work.observed,
            DeferredFreshness::SameGeneration => current.generation == work.observed.generation,
        };
        if !fresh {
            return Err(OwnershipError::StaleDeferred { id: work.id });
        }
        self.apply_delta(work.domain, work.delta)?;
        self.installed_deferred.insert(work.id);
        self.stamp(work.domain)
    }

    /// Emits one typed cross-domain effect for deterministic stage-boundary application.
    ///
    /// Emission does not mutate the target immediately. This is the firewall that prevents worker
    /// interleaving from becoming gameplay order.
    ///
    /// # Errors
    ///
    /// Returns an error outside an open stage, for stale source authority, an unknown/self target,
    /// or a duplicate effect identity.
    pub fn emit_effect(
        &mut self,
        token: MutationToken,
        id: EffectId,
        target: DomainId,
        payload: EffectPayload,
    ) -> Result<(), OwnershipError> {
        self.require_open_stage()?;
        self.require_token(token)?;
        self.domain(target)?;
        if token.domain == target {
            return Err(OwnershipError::SelfTargetedEffect {
                domain: token.domain,
            });
        }
        if !self.seen_effects.insert(id) {
            return Err(OwnershipError::DuplicateEffect { id });
        }
        self.pending_effects.push(CrossDomainEffect {
            id,
            source: token.domain,
            target,
            payload,
        });
        Ok(())
    }

    /// Closes the current stage and applies all foreign effects in canonical semantic order.
    ///
    /// Effects are ordered by `(target, source, effect_id)`, never by worker completion timing.
    /// Application is transactional: an overflow rejects the barrier without partially committing
    /// the effect set.
    ///
    /// # Errors
    ///
    /// Returns an error if no stage is open or any effect would overflow semantic state/counters.
    pub fn finish_stage(&mut self) -> Result<(), OwnershipError> {
        self.require_open_stage()?;

        let mut effects = self.pending_effects.clone();
        effects.sort_by_key(|effect| (effect.target, effect.source, effect.id));

        let mut updates: BTreeMap<DomainId, (i64, DomainRevision)> = self
            .domains
            .iter()
            .map(|(&domain, state)| (domain, (state.value, state.revision)))
            .collect();
        for effect in effects {
            let Some((value, revision)) = updates.get_mut(&effect.target) else {
                return Err(OwnershipError::UnknownDomain {
                    domain: effect.target,
                });
            };
            let next = match effect.payload {
                EffectPayload::Add(delta) => value.checked_add(delta),
                EffectPayload::RaiseToAtLeast(floor) => Some((*value).max(floor)),
            }
            .ok_or(OwnershipError::ValueOverflow {
                domain: effect.target,
            })?;
            if next != *value {
                let next_revision =
                    revision
                        .0
                        .checked_add(1)
                        .ok_or(OwnershipError::CounterOverflow {
                            domain: effect.target,
                        })?;
                *value = next;
                *revision = DomainRevision(next_revision);
            }
        }

        for (domain, (value, revision)) in updates {
            let state = self
                .domains
                .get_mut(&domain)
                .ok_or(OwnershipError::UnknownDomain { domain })?;
            state.value = value;
            state.revision = revision;
        }
        self.pending_effects.clear();
        self.stage_open = false;
        Ok(())
    }

    /// Begins an explicit quiescent migration handoff.
    ///
    /// Between begin and commit the domain has no ordinary mutation token at all.
    ///
    /// # Errors
    ///
    /// Returns an error unless the semantic stage is closed and `token` is current authority.
    pub fn begin_migration(
        &mut self,
        token: MutationToken,
        to: WorkerId,
    ) -> Result<MigrationHandoff, OwnershipError> {
        if self.stage_open {
            return Err(OwnershipError::MigrationRequiresClosedStage);
        }
        self.require_token(token)?;
        let revision = self.domain(token.domain)?.revision;
        let handoff = MigrationHandoff {
            domain: token.domain,
            from: token.worker,
            to,
            generation: token.generation,
            revision,
        };
        let state = self
            .domains
            .get_mut(&token.domain)
            .ok_or(OwnershipError::UnknownDomain {
                domain: token.domain,
            })?;
        state.authority = AuthorityState::Migrating {
            from: token.worker,
            to,
            generation: token.generation,
            revision,
        };
        Ok(handoff)
    }

    /// Commits one exact handoff, advances generation, and resets the generation-local revision.
    ///
    /// # Errors
    ///
    /// Returns an error unless the stage is closed and `handoff` exactly matches the migration in
    /// progress. A mismatched handoff cannot steal authority.
    pub fn commit_migration(
        &mut self,
        handoff: MigrationHandoff,
    ) -> Result<MutationToken, OwnershipError> {
        if self.stage_open {
            return Err(OwnershipError::MigrationRequiresClosedStage);
        }
        let state = self
            .domains
            .get_mut(&handoff.domain)
            .ok_or(OwnershipError::UnknownDomain {
                domain: handoff.domain,
            })?;
        let expected = AuthorityState::Migrating {
            from: handoff.from,
            to: handoff.to,
            generation: handoff.generation,
            revision: handoff.revision,
        };
        if state.authority != expected || state.revision != handoff.revision {
            return Err(OwnershipError::HandoffMismatch {
                domain: handoff.domain,
            });
        }
        let next_generation =
            handoff
                .generation
                .0
                .checked_add(1)
                .ok_or(OwnershipError::CounterOverflow {
                    domain: handoff.domain,
                })?;
        let generation = OwnershipGeneration(next_generation);
        state.authority = AuthorityState::Active {
            owner: handoff.to,
            generation,
        };
        state.revision = DomainRevision::default();
        Ok(MutationToken {
            domain: handoff.domain,
            worker: handoff.to,
            generation,
        })
    }

    /// Opens the next semantic stage after all requested migrations have committed.
    ///
    /// The new stage snapshot is captured once from the post-barrier/post-migration image.
    ///
    /// # Errors
    ///
    /// Returns an error if a stage is already open, a migration remains incomplete, or the stage
    /// counter exhausts.
    pub fn begin_stage(&mut self) -> Result<(), OwnershipError> {
        if self.stage_open {
            return Err(OwnershipError::StageAlreadyOpen);
        }
        for (&domain, state) in &self.domains {
            if matches!(state.authority, AuthorityState::Migrating { .. }) {
                return Err(OwnershipError::MigrationStillOpen { domain });
            }
        }
        self.stage = self
            .stage
            .checked_add(1)
            .ok_or(OwnershipError::StageCounterOverflow)?;
        self.stage_snapshot = self
            .domains
            .iter()
            .map(|(&domain, state)| (domain, state.value))
            .collect();
        self.stage_open = true;
        Ok(())
    }

    /// Returns a topology-independent semantic digest at a closed stage boundary.
    ///
    /// # Errors
    ///
    /// Returns an error while a stage remains open, any migration is incomplete, or the completed
    /// stage count cannot be represented.
    pub fn semantic_digest(&self) -> Result<SemanticDigest, OwnershipError> {
        if self.stage_open {
            return Err(OwnershipError::StageAlreadyOpen);
        }
        let mut domains = Vec::with_capacity(self.domains.len());
        for (&domain, state) in &self.domains {
            let AuthorityState::Active { generation, .. } = state.authority else {
                return Err(OwnershipError::MigrationStillOpen { domain });
            };
            domains.push((domain, state.value, generation, state.revision));
        }
        let completed_stages = self
            .stage
            .checked_add(1)
            .ok_or(OwnershipError::StageCounterOverflow)?;
        Ok(SemanticDigest {
            completed_stages,
            domains,
        })
    }

    fn domain(&self, domain: DomainId) -> Result<&DomainState, OwnershipError> {
        self.domains
            .get(&domain)
            .ok_or(OwnershipError::UnknownDomain { domain })
    }

    fn require_open_stage(&self) -> Result<(), OwnershipError> {
        if self.stage_open {
            Ok(())
        } else {
            Err(OwnershipError::StageClosed)
        }
    }

    fn require_token(&self, token: MutationToken) -> Result<&DomainState, OwnershipError> {
        let state = self.domain(token.domain)?;
        match state.authority {
            AuthorityState::Active { owner, generation }
                if owner == token.worker && generation == token.generation =>
            {
                Ok(state)
            }
            AuthorityState::Active { .. } => Err(OwnershipError::NotCurrentAuthority {
                domain: token.domain,
            }),
            AuthorityState::Migrating { .. } => Err(OwnershipError::DomainMigrating {
                domain: token.domain,
            }),
        }
    }

    fn apply_delta(&mut self, domain: DomainId, delta: i64) -> Result<(), OwnershipError> {
        let state = self
            .domains
            .get_mut(&domain)
            .ok_or(OwnershipError::UnknownDomain { domain })?;
        let next_value = state
            .value
            .checked_add(delta)
            .ok_or(OwnershipError::ValueOverflow { domain })?;
        if next_value == state.value {
            return Ok(());
        }
        let next_revision = state
            .revision
            .0
            .checked_add(1)
            .ok_or(OwnershipError::CounterOverflow { domain })?;
        state.value = next_value;
        state.revision = DomainRevision(next_revision);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_domains() -> OwnershipSimulator {
        OwnershipSimulator::new([
            (DomainId(0), WorkerId(0), 10),
            (DomainId(1), WorkerId(1), 20),
        ])
        .expect("valid simulator")
    }

    #[test]
    fn migration_removes_authority_between_begin_and_commit() {
        let mut sim = two_domains();
        let old = sim.token(DomainId(0)).expect("active token");
        sim.finish_stage().expect("close stage");
        let handoff = sim
            .begin_migration(old, WorkerId(7))
            .expect("begin exact handoff");

        assert_eq!(
            sim.validate_token(old),
            Err(OwnershipError::DomainMigrating {
                domain: DomainId(0)
            })
        );
        assert_eq!(
            sim.token(DomainId(0)),
            Err(OwnershipError::DomainMigrating {
                domain: DomainId(0)
            })
        );

        let new = sim.commit_migration(handoff).expect("commit exact handoff");
        assert_eq!(new.worker(), WorkerId(7));
        assert_eq!(new.generation(), OwnershipGeneration(2));
        assert_eq!(
            sim.validate_token(old),
            Err(OwnershipError::NotCurrentAuthority {
                domain: DomainId(0)
            })
        );
        sim.validate_token(new).expect("new authority is current");
    }

    #[test]
    fn exact_deferred_work_goes_stale_after_intervening_mutation() {
        let mut sim = two_domains();
        let token = sim.token(DomainId(0)).expect("active token");
        let work = sim
            .prepare_deferred(token, DeferredId(1), 100, DeferredFreshness::ExactStamp)
            .expect("prepare work");
        sim.mutate(token, 1).expect("intervening mutation");
        assert_eq!(
            sim.install_deferred(token, work),
            Err(OwnershipError::StaleDeferred { id: DeferredId(1) })
        );
        assert_eq!(sim.value(DomainId(0)), Ok(11));
    }

    #[test]
    fn generation_tolerant_work_survives_revision_change_but_not_migration() {
        let mut sim = two_domains();
        let token = sim.token(DomainId(0)).expect("active token");
        let work = sim
            .prepare_deferred(token, DeferredId(2), 5, DeferredFreshness::SameGeneration)
            .expect("prepare work");
        sim.mutate(token, 1).expect("same-generation mutation");
        sim.install_deferred(token, work)
            .expect("same-generation work stays admissible");
        assert_eq!(sim.value(DomainId(0)), Ok(16));

        let stale = sim
            .prepare_deferred(token, DeferredId(3), 7, DeferredFreshness::SameGeneration)
            .expect("prepare pre-migration work");
        sim.finish_stage().expect("close stage");
        let handoff = sim
            .begin_migration(token, WorkerId(9))
            .expect("begin handoff");
        let new = sim.commit_migration(handoff).expect("commit handoff");
        sim.begin_stage().expect("open next stage");
        assert_eq!(
            sim.install_deferred(new, stale),
            Err(OwnershipError::StaleDeferred { id: DeferredId(3) })
        );
    }

    #[test]
    fn stage_reads_are_stable_and_foreign_effects_wait_for_barrier() {
        let mut sim = two_domains();
        let source = sim.token(DomainId(0)).expect("source token");
        assert_eq!(sim.stage_value(DomainId(1)), Ok(20));
        let target = sim.token(DomainId(1)).expect("target token");
        sim.mutate(target, 50).expect("target local mutation");
        assert_eq!(sim.value(DomainId(1)), Ok(70));
        assert_eq!(sim.stage_value(DomainId(1)), Ok(20));

        sim.emit_effect(source, EffectId(4), DomainId(1), EffectPayload::Add(3))
            .expect("emit staged effect");
        assert_eq!(sim.value(DomainId(1)), Ok(70));
        sim.finish_stage().expect("apply stage effects");
        assert_eq!(sim.value(DomainId(1)), Ok(73));
    }

    #[test]
    fn duplicate_effect_and_self_target_fail_closed() {
        let mut sim = two_domains();
        let source = sim.token(DomainId(0)).expect("source token");
        sim.emit_effect(source, EffectId(9), DomainId(1), EffectPayload::Add(1))
            .expect("first effect");
        assert_eq!(
            sim.emit_effect(source, EffectId(9), DomainId(1), EffectPayload::Add(1),),
            Err(OwnershipError::DuplicateEffect { id: EffectId(9) })
        );
        assert_eq!(
            sim.emit_effect(source, EffectId(10), DomainId(0), EffectPayload::Add(1),),
            Err(OwnershipError::SelfTargetedEffect {
                domain: DomainId(0)
            })
        );
    }
}
