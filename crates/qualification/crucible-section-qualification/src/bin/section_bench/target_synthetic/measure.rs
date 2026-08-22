use std::hint::black_box;
use std::time::Instant;

use crucible_generated::{BlockStateId, GeneratedStateFacts};
use crucible_world_contract::BLOCK_SECTION_CELLS;

use crate::model::{
    BenchSection, CaseSpec, PROMOTION_TARGETS, RepresentationCode, SampleSummary, TimingRecord,
};
use crate::workloads::{make_positions, make_state_stream, pos, prepare, state_id};

use super::{CONTROL_WORKLOAD, TargetSyntheticSettings};

const POSITION_SEED: u64 = 0x5359_4E54_4850_4F53;
const HIGH_STATE_SEED: u64 = 0x5359_4E54_4849_4748;
const LOW_STATE_SEED: u64 = 0x5359_4E54_4C4F_5721;

#[derive(Debug)]
pub(super) struct TargetSyntheticOutput {
    pub(super) timings: Vec<TimingRecord>,
    pub(super) control: SampleSummary,
}

pub(super) fn run<C: BenchSection>(
    cases: &[CaseSpec],
    settings: TargetSyntheticSettings,
) -> Result<TargetSyntheticOutput, String> {
    let mut timings = Vec::new();
    for &case in cases {
        measure_replacements::<C>(case, settings, &mut timings)?;
    }
    for target in PROMOTION_TARGETS {
        timings.push(measure_promotion::<C>(target, settings)?);
    }
    let control = measure_control(settings);
    Ok(TargetSyntheticOutput { timings, control })
}

fn measure_replacements<C: BenchSection>(
    case: CaseSpec,
    settings: TargetSyntheticSettings,
    timings: &mut Vec<TimingRecord>,
) -> Result<(), String> {
    let prepared = prepare::<C>(case);
    let operations = settings.benchmark.mutations;
    let positions = make_positions(operations, POSITION_SEED);
    let high_states = make_state_stream(&prepared.states, operations, HIGH_STATE_SEED);
    let low_count = prepared.states.len().min(4);
    let low_states = make_state_stream(&prepared.states[..low_count], operations, LOW_STATE_SEED);
    let same_states = positions
        .iter()
        .map(|position| prepared.section.get(*position))
        .collect::<Vec<_>>();
    let churn_states = (0..operations)
        .map(|operation| state_id(5_000 + operation % 512))
        .collect::<Vec<_>>();

    for (workload, states) in [
        ("same-state-replace", same_states.as_slice()),
        ("low-entropy-replace", low_states.as_slice()),
        ("high-entropy-replace", high_states.as_slice()),
        ("palette-churn", churn_states.as_slice()),
    ] {
        let (timing, final_representation) =
            measure_replace_checked(&prepared.section, &positions, states, settings, workload)?;
        timings.push(TimingRecord {
            candidate: C::NAME,
            production_candidate: C::PRODUCTION_CANDIDATE,
            workload: workload.to_owned(),
            pattern: case.pattern.as_str(),
            pool_cardinality: case.pool_cardinality,
            actual_cardinality: prepared.actual_cardinality,
            representation: format!(
                "{}->{}",
                prepared.section.representation_name(),
                final_representation.name()
            ),
            unit: "replace",
            timing,
        });
    }
    Ok(())
}

fn measure_replace_checked<C: BenchSection>(
    base: &C,
    positions: &[crucible_world_contract::SectionBlockPos],
    states: &[BlockStateId],
    settings: TargetSyntheticSettings,
    workload: &str,
) -> Result<(SampleSummary, RepresentationCode), String> {
    if positions.len() != states.len() || positions.is_empty() {
        return Err(format!("{workload}: replacement plan shape is invalid"));
    }

    let expected = expected_after(base, positions, states);
    let mut preflight = base.clone();
    apply_replacements(&mut preflight, positions, states);
    verify_image(&preflight, &expected, workload)?;
    let final_representation = preflight.representation_code();

    for _ in 0..settings.benchmark.warmup_samples {
        let mut section = base.clone();
        apply_replacements(&mut section, positions, states);
        black_box(section);
    }

    let mut samples = Vec::with_capacity(settings.benchmark.measured_samples);
    for _ in 0..settings.benchmark.measured_samples {
        let mut section = base.clone();
        let start = Instant::now();
        apply_replacements(&mut section, positions, states);
        samples.push(start.elapsed().as_nanos());
        black_box(section);
    }
    Ok((summarize(samples, positions.len()), final_representation))
}

fn apply_replacements<C: BenchSection>(
    section: &mut C,
    positions: &[crucible_world_contract::SectionBlockPos],
    states: &[BlockStateId],
) {
    for (&position, &state) in positions.iter().zip(states) {
        black_box(section.replace(position, state, &GeneratedStateFacts));
    }
}

