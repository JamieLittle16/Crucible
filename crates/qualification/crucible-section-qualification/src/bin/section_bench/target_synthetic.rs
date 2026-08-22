mod measure;
mod report;

#[cfg(test)]
mod tests;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::hardware;
use crate::model::{BenchSection, Mode, Settings};
use crate::workloads;

pub(crate) const REPORT_SCHEMA: u32 = 1;
pub(crate) const REPORT_VERSION: &str = "section-target-synthetic-bench-v1";
pub(crate) const BUILD_PROFILE: &str = "release";
pub(crate) const CODEGEN_POLICY: &str = "lto=thin,codegen-units=1,panic=abort,strip=debuginfo";
pub(crate) const CONTROL_WORKLOAD: &str = "control-integer-loop";
pub(crate) const REPLACEMENT_WORKLOADS: [&str; 4] = [
    "same-state-replace",
    "low-entropy-replace",
    "high-entropy-replace",
    "palette-churn",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetSyntheticMode {
    Smoke,
    Qualification,
}

impl TargetSyntheticMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Qualification => "qualification",
        }
    }

    pub(crate) const fn settings(self) -> TargetSyntheticSettings {
        match self {
            Self::Smoke => TargetSyntheticSettings {
                benchmark: Mode::Smoke.settings(),
                control_operations: 20_000,
            },
            Self::Qualification => TargetSyntheticSettings {
                benchmark: Mode::Qualification.settings(),
                control_operations: 1_000_000,
            },
        }
    }

    pub(crate) const fn benchmark_mode(self) -> Mode {
        match self {
            Self::Smoke => Mode::Smoke,
            Self::Qualification => Mode::Qualification,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetSyntheticSettings {
    pub(crate) benchmark: Settings,
    pub(crate) control_operations: usize,
}

pub(crate) fn run(candidate: &str, mode: TargetSyntheticMode) -> Result<String, String> {
    if cfg!(debug_assertions) {
        return Err("target synthetic benchmarks must be built with --release".to_owned());
    }
    if BLOCK_STATE_COUNT <= 5_511 {
        return Err("target state universe is too small for synthetic benchmark streams".to_owned());
    }

    match candidate {
        "direct-reference" => run_candidate::<DirectBlockSection<BlockStateId>>(mode),
        "direct" => run_candidate::<DirectNBlockSection<BlockStateId>>(mode),
        "adaptive" => run_candidate::<AdaptiveBlockSection<BlockStateId>>(mode),
        "fast-local" => run_candidate::<FastLocalBlockSection<BlockStateId>>(mode),
        "packed-local" => run_candidate::<PackedLocalBlockSection<BlockStateId>>(mode),
        other => Err(format!(
            "unknown target synthetic candidate {other:?}; expected direct-reference, direct, adaptive, fast-local or packed-local"
        )),
    }
}

fn run_candidate<C: BenchSection>(mode: TargetSyntheticMode) -> Result<String, String> {
    let settings = mode.settings();
    let cases = workloads::cases_for(mode.benchmark_mode());
    let measured = measure::run::<C>(&cases, settings)?;
    let hardware = hardware::collect()?;
    report::render::<C>(mode, settings, &cases, &measured, &hardware)
}
