use std::hint::black_box;
use std::time::Instant;

use crucible_generated::BlockStateId;
use crucible_world_contract::BLOCK_SECTION_CELLS;

use crate::model::BenchSection;
use crate::workloads::pos;

use super::{PopulationSettings, SampleSummary, TimingRecord};

const BENCH_SEED: u64 = 0x243F_6A88_85A3_08D3;

pub(super) fn steady_state_timings<C: BenchSection>(
    sections: &[C],
    negative_state: BlockStateId,
    settings: PopulationSettings,
) -> Result<Vec<TimingRecord>, String> {
    if sections.is_empty() {
        return Err("population benchmark has no loaded sections".to_owned());
    }

    // Query-plan allocation occurs after the candidate RSS snapshot in pack::load_candidate.
    let random_plan = query_plan(settings.random_reads, sections.len(), BENCH_SEED ^ 0x10);
    let volume_plan = query_plan(settings.volume_queries, sections.len(), BENCH_SEED ^ 0x20);
    let contains_plan = query_plan(settings.contains_queries, sections.len(), BENCH_SEED ^ 0x30);
    let scan_count = settings.sequential_sections.min(sections.len());
    let scan_plan = section_plan(scan_count, sections.len(), BENCH_SEED ^ 0x40);
    let positive_states = positive_needles(sections, &contains_plan);
    verify_positive_queries(sections, &contains_plan, &positive_states)?;

    let random = measure(settings, settings.random_reads, |operation| {
        let (section, cell) = random_plan[operation];
        black_box(sections[section].get(pos(cell)));
    });

    let sequential_operations = scan_count
        .checked_mul(BLOCK_SECTION_CELLS)
        .ok_or_else(|| "sequential operation count overflow".to_owned())?;
    let sequential = measure(settings, scan_count, |operation| {
        let section = &sections[scan_plan[operation]];
        for cell in 0..BLOCK_SECTION_CELLS {
            black_box(section.get(pos(cell)));
        }
    });
    let sequential = with_operations(sequential, sequential_operations);

    let volume_operations = settings
        .volume_queries
        .checked_mul(64)
        .ok_or_else(|| "volume operation count overflow".to_owned())?;
    let volume = measure(settings, settings.volume_queries, |operation| {
        let (section_index, cell) = volume_plan[operation];
        read_volume(&sections[section_index], cell);
    });
    let volume = with_operations(volume, volume_operations);

    let positive = measure(settings, settings.contains_queries, |operation| {
        let (section_index, _) = contains_plan[operation];
        let needle = positive_states[operation];
        black_box(sections[section_index].maybe_contains(|state| state == needle));
    });
    let negative = measure(settings, settings.contains_queries, |operation| {
        let (section_index, _) = contains_plan[operation];
        black_box(sections[section_index].maybe_contains(|state| state == negative_state));
    });
    let control = measure_control(settings);

    Ok(vec![
        TimingRecord {
            workload: "random-read",
            unit: "cell-read",
            timing: random,
        },
        TimingRecord {
            workload: "sequential-full-read",
            unit: "cell-read",
            timing: sequential,
        },
        TimingRecord {
            workload: "small-volume-read",
            unit: "cell-read",
            timing: volume,
        },
        TimingRecord {
            workload: "maybe-contains-positive",
            unit: "query",
            timing: positive,
        },
        TimingRecord {
            workload: "maybe-contains-negative",
            unit: "query",
            timing: negative,
        },
        TimingRecord {
            workload: "control-integer-loop",
            unit: "iteration",
            timing: control,
        },
    ])
}

pub(super) fn positive_needles<C: BenchSection>(
    sections: &[C],
    plan: &[(usize, usize)],
) -> Vec<BlockStateId> {
    plan.iter()
        .map(|&(section, cell)| sections[section].get(pos(cell)))
        .collect()
}

fn verify_positive_queries<C: BenchSection>(
    sections: &[C],
    plan: &[(usize, usize)],
    needles: &[BlockStateId],
) -> Result<(), String> {
    if plan.len() != needles.len() {
        return Err("positive membership plan/needle length mismatch".to_owned());
    }
    for (operation, (&(section_index, _), &needle)) in plan.iter().zip(needles.iter()).enumerate() {
        if !sections[section_index].maybe_contains(|state| state == needle) {
            return Err(format!(
                "positive membership preflight produced false at operation {operation}"
            ));
        }
    }
    Ok(())
}

fn measure<F>(settings: PopulationSettings, operations: usize, mut operation: F) -> SampleSummary
where
    F: FnMut(usize),
{
    for _ in 0..settings.warmup_samples {
        for index in 0..operations {
            operation(index);
        }
    }

    let mut samples = Vec::with_capacity(settings.measured_samples);
    for _ in 0..settings.measured_samples {
        let start = Instant::now();
        for index in 0..operations {
            operation(index);
        }
        samples.push(start.elapsed().as_nanos());
    }
    summarize(samples, operations)
}

fn measure_control(settings: PopulationSettings) -> SampleSummary {
    measure(settings, settings.control_operations, |index| {
        let mut value = u64::try_from(index).expect("benchmark operation index fits u64")
            ^ 0x9E37_79B9_7F4A_7C15;
        value = value.rotate_left(17).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        value ^= value >> 23;
        black_box(value);
    })
}

fn with_operations(mut summary: SampleSummary, operations: usize) -> SampleSummary {
    summary.operations_per_sample = operations;
    summary
}

pub(super) fn summarize(samples_ns: Vec<u128>, operations_per_sample: usize) -> SampleSummary {
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

pub(super) fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn query_plan(count: usize, section_count: usize, seed: u64) -> Vec<(usize, usize)> {
    let section_modulus = u64::try_from(section_count).expect("section count fits u64");
    let cell_modulus = u64::try_from(BLOCK_SECTION_CELLS).expect("section cell count fits u64");
    let mut state = seed;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        state = xorshift64(state);
        let section = usize::try_from(state % section_modulus).expect("section index fits usize");
        state = xorshift64(state);
        let cell = usize::try_from(state % cell_modulus).expect("cell index fits usize");
        result.push((section, cell));
    }
    result
}

fn section_plan(count: usize, section_count: usize, seed: u64) -> Vec<usize> {
    query_plan(count, section_count, seed)
        .into_iter()
        .map(|(section, _)| section)
        .collect()
}

fn xorshift64(mut value: u64) -> u64 {
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    value
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
