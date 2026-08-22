use std::hint::black_box;
use std::time::Instant;

use crucible_generated::{BlockStateId, GeneratedStateFacts};
use crucible_world_contract::BLOCK_SECTION_CELLS;
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::{
    BenchSection, CaseSpec, LifetimeRecord, MemoryRecord, PROMOTION_TARGETS, SampleSummary,
    Settings, TimingRecord,
};
use crate::workloads::{
    Prepared, make_positions, make_state_stream, negative_contains_state, pos,
    positive_contains_state, prepare, state_id,
};

#[derive(Debug, Default)]
pub(crate) struct BenchmarkOutput {
    pub(crate) timings: Vec<TimingRecord>,
    pub(crate) memory: Vec<MemoryRecord>,
    pub(crate) lifetimes: Vec<LifetimeRecord>,
}

pub(crate) fn run_all(cases: &[CaseSpec], settings: Settings) -> BenchmarkOutput {
    let mut output = BenchmarkOutput::default();
    run_candidate::<DirectBlockSection<BlockStateId>>(cases, settings, &mut output);
    run_candidate::<DirectNBlockSection<BlockStateId>>(cases, settings, &mut output);
    run_candidate::<AdaptiveBlockSection<BlockStateId>>(cases, settings, &mut output);
    run_candidate::<FastLocalBlockSection<BlockStateId>>(cases, settings, &mut output);
    run_candidate::<PackedLocalBlockSection<BlockStateId>>(cases, settings, &mut output);
    output
}

fn run_candidate<C: BenchSection>(
    cases: &[CaseSpec],
    settings: Settings,
    output: &mut BenchmarkOutput,
) {
    for &case in cases {
        let prepared = prepare::<C>(case);
        output.memory.push(memory_record::<C>(&prepared, case));
        output
            .lifetimes
            .push(lifetime_record::<C>(&prepared, case, settings));
        bench_prepared::<C>(&prepared, case, settings, &mut output.timings);
    }

    for target in PROMOTION_TARGETS {
        output.timings.push(bench_promotion::<C>(target, settings));
    }
}

fn memory_record<C: BenchSection>(prepared: &Prepared<C>, case: CaseSpec) -> MemoryRecord {
    MemoryRecord {
        candidate: C::NAME,
        production_candidate: C::PRODUCTION_CANDIDATE,
        pattern: case.pattern.as_str(),
        pool_cardinality: case.pool_cardinality,
        actual_cardinality: prepared.actual_cardinality,
        representation: prepared.section.representation_name(),
        owned_bytes: prepared.section.owned_bytes(),
        construction_logical_allocations: prepared.construction_logical_allocations,
        construction_transitions: prepared.construction_transitions,
    }
}

fn lifetime_record<C: BenchSection>(
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
) -> LifetimeRecord {
    let mut section = prepared.section.clone();
    let positions = make_positions(settings.lifetime_mutations, 0x11FE_71A0);
    let states = (0..512)
        .map(|index| state_id(5_000 + index))
        .collect::<Vec<_>>();
    let stream = make_state_stream(&states, settings.lifetime_mutations, 0x11FE_71A1);
    let mut transitions = prepared.construction_transitions;
    let mut allocations = prepared.construction_logical_allocations;
    let mut peak_owned_bytes = prepared.peak_owned_bytes;

    for operation in 0..settings.lifetime_mutations {
        let before = section.representation_name();
        let _ = section.replace(
            positions[operation],
            stream[operation],
            &GeneratedStateFacts,
        );
        let after = section.representation_name();
        if before != after {
            transitions += 1;
            allocations += C::transition_logical_allocations(&before, &after);
        }
        peak_owned_bytes = peak_owned_bytes.max(section.owned_bytes());
    }

    LifetimeRecord {
        candidate: C::NAME,
        pattern: case.pattern.as_str(),
        pool_cardinality: case.pool_cardinality,
        mutation_count: settings.lifetime_mutations,
        representation_transitions: transitions,
        logical_backing_allocations: allocations,
        peak_owned_bytes,
        final_owned_bytes: section.owned_bytes(),
        final_representation: section.representation_name(),
    }
}

