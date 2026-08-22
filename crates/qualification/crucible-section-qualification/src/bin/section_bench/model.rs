use std::mem;

use crucible_generated::{BlockStateId, GeneratedStateFacts};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, FastLocalRepresentation,
    PackedLocalBlockSection, PackedLocalRepresentation, RepresentationKind,
};

pub(crate) const HARNESS_SCHEMA: u32 = 2;
pub(crate) const HARNESS_VERSION: &str = "section-bench-v2";
pub(crate) const BENCH_SEED: u64 = 0x6A09_E667_F3BC_C909;
pub(crate) const CARDINALITIES: [usize; 13] =
    [1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096];
pub(crate) const PROMOTION_TARGETS: [usize; 9] = [2, 3, 5, 9, 17, 33, 65, 129, 257];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    Smoke,
    Qualification,
}

impl Mode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Qualification => "qualification",
        }
    }

    pub(crate) const fn settings(self) -> Settings {
        match self {
            Self::Smoke => Settings {
                warmup_samples: 1,
                measured_samples: 3,
                random_reads: 2_048,
                full_scans: 2,
                volume_queries: 16,
                mutations: 2_048,
                contains_queries: 32,
                promotion_samples: 8,
                lifetime_mutations: 2_048,
            },
            Self::Qualification => Settings {
                warmup_samples: 5,
                measured_samples: 25,
                random_reads: 65_536,
                full_scans: 64,
                volume_queries: 512,
                mutations: 32_768,
                contains_queries: 1_024,
                promotion_samples: 1_000,
                lifetime_mutations: 32_768,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    pub(crate) warmup_samples: usize,
    pub(crate) measured_samples: usize,
    pub(crate) random_reads: usize,
    pub(crate) full_scans: usize,
    pub(crate) volume_queries: usize,
    pub(crate) mutations: usize,
    pub(crate) contains_queries: usize,
    pub(crate) promotion_samples: usize,
    pub(crate) lifetime_mutations: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Pattern {
    CardinalitySpread,
    Homogeneous,
    Layered,
    Clustered,
    Checker,
    Noisy,
    FluidContaining,
    SurvivalLike,
    BuildLike,
}

impl Pattern {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CardinalitySpread => "cardinality-spread",
            Self::Homogeneous => "homogeneous",
            Self::Layered => "layered",
            Self::Clustered => "clustered",
            Self::Checker => "checker",
            Self::Noisy => "noisy",
            Self::FluidContaining => "fluid-containing",
            Self::SurvivalLike => "survival-like",
            Self::BuildLike => "build-like",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaseSpec {
    pub(crate) pattern: Pattern,
    pub(crate) pool_cardinality: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RepresentationCode {
    DirectReference,
    DirectN,
    Uniform,
    Local4Stable,
    Local8Stable,
    Packed(u8),
}

impl RepresentationCode {
    pub(crate) fn name(self) -> String {
        match self {
            Self::DirectReference => "direct-reference".to_owned(),
            Self::DirectN => "direct-n".to_owned(),
            Self::Uniform => "uniform".to_owned(),
            Self::Local4Stable => "local4-stable".to_owned(),
            Self::Local8Stable => "local8-stable".to_owned(),
            Self::Packed(bits) => format!("packed-{bits}"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SampleSummary {
    pub(crate) samples_ns: Vec<u128>,
    pub(crate) operations_per_sample: usize,
    pub(crate) p50_ns: u128,
    pub(crate) p95_ns: u128,
    pub(crate) p99_ns: u128,
    pub(crate) max_ns: u128,
}

impl SampleSummary {
    pub(crate) fn p50_ps_per_op(&self) -> u128 {
        let operations =
            u128::try_from(self.operations_per_sample).expect("usize operation count fits u128");
        self.p50_ns.saturating_mul(1_000) / operations
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TimingRecord {
    pub(crate) candidate: &'static str,
    pub(crate) production_candidate: bool,
    pub(crate) workload: String,
    pub(crate) pattern: &'static str,
    pub(crate) pool_cardinality: usize,
    pub(crate) actual_cardinality: usize,
    pub(crate) representation: String,
    pub(crate) unit: &'static str,
    pub(crate) timing: SampleSummary,
}

#[derive(Clone, Debug)]
pub(crate) struct MemoryRecord {
    pub(crate) candidate: &'static str,
    pub(crate) production_candidate: bool,
    pub(crate) pattern: &'static str,
    pub(crate) pool_cardinality: usize,
    pub(crate) actual_cardinality: usize,
    pub(crate) representation: String,
    pub(crate) owned_bytes: usize,
    pub(crate) construction_logical_allocations: usize,
    pub(crate) construction_transitions: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct LifetimeRecord {
    pub(crate) candidate: &'static str,
    pub(crate) pattern: &'static str,
    pub(crate) pool_cardinality: usize,
    pub(crate) mutation_count: usize,
    pub(crate) representation_transitions: usize,
    pub(crate) logical_backing_allocations: usize,
    pub(crate) peak_owned_bytes: usize,
    pub(crate) final_owned_bytes: usize,
    pub(crate) final_representation: String,
}

pub(crate) trait BenchSection: BlockSection<BlockStateId> + Clone {
    const NAME: &'static str;
    const PRODUCTION_CANDIDATE: bool;

    fn filled(state: BlockStateId) -> Self;
    fn owned_bytes(&self) -> usize;
    fn representation_code(&self) -> RepresentationCode;
    fn initial_logical_allocations() -> usize;
    fn transition_logical_allocations(
        before: RepresentationCode,
        after: RepresentationCode,
    ) -> usize;

    fn representation_name(&self) -> String {
        self.representation_code().name()
    }
}

impl BenchSection for DirectBlockSection<BlockStateId> {
    const NAME: &'static str = "direct-reference";
    const PRODUCTION_CANDIDATE: bool = false;

    fn filled(state: BlockStateId) -> Self {
        Self::filled(state, &GeneratedStateFacts)
    }

    fn owned_bytes(&self) -> usize {
        mem::size_of::<Self>() + BLOCK_SECTION_CELLS * mem::size_of::<BlockStateId>()
    }

    fn representation_code(&self) -> RepresentationCode {
        RepresentationCode::DirectReference
    }

    fn initial_logical_allocations() -> usize {
        1
    }

    fn transition_logical_allocations(
        _before: RepresentationCode,
        _after: RepresentationCode,
    ) -> usize {
        0
    }
}

impl BenchSection for DirectNBlockSection<BlockStateId> {
    const NAME: &'static str = "direct";
    const PRODUCTION_CANDIDATE: bool = true;

    fn filled(state: BlockStateId) -> Self {
        Self::filled(state, &GeneratedStateFacts)
    }

    fn owned_bytes(&self) -> usize {
        self.owned_bytes()
    }

    fn representation_code(&self) -> RepresentationCode {
        RepresentationCode::DirectN
    }

    fn initial_logical_allocations() -> usize {
        1
    }

    fn transition_logical_allocations(
        _before: RepresentationCode,
        _after: RepresentationCode,
    ) -> usize {
        0
    }
}

impl BenchSection for AdaptiveBlockSection<BlockStateId> {
    const NAME: &'static str = "adaptive";
    const PRODUCTION_CANDIDATE: bool = true;

    fn filled(state: BlockStateId) -> Self {
        Self::filled(state, &GeneratedStateFacts)
    }

    fn owned_bytes(&self) -> usize {
        self.owned_bytes()
    }

    fn representation_code(&self) -> RepresentationCode {
        match self.representation() {
            RepresentationKind::Uniform => RepresentationCode::Uniform,
            RepresentationKind::Local4Stable => RepresentationCode::Local4Stable,
            RepresentationKind::Local8Stable => RepresentationCode::Local8Stable,
            RepresentationKind::DirectN => RepresentationCode::DirectN,
        }
    }

    fn initial_logical_allocations() -> usize {
        0
    }

    fn transition_logical_allocations(
        before: RepresentationCode,
        after: RepresentationCode,
    ) -> usize {
        match (before, after) {
            (RepresentationCode::Uniform, RepresentationCode::Local4Stable)
            | (RepresentationCode::Local4Stable, RepresentationCode::Local8Stable) => 2,
            (RepresentationCode::Local8Stable, RepresentationCode::DirectN) => 1,
            _ => 0,
        }
    }
}

impl BenchSection for FastLocalBlockSection<BlockStateId> {
    const NAME: &'static str = "fast-local";
    const PRODUCTION_CANDIDATE: bool = true;

    fn filled(state: BlockStateId) -> Self {
        Self::filled(state, &GeneratedStateFacts)
    }

    fn owned_bytes(&self) -> usize {
        self.owned_bytes()
    }

    fn representation_code(&self) -> RepresentationCode {
        match self.representation() {
            FastLocalRepresentation::Uniform => RepresentationCode::Uniform,
            FastLocalRepresentation::Local8Stable => RepresentationCode::Local8Stable,
            FastLocalRepresentation::DirectN => RepresentationCode::DirectN,
        }
    }

    fn initial_logical_allocations() -> usize {
        0
    }

    fn transition_logical_allocations(
        before: RepresentationCode,
        after: RepresentationCode,
    ) -> usize {
        match (before, after) {
            (RepresentationCode::Uniform, RepresentationCode::Local8Stable) => 2,
            (RepresentationCode::Local8Stable, RepresentationCode::DirectN) => 1,
            _ => 0,
        }
    }
}

impl BenchSection for PackedLocalBlockSection<BlockStateId> {
    const NAME: &'static str = "packed-local";
    const PRODUCTION_CANDIDATE: bool = true;

    fn filled(state: BlockStateId) -> Self {
        Self::filled(state, &GeneratedStateFacts)
    }

    fn owned_bytes(&self) -> usize {
        self.owned_bytes()
    }

    fn representation_code(&self) -> RepresentationCode {
        match self.representation() {
            PackedLocalRepresentation::Uniform => RepresentationCode::Uniform,
            PackedLocalRepresentation::Packed(bits) => RepresentationCode::Packed(bits),
            PackedLocalRepresentation::DirectN => RepresentationCode::DirectN,
        }
    }

    fn initial_logical_allocations() -> usize {
        0
    }

    fn transition_logical_allocations(
        before: RepresentationCode,
        after: RepresentationCode,
    ) -> usize {
        match (before, after) {
            (RepresentationCode::Uniform, RepresentationCode::Packed(_)) => 2,
            (RepresentationCode::Packed(before_bits), RepresentationCode::Packed(after_bits))
                if before_bits != after_bits =>
            {
                2
            }
            (RepresentationCode::Packed(_), RepresentationCode::DirectN) => 1,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BenchSection, RepresentationCode, SampleSummary};
    use crucible_generated::{AIR, GeneratedStateFacts};
    use crucible_world_contract::{BlockSection, SectionBlockPos};
    use crucible_world_section::PackedLocalBlockSection;

    #[test]
    fn integer_normalization_never_needs_float_precision() {
        let summary = SampleSummary {
            samples_ns: vec![101],
            operations_per_sample: 4,
            p50_ns: 101,
            p95_ns: 101,
            p99_ns: 101,
            max_ns: 101,
        };
        assert_eq!(summary.p50_ps_per_op(), 25_250);
    }

    #[test]
    fn packed_width_transition_has_two_logical_backing_allocations() {
        assert_eq!(
            <PackedLocalBlockSection<_> as BenchSection>::transition_logical_allocations(
                RepresentationCode::Packed(1),
                RepresentationCode::Packed(2),
            ),
            2
        );
    }

    #[test]
    fn packed_to_direct_has_one_logical_backing_allocation() {
        assert_eq!(
            <PackedLocalBlockSection<_> as BenchSection>::transition_logical_allocations(
                RepresentationCode::Packed(8),
                RepresentationCode::DirectN,
            ),
            1
        );
    }

    #[test]
    fn representation_code_names_are_stable() {
        assert_eq!(RepresentationCode::Uniform.name(), "uniform");
        assert_eq!(RepresentationCode::Local4Stable.name(), "local4-stable");
        assert_eq!(RepresentationCode::Local8Stable.name(), "local8-stable");
        assert_eq!(RepresentationCode::Packed(7).name(), "packed-7");
        assert_eq!(RepresentationCode::DirectN.name(), "direct-n");
    }

    #[test]
    fn packed_first_widen_is_observable_in_benchmark_build_shape() {
        let mut section = PackedLocalBlockSection::filled(AIR, &GeneratedStateFacts);
        let first = crucible_generated::BlockStateId::new(1).expect("state 1 exists");
        let second = crucible_generated::BlockStateId::new(2).expect("state 2 exists");
        let first_pos = SectionBlockPos::new(0, 0, 0).expect("valid position");
        let second_pos = SectionBlockPos::new(1, 0, 0).expect("valid position");
        let _ = section.replace(first_pos, first, &GeneratedStateFacts);
        let _ = section.replace(second_pos, second, &GeneratedStateFacts);
        assert_eq!(section.get(second_pos), second);
    }
}
