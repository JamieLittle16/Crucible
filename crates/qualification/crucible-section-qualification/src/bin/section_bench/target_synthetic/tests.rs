use std::collections::BTreeSet;

use crucible_generated::BlockStateId;
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::{PROMOTION_TARGETS, Mode, Settings};
use crate::workloads::{cases_for, CaseSpec, Pattern};

use super::measure;
use super::{REPLACEMENT_WORKLOADS, TargetSyntheticMode, TargetSyntheticSettings};

fn tiny_settings() -> TargetSyntheticSettings {
    TargetSyntheticSettings {
        benchmark: Settings {
            warmup_samples: 0,
            measured_samples: 2,
            random_reads: 1,
            full_scans: 1,
            volume_queries: 1,
            mutations: 16,
            contains_queries: 1,
            promotion_samples: 3,
            lifetime_mutations: 1,
        },
        control_operations: 64,
    }
}

fn tiny_case() -> [CaseSpec; 1] {
    [CaseSpec {
        pattern: Pattern::CardinalitySpread,
        pool_cardinality: 17,
    }]
}

fn assert_candidate_shape<C: crate::model::BenchSection>() {
    let cases = tiny_case();
    let measured = measure::run::<C>(&cases, tiny_settings()).expect("isolated synthetic run");
    assert_eq!(measured.timings.len(), 4 + PROMOTION_TARGETS.len());
    assert_eq!(measured.control.samples_ns.len(), 2);
    assert_eq!(measured.control.operations_per_sample, 64);

    let workloads = measured
        .timings
        .iter()
        .map(|record| record.workload.as_str())
        .collect::<BTreeSet<_>>();
    for workload in REPLACEMENT_WORKLOADS {
        assert!(workloads.contains(workload));
    }
    for target in PROMOTION_TARGETS {
        assert!(workloads.contains(format!("promotion-to-{target}").as_str()));
    }

    for record in &measured.timings {
        if record.workload.starts_with("promotion-to-") {
            assert_eq!(record.timing.samples_ns.len(), 3);
            assert_eq!(record.timing.operations_per_sample, 1);
            assert!(record.timing.p99_ns <= record.timing.max_ns);
        } else {
            assert_eq!(record.timing.samples_ns.len(), 2);
            assert_eq!(record.timing.operations_per_sample, 16);
        }
    }
}

#[test]
fn all_candidates_execute_the_same_isolated_stress_contract() {
    assert_candidate_shape::<DirectBlockSection<BlockStateId>>();
    assert_candidate_shape::<DirectNBlockSection<BlockStateId>>();
    assert_candidate_shape::<AdaptiveBlockSection<BlockStateId>>();
    assert_candidate_shape::<FastLocalBlockSection<BlockStateId>>();
    assert_candidate_shape::<PackedLocalBlockSection<BlockStateId>>();
}

#[test]
fn packed_promotion_to_three_exercises_first_width_widen() {
    let measured = measure::run::<PackedLocalBlockSection<BlockStateId>>(&tiny_case(), tiny_settings())
        .expect("packed synthetic run");
    let record = measured
        .timings
        .iter()
        .find(|record| record.workload == "promotion-to-3")
        .expect("promotion-to-3 exists");
    assert_eq!(record.representation, "packed-1->packed-2");
    assert_eq!(record.actual_cardinality, 3);
    assert_eq!(record.timing.operations_per_sample, 1);
}

#[test]
fn target_synthetic_modes_reuse_frozen_v2_sampling_scale() {
    let smoke = TargetSyntheticMode::Smoke.settings();
    let qualification = TargetSyntheticMode::Qualification.settings();
    assert_eq!(smoke.benchmark.mutations, Mode::Smoke.settings().mutations);
    assert_eq!(smoke.benchmark.promotion_samples, 8);
    assert_eq!(smoke.control_operations, 20_000);
    assert_eq!(qualification.benchmark.mutations, Mode::Qualification.settings().mutations);
    assert_eq!(qualification.benchmark.promotion_samples, 1_000);
    assert_eq!(qualification.control_operations, 1_000_000);
}

#[test]
fn frozen_case_and_promotion_surfaces_are_complete() {
    assert_eq!(cases_for(Mode::Smoke).len(), 10);
    assert_eq!(cases_for(Mode::Qualification).len(), 22);
    assert_eq!(PROMOTION_TARGETS, [2, 3, 5, 9, 17, 33, 65, 129, 257]);
    assert_eq!(
        REPLACEMENT_WORKLOADS,
        [
            "same-state-replace",
            "low-entropy-replace",
            "high-entropy-replace",
            "palette-churn",
        ]
    );
}
