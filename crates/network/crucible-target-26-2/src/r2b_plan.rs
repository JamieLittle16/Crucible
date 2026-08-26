//! Compact immutable publication-plan representation for replay-free R2B.
//!
//! The plan mixes one connection-owned contiguous dynamic arena with borrowed composition/status
//! bodies. Its only per-connection metadata is a fixed inline locator array plus ten compact stage
//! spans. It owns no packet semantics, queue or socket state.

use crate::r2b_arena::DynamicBootstrapArena;

/// Number of network-owned semantic stages before the explicit R2C world handoff.
pub const NETWORK_STAGE_COUNT: usize = 10;
/// Maximum dynamic bodies: selected clear route plus three optional weather events.
pub const MAX_DYNAMIC_BODIES: usize = 20;
/// Maximum total bodies after shared commands/recipes and optional server data are included.
pub const MAX_PUBLICATION_BODIES: usize = 23;

/// Exact result of one immutable prepared-plan lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedLookup<'a> {
    /// Every network-owned stage is complete.
    Complete,
    /// Current stage exists but has no body at this index.
    StageComplete,
    /// Exact packet body at `(stage, body)`.
    Body(&'a [u8]),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StageSpan {
    start: u8,
    len: u8,
}

/// Borrowed non-arena body classes admitted by R2B.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SharedBody {
    Commands,
    UpdateRecipes,
    ServerData,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BodyLocator {
    #[default]
    Empty,
    Arena(u8),
    Shared(SharedBody),
}

/// Fail-closed inline-plan construction error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanBuildError {
    /// More bodies/stages were supplied than the selected compile-time plan permits.
    IndexOverflow,
}

/// Prepared finite R2B publication plan.
///
/// Dynamic bytes are owned exactly once. Shared bodies remain borrowed and are never copied into the
/// arena merely to homogenize representation.
#[derive(Debug)]
pub struct PreparedR2bPlan<'a> {
    arena: DynamicBootstrapArena<MAX_DYNAMIC_BODIES>,
    commands: &'a [u8],
    update_recipes: &'a [u8],
    server_data: Option<&'a [u8]>,
    stages: [StageSpan; NETWORK_STAGE_COUNT],
    locators: [BodyLocator; MAX_PUBLICATION_BODIES],
    locator_len: u8,
}

impl PreparedR2bPlan<'_> {
    /// O(1) lookup matching the target-neutral staged-publication cursor contract.
    #[must_use]
    pub fn lookup(&self, stage: usize, body: usize) -> PreparedLookup<'_> {
        let Some(span) = self.stages.get(stage).copied() else {
            return PreparedLookup::Complete;
        };
        let Ok(body) = u8::try_from(body) else {
            return PreparedLookup::StageComplete;
        };
        if body >= span.len {
            return PreparedLookup::StageComplete;
        }

        let locator = usize::from(span.start) + usize::from(body);
        match self.locators[locator] {
            BodyLocator::Arena(index) => self
                .arena
                .body(usize::from(index))
                .map_or(PreparedLookup::StageComplete, PreparedLookup::Body),
            BodyLocator::Shared(SharedBody::Commands) => PreparedLookup::Body(self.commands),
            BodyLocator::Shared(SharedBody::UpdateRecipes) => {
                PreparedLookup::Body(self.update_recipes)
            }
            BodyLocator::Shared(SharedBody::ServerData) => self
                .server_data
                .map_or(PreparedLookup::StageComplete, PreparedLookup::Body),
            BodyLocator::Empty => PreparedLookup::StageComplete,
        }
    }

    /// Number of packet bodies across all ten network-owned stages.
    #[must_use]
    pub fn body_count(&self) -> usize {
        usize::from(self.locator_len)
    }

    /// Number of bodies retained in the contiguous dynamic arena.
    #[must_use]
    pub const fn dynamic_body_count(&self) -> usize {
        self.arena.body_count()
    }

    /// Dynamic packet-body bytes retained by the contiguous arena.
    #[must_use]
    pub fn dynamic_body_bytes(&self) -> usize {
        self.arena.body_bytes()
    }

    /// Body count for one semantic stage.
    #[must_use]
    pub fn stage_body_count(&self, stage: usize) -> Option<usize> {
        self.stages.get(stage).map(|span| usize::from(span.len))
    }
}

/// Internal builder used only while preparing one immutable plan.
pub(crate) struct PreparedR2bPlanBuilder<'a> {
    arena: DynamicBootstrapArena<MAX_DYNAMIC_BODIES>,
    commands: &'a [u8],
    update_recipes: &'a [u8],
    server_data: Option<&'a [u8]>,
    stages: [StageSpan; NETWORK_STAGE_COUNT],
    locators: [BodyLocator; MAX_PUBLICATION_BODIES],
    len: usize,
}