fn expected_after<C: BenchSection>(
    base: &C,
    positions: &[crucible_world_contract::SectionBlockPos],
    states: &[BlockStateId],
) -> Vec<BlockStateId> {
    let mut expected = (0..BLOCK_SECTION_CELLS)
        .map(|cell| base.get(pos(cell)))
        .collect::<Vec<_>>();
    for (&position, &state) in positions.iter().zip(states) {
        expected[position.index()] = state;
    }
    expected
}

fn verify_image<C: BenchSection>(
    section: &C,
    expected: &[BlockStateId],
    label: &str,
) -> Result<(), String> {
    if expected.len() != BLOCK_SECTION_CELLS {
        return Err(format!("{label}: expected image has the wrong cell count"));
    }
    for (cell, &state) in expected.iter().enumerate() {
        let actual = section.get(pos(cell));
        if actual != state {
            return Err(format!(
                "{label}: semantic preflight mismatch at cell {cell}: expected {}, got {}",
                state.as_usize(),
                actual.as_usize()
            ));
        }
    }
    Ok(())
}

fn measure_promotion<C: BenchSection>(
    target_cardinality: usize,
    settings: TargetSyntheticSettings,
) -> Result<TimingRecord, String> {
    let before_cardinality = target_cardinality
        .checked_sub(1)
        .ok_or_else(|| "promotion target must be positive".to_owned())?;
    let states = (1..=target_cardinality).map(state_id).collect::<Vec<_>>();
    let mut base = C::filled(states[0]);
    for (state_index, state) in states
        .iter()
        .copied()
        .enumerate()
        .take(before_cardinality)
        .skip(1)
    {
        let _ = base.replace(pos(state_index - 1), state, &GeneratedStateFacts);
    }
    let target = states[target_cardinality - 1];
    let target_position = pos(BLOCK_SECTION_CELLS - 1);
    let representation_before = base.representation_code();

    let mut expected = (0..BLOCK_SECTION_CELLS)
        .map(|cell| base.get(pos(cell)))
        .collect::<Vec<_>>();
    expected[target_position.index()] = target;
    let mut preflight = base.clone();
    let previous = preflight.replace(target_position, target, &GeneratedStateFacts);
    if previous == target {
        return Err(format!(
            "promotion-to-{target_cardinality}: target state was already present at promotion cell"
        ));
    }
    verify_image(&preflight, &expected, "promotion preflight")?;
    let representation_after = preflight.representation_code();

    for _ in 0..settings.benchmark.warmup_samples {
        let mut section = base.clone();
        black_box(section.replace(target_position, target, &GeneratedStateFacts));
        black_box(section);
    }
    let mut samples = Vec::with_capacity(settings.benchmark.promotion_samples);
    for _ in 0..settings.benchmark.promotion_samples {
        let mut section = base.clone();
        let start = Instant::now();
        black_box(section.replace(target_position, target, &GeneratedStateFacts));
        samples.push(start.elapsed().as_nanos());
        black_box(section);
    }

    Ok(TimingRecord {
        candidate: C::NAME,
        production_candidate: C::PRODUCTION_CANDIDATE,
        workload: format!("promotion-to-{target_cardinality}"),
        pattern: "promotion-boundary",
        pool_cardinality: target_cardinality,
        actual_cardinality: target_cardinality,
        representation: format!(
            "{}->{}",
            representation_before.name(),
            representation_after.name()
        ),
        unit: "single-replace",
        timing: summarize(samples, 1),
    })
}

fn measure_control(settings: TargetSyntheticSettings) -> SampleSummary {
    let operations = settings.control_operations;
    for _ in 0..settings.benchmark.warmup_samples {
        control_loop(operations);
    }
    let mut samples = Vec::with_capacity(settings.benchmark.measured_samples);
    for _ in 0..settings.benchmark.measured_samples {
        let start = Instant::now();
        control_loop(operations);
        samples.push(start.elapsed().as_nanos());
    }
    summarize(samples, operations)
}

fn control_loop(operations: usize) {
    for index in 0..operations {
        let mut value = u64::try_from(index).expect("benchmark operation index fits u64")
            ^ 0x9E37_79B9_7F4A_7C15;
        value = value.rotate_left(17).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        value ^= value >> 23;
        black_box(value);
    }
}

fn summarize(samples_ns: Vec<u128>, operations_per_sample: usize) -> SampleSummary {
    assert!(operations_per_sample > 0);
    let mut sorted = samples_ns.clone();
    sorted.sort_unstable();
    SampleSummary {
        p50_ns: percentile(&sorted, 50),
        p95_ns: percentile(&sorted, 95),
        p99_ns: percentile(&sorted, 99),
        max_ns: sorted.last().copied().unwrap_or(0),
        samples_ns,
        operations_per_sample,
    }
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

pub(super) fn expected_timing_records(case_count: usize) -> usize {
    case_count * 4 + PROMOTION_TARGETS.len()
}

pub(super) const fn control_workload_name() -> &'static str {
    CONTROL_WORKLOAD
}