fn bench_prepared<C: BenchSection>(
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
    timings: &mut Vec<TimingRecord>,
) {
    let positions = make_positions(settings.random_reads.max(settings.mutations), 0xA11C_E001);
    let mutation_positions = make_positions(settings.mutations, 0xA11C_E002);
    bench_reads::<C>(prepared, case, settings, &positions, timings);
    bench_replacements::<C>(prepared, case, settings, &mutation_positions, timings);
    bench_contains::<C>(prepared, case, settings, timings);
}

fn bench_reads<C: BenchSection>(
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
    positions: &[crucible_world_contract::SectionBlockPos],
    timings: &mut Vec<TimingRecord>,
) {
    push_timing::<C>(
        timings,
        prepared,
        case,
        "random-read",
        "cell-read",
        measure_immutable(
            settings,
            settings.random_reads,
            &prepared.section,
            |section, operation| {
                black_box(section.get(positions[operation % positions.len()]));
            },
        ),
    );

    push_timing::<C>(
        timings,
        prepared,
        case,
        "sequential-full-read",
        "section-scan",
        measure_immutable(
            settings,
            settings.full_scans,
            &prepared.section,
            |section, _| {
                for cell in 0..BLOCK_SECTION_CELLS {
                    black_box(section.get(pos(cell)));
                }
            },
        ),
    );

    push_timing::<C>(
        timings,
        prepared,
        case,
        "small-volume-read",
        "4x4x4-volume",
        measure_immutable(
            settings,
            settings.volume_queries,
            &prepared.section,
            |section, operation| {
                let base = positions[operation % positions.len()].index();
                read_volume(section, base);
            },
        ),
    );
}

fn read_volume<C: BenchSection>(section: &C, base: usize) {
    let bx = base & 0x0c;
    let bz = (base >> 4) & 0x0c;
    let by = (base >> 8) & 0x0c;
    for dy in 0..4 {
        for dz in 0..4 {
            for dx in 0..4 {
                let index = (bx + dx) | ((bz + dz) << 4) | ((by + dy) << 8);
                black_box(section.get(pos(index)));
            }
        }
    }
}

fn bench_replacements<C: BenchSection>(
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
    positions: &[crucible_world_contract::SectionBlockPos],
    timings: &mut Vec<TimingRecord>,
) {
    let high_states = make_state_stream(&prepared.states, settings.mutations, 0xA11C_E003);
    let low_count = prepared.states.len().min(4);
    let low_states = make_state_stream(
        &prepared.states[..low_count],
        settings.mutations,
        0xA11C_E004,
    );
    let same_states = positions
        .iter()
        .map(|position| prepared.section.get(*position))
        .collect::<Vec<_>>();

    push_replace_timing::<C>(
        timings,
        prepared,
        case,
        settings,
        positions,
        &same_states,
        "same-state-replace",
    );
    push_replace_timing::<C>(
        timings,
        prepared,
        case,
        settings,
        positions,
        &low_states,
        "low-entropy-replace",
    );
    push_replace_timing::<C>(
        timings,
        prepared,
        case,
        settings,
        positions,
        &high_states,
        "high-entropy-replace",
    );

    let churn_states = (0..settings.mutations)
        .map(|operation| state_id(5_000 + operation % 512))
        .collect::<Vec<_>>();
    push_replace_timing::<C>(
        timings,
        prepared,
        case,
        settings,
        positions,
        &churn_states,
        "palette-churn",
    );
}

fn push_replace_timing<C: BenchSection>(
    timings: &mut Vec<TimingRecord>,
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
    positions: &[crucible_world_contract::SectionBlockPos],
    states: &[BlockStateId],
    workload: &str,
) {
    push_timing::<C>(
        timings,
        prepared,
        case,
        workload,
        "replace",
        measure_mutating(
            settings,
            settings.mutations,
            &prepared.section,
            |section, operation| {
                let index = operation % positions.len();
                black_box(section.replace(positions[index], states[index], &GeneratedStateFacts));
            },
        ),
    );
}