impl<'a> PreparedR2bPlanBuilder<'a> {
    pub(crate) fn new(
        arena: DynamicBootstrapArena<MAX_DYNAMIC_BODIES>,
        commands: &'a [u8],
        update_recipes: &'a [u8],
        server_data: Option<&'a [u8]>,
    ) -> Self {
        Self {
            arena,
            commands,
            update_recipes,
            server_data,
            stages: [StageSpan::default(); NETWORK_STAGE_COUNT],
            locators: [BodyLocator::Empty; MAX_PUBLICATION_BODIES],
            len: 0,
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn arena_mut(&mut self) -> &mut DynamicBootstrapArena<MAX_DYNAMIC_BODIES> {
        &mut self.arena
    }

    pub(crate) fn push_arena(&mut self, index: usize) -> Result<(), PlanBuildError> {
        let index = u8::try_from(index).map_err(|_| PlanBuildError::IndexOverflow)?;
        self.push(BodyLocator::Arena(index))
    }

    pub(crate) fn push_shared(&mut self, body: SharedBody) -> Result<(), PlanBuildError> {
        self.push(BodyLocator::Shared(body))
    }

    fn push(&mut self, locator: BodyLocator) -> Result<(), PlanBuildError> {
        let Some(slot) = self.locators.get_mut(self.len) else {
            return Err(PlanBuildError::IndexOverflow);
        };
        *slot = locator;
        self.len += 1;
        Ok(())
    }

    pub(crate) fn finish_stage(
        &mut self,
        stage: usize,
        start: usize,
    ) -> Result<(), PlanBuildError> {
        let Some(slot) = self.stages.get_mut(stage) else {
            return Err(PlanBuildError::IndexOverflow);
        };
        let start_u8 = u8::try_from(start).map_err(|_| PlanBuildError::IndexOverflow)?;
        let len = self
            .len
            .checked_sub(start)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or(PlanBuildError::IndexOverflow)?;
        *slot = StageSpan {
            start: start_u8,
            len,
        };
        Ok(())
    }

    pub(crate) fn finish(self) -> PreparedR2bPlan<'a> {
        PreparedR2bPlan {
            arena: self.arena,
            commands: self.commands,
            update_recipes: self.update_recipes,
            server_data: self.server_data,
            stages: self.stages,
            locators: self.locators,
            locator_len: u8::try_from(self.len).expect("23-body bound fits u8"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::PacketWriter;

    use super::{
        MAX_DYNAMIC_BODIES, MAX_PUBLICATION_BODIES, NETWORK_STAGE_COUNT, PreparedLookup,
        PreparedR2bPlanBuilder, SharedBody,
    };
    use crate::r2b_arena::DynamicBootstrapArena;

    #[test]
    fn inline_plan_mixes_arena_and_shared_bodies_without_copying_shared_bytes() {
        let commands = [0x10, 0xaa];
        let recipes = [0x85, 0x01, 0xbb];
        let status = [0x56, 0xcc];
        let mut builder = PreparedR2bPlanBuilder::new(
            DynamicBootstrapArena::with_capacity(16),
            &commands,
            &recipes,
            Some(&status),
        );
        let mut scratch = PacketWriter::new(8).expect("scratch");

        let start = builder.len();
        scratch.write_bytes(&[0x31, 0x01]).expect("dynamic body");
        let index = builder
            .arena_mut()
            .seal_from(&mut scratch)
            .expect("seal dynamic");
        builder.push_arena(index).expect("index");
        builder.finish_stage(0, start).expect("stage");

        let start = builder.len();
        builder
            .push_shared(SharedBody::UpdateRecipes)
            .expect("recipes");
        builder.push_shared(SharedBody::Commands).expect("commands");
        builder.finish_stage(1, start).expect("stage");

        let start = builder.len();
        builder
            .push_shared(SharedBody::ServerData)
            .expect("server data");
        builder.finish_stage(2, start).expect("stage");

        for stage in 3..NETWORK_STAGE_COUNT {
            let start = builder.len();
            builder.finish_stage(stage, start).expect("empty stage");
        }

        let plan = builder.finish();
        assert_eq!(plan.body_count(), 4);
        assert_eq!(plan.dynamic_body_count(), 1);
        assert_eq!(plan.lookup(0, 0), PreparedLookup::Body(&[0x31, 0x01]));
        assert_eq!(plan.lookup(1, 0), PreparedLookup::Body(&recipes));
        assert_eq!(plan.lookup(1, 1), PreparedLookup::Body(&commands));
        assert_eq!(plan.lookup(2, 0), PreparedLookup::Body(&status));
        assert_eq!(plan.lookup(3, 0), PreparedLookup::StageComplete);
        assert_eq!(
            plan.lookup(NETWORK_STAGE_COUNT, 0),
            PreparedLookup::Complete
        );

        let first = match plan.lookup(1, 1) {
            PreparedLookup::Body(body) => body,
            _ => panic!("commands body"),
        };
        assert_eq!(first.as_ptr(), commands.as_ptr());
    }

    #[test]
    fn compile_time_bounds_cover_selected_maxima() {
        assert_eq!(MAX_DYNAMIC_BODIES, 20);
        assert_eq!(MAX_PUBLICATION_BODIES, 23);
        assert_eq!(NETWORK_STAGE_COUNT, 10);
    }
}
