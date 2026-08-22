mod measure;
mod pack;
mod report;

#[cfg(test)]
mod tests;

use std::path::Path;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::hardware;
use crate::model::BenchSection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PopulationMode {
    Smoke,
    Qualification,
}

impl PopulationMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Qualification => "qualification",
        }
    }

    pub(super) const fn settings(self) -> PopulationSettings {
        match self {
            Self::Smoke => PopulationSettings {
                warmup_samples: 1,
                measured_samples: 3,
                random_reads: 4_096,
                sequential_sections: 4,
                volume_queries: 64,
                contains_queries: 128,
                control_operations: 20_000,
            },
            Self::Qualification => PopulationSettings {
                warmup_samples: 5,
                measured_samples: 21,
                random_reads: 262_144,
                sequential_sections: 128,
                volume_queries: 4_096,
                contains_queries: 8_192,
                control_operations: 1_000_000,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PopulationSettings {
    pub(super) warmup_samples: usize,
    pub(super) measured_samples: usize,
    pub(super) random_reads: usize,
    pub(super) sequential_sections: usize,
    pub(super) volume_queries: usize,
    pub(super) contains_queries: usize,
    pub(super) control_operations: usize,
}

#[derive(Clone, Debug)]
pub(super) struct SampleSummary {
    pub(super) samples_ns: Vec<u128>,
    pub(super) operations_per_sample: usize,
    pub(super) p50_ns: u128,
    pub(super) p95_ns: u128,
    pub(super) p99_ns: u128,
    pub(super) max_ns: u128,
}

impl SampleSummary {
    pub(super) fn p50_ps_per_op(&self) -> u128 {
        let operations =
            u128::try_from(self.operations_per_sample).expect("operation count fits u128");
        self.p50_ns.saturating_mul(1_000) / operations
    }
}

#[derive(Clone, Debug)]
pub(super) struct TimingRecord {
    pub(super) workload: &'static str,
    pub(super) unit: &'static str,
    pub(super) timing: SampleSummary,
}

pub(crate) fn run(
    pack_path: &Path,
    candidate: &str,
    mode: PopulationMode,
) -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Err("population benchmarks must be built with --release".to_owned());
    }
    if BLOCK_STATE_COUNT > usize::from(u16::MAX) + 1 {
        return Err("target state universe no longer fits population pack u16 encoding".to_owned());
    }

    match candidate {
        "direct-reference" => run_candidate::<DirectBlockSection<BlockStateId>>(pack_path, mode),
        "direct" => run_candidate::<DirectNBlockSection<BlockStateId>>(pack_path, mode),
        "adaptive" => run_candidate::<AdaptiveBlockSection<BlockStateId>>(pack_path, mode),
        "fast-local" => run_candidate::<FastLocalBlockSection<BlockStateId>>(pack_path, mode),
        "packed-local" => run_candidate::<PackedLocalBlockSection<BlockStateId>>(pack_path, mode),
        other => Err(format!(
            "unknown population benchmark candidate {other:?}; expected direct-reference, direct, adaptive, fast-local or packed-local"
        )),
    }
}

fn run_candidate<C: BenchSection>(
    pack_path: &Path,
    mode: PopulationMode,
) -> Result<String, String> {
    let settings = mode.settings();
    let loaded = pack::load_candidate::<C>(pack_path)?;
    let timings = measure::steady_state_timings(&loaded.sections, loaded.negative_state, settings)?;
    let hardware = hardware::collect()?;
    report::render::<C>(mode, settings, &loaded, &timings, &hardware)
}