fn bench_contains<C: BenchSection>(
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
    timings: &mut Vec<TimingRecord>,
) {
    let positive = positive_contains_state(prepared);
    push_timing::<C>(
        timings,
        prepared,
        case,
        "maybe-contains-positive",
        "query",
        measure_immutable(
            settings,
            settings.contains_queries,
            &prepared.section,
            |section, _| {
                black_box(section.maybe_contains(|state| state == positive));
            },
        ),
    );

    let negative = negative_contains_state(prepared);
    push_timing::<C>(
        timings,
        prepared,
        case,
        "maybe-contains-negative",
        "query",
        measure_immutable(
            settings,
            settings.contains_queries,
            &prepared.section,
            |section, _| {
                black_box(section.maybe_contains(|state| state == negative));
            },
        ),
    );
}

fn bench_promotion<C: BenchSection>(target_cardinality: usize, settings: Settings) -> TimingRecord {
    let before_cardinality = target_cardinality - 1;
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
    let representation_before = base.representation_name();
    let target = states[target_cardinality - 1];
    let mut after = base.clone();
    let _ = after.replace(pos(BLOCK_SECTION_CELLS - 1), target, &GeneratedStateFacts);
    let representation = format!("{representation_before}->{}", after.representation_name());
    let timing = measure_promotion(settings, &base, target);

    TimingRecord {
        candidate: C::NAME,
        production_candidate: C::PRODUCTION_CANDIDATE,
        workload: format!("promotion-to-{target_cardinality}"),
        pattern: "promotion-boundary",
        pool_cardinality: target_cardinality,
        actual_cardinality: target_cardinality,
        representation,
        unit: "single-replace",
        timing,
    }
}

fn measure_promotion<C: BenchSection>(
    settings: Settings,
    base: &C,
    target: BlockStateId,
) -> SampleSummary {
    for _ in 0..settings.warmup_samples {
        let mut section = base.clone();
        black_box(section.replace(pos(BLOCK_SECTION_CELLS - 1), target, &GeneratedStateFacts));
    }
    let mut samples = Vec::with_capacity(settings.promotion_samples);
    for _ in 0..settings.promotion_samples {
        let mut section = base.clone();
        let start = Instant::now();
        black_box(section.replace(pos(BLOCK_SECTION_CELLS - 1), target, &GeneratedStateFacts));
        samples.push(start.elapsed().as_nanos());
        black_box(section);
    }
    summarize(samples, 1)
}

fn push_timing<C: BenchSection>(
    timings: &mut Vec<TimingRecord>,
    prepared: &Prepared<C>,
    case: CaseSpec,
    workload: &str,
    unit: &'static str,
    timing: SampleSummary,
) {
    timings.push(TimingRecord {
        candidate: C::NAME,
        production_candidate: C::PRODUCTION_CANDIDATE,
        workload: workload.to_owned(),
        pattern: case.pattern.as_str(),
        pool_cardinality: case.pool_cardinality,
        actual_cardinality: prepared.actual_cardinality,
        representation: prepared.section.representation_name(),
        unit,
        timing,
    });
}

fn measure_immutable<C, F>(
    settings: Settings,
    operations: usize,
    section: &C,
    mut operation: F,
) -> SampleSummary
where
    F: FnMut(&C, usize),
{
    for _ in 0..settings.warmup_samples {
        for index in 0..operations {
            operation(section, index);
        }
    }
    let mut samples = Vec::with_capacity(settings.measured_samples);
    for _ in 0..settings.measured_samples {
        let start = Instant::now();
        for index in 0..operations {
            operation(section, index);
        }
        samples.push(start.elapsed().as_nanos());
    }
    summarize(samples, operations)
}

fn measure_mutating<C, F>(
    settings: Settings,
    operations: usize,
    base: &C,
    mut operation: F,
) -> SampleSummary
where
    C: Clone,
    F: FnMut(&mut C, usize),
{
    for _ in 0..settings.warmup_samples {
        let mut section = base.clone();
        for index in 0..operations {
            operation(&mut section, index);
        }
        black_box(section);
    }
    let mut samples = Vec::with_capacity(settings.measured_samples);
    for _ in 0..settings.measured_samples {
        let mut section = base.clone();
        let start = Instant::now();
        for index in 0..operations {
            operation(&mut section, index);
        }
        samples.push(start.elapsed().as_nanos());
        black_box(section);
    }
    summarize(samples, operations)
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

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn percentile_uses_nearest_rank_ceiling() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&values, 50), 30);
        assert_eq!(percentile(&values, 95), 50);
        assert_eq!(percentile(&values, 99), 50);
    }
}
