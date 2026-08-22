//! Reproducible section-representation performance laboratory for M0.3D.
//!
//! This binary is intentionally dependency-light and is not linked into the server. Correctness is
//! qualified elsewhere; this harness measures CPU/tail behaviour and deterministic owned bytes for
//! already-qualified candidates.

#![forbid(unsafe_code)]

use std::env;
use std::fmt::{self, Write as _};
use std::fs;
use std::hint::black_box;
use std::mem;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use crucible_generated::{
    BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
    STATE_DATA_INPUT_SHA256,
};
use crucible_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, SectionBlockPos};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, FastLocalRepresentation,
    PackedLocalBlockSection, PackedLocalRepresentation, RepresentationKind,
};

const HARNESS_SCHEMA: u32 = 1;
const HARNESS_VERSION: &str = "section-bench-v1";
const BENCH_SEED: u64 = 0x6A09_E667_F3BC_C909;
const CARDINALITIES: [usize; 13] = [1, 2, 4, 8, 16, 17, 32, 64, 128, 256, 257, 1024, 4096];
const PROMOTION_TARGETS: [usize; 9] = [2, 3, 5, 9, 17, 33, 65, 129, 257];
const NEGATIVE_STATE_RAW: u32 = 32_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Smoke,
    Qualification,
}

impl Mode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Qualification => "qualification",
        }
    }

    const fn settings(self) -> Settings {
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
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Settings {
    warmup_samples: usize,
    measured_samples: usize,
    random_reads: usize,
    full_scans: usize,
    volume_queries: usize,
    mutations: usize,
    contains_queries: usize,
    promotion_samples: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pattern {
    CardinalitySpread,
    Homogeneous,
    Layered,
    Clustered,
    Checker,
    Noisy,
    SurvivalLike,
    BuildLike,
}

impl Pattern {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CardinalitySpread => "cardinality-spread",
            Self::Homogeneous => "homogeneous",
            Self::Layered => "layered",
            Self::Clustered => "clustered",
            Self::Checker => "checker",
            Self::Noisy => "noisy",
            Self::SurvivalLike => "survival-like",
            Self::BuildLike => "build-like",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CaseSpec {
    pattern: Pattern,
    pool_cardinality: usize,
}

#[derive(Clone, Debug)]
struct Prepared<C> {
    section: C,
    actual_cardinality: usize,
    states: Vec<BlockStateId>,
}

#[derive(Clone, Debug)]
struct SampleSummary {
    samples_ns: Vec<u128>,
    operations_per_sample: usize,
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
    max_ns: u128,
}

impl SampleSummary {
    fn from_samples(samples_ns: Vec<u128>, operations_per_sample: usize) -> Self {
        let mut sorted = samples_ns.clone();
        sorted.sort_unstable();
        let p50_ns = percentile(&sorted, 50);
        let p95_ns = percentile(&sorted, 95);
        let p99_ns = percentile(&sorted, 99);
        let max_ns = sorted.last().copied().unwrap_or(0);
        Self {
            samples_ns,
            operations_per_sample,
            p50_ns,
            p95_ns,
            p99_ns,
            max_ns,
        }
    }

    fn p50_ns_per_op(&self) -> f64 {
        self.p50_ns as f64 / self.operations_per_sample as f64
    }
}

#[derive(Clone, Debug)]
struct TimingRecord {
    candidate: &'static str,
    production_candidate: bool,
    workload: String,
    pattern: &'static str,
    pool_cardinality: usize,
    actual_cardinality: usize,
    representation: String,
    unit: &'static str,
    timing: SampleSummary,
}

#[derive(Clone, Debug)]
struct MemoryRecord {
    candidate: &'static str,
    production_candidate: bool,
    pattern: &'static str,
    pool_cardinality: usize,
    actual_cardinality: usize,
    representation: String,
    owned_bytes: usize,
}

trait BenchSection: BlockSection<BlockStateId> + Clone {
    const NAME: &'static str;
    const PRODUCTION_CANDIDATE: bool;

    fn filled(state: BlockStateId) -> Self;
    fn owned_bytes(&self) -> usize;
    fn representation_name(&self) -> String;
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

    fn representation_name(&self) -> String {
        "direct-reference".to_owned()
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

    fn representation_name(&self) -> String {
        "direct-n".to_owned()
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

    fn representation_name(&self) -> String {
        match self.representation() {
            RepresentationKind::Uniform => "uniform".to_owned(),
            RepresentationKind::Local4Stable => "local4-stable".to_owned(),
            RepresentationKind::Local8Stable => "local8-stable".to_owned(),
            RepresentationKind::DirectN => "direct-n".to_owned(),
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

    fn representation_name(&self) -> String {
        match self.representation() {
            FastLocalRepresentation::Uniform => "uniform".to_owned(),
            FastLocalRepresentation::Local8Stable => "local8-stable".to_owned(),
            FastLocalRepresentation::DirectN => "direct-n".to_owned(),
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

    fn representation_name(&self) -> String {
        match self.representation() {
            PackedLocalRepresentation::Uniform => "uniform".to_owned(),
            PackedLocalRepresentation::Packed(bits) => format!("packed-{bits}"),
            PackedLocalRepresentation::DirectN => "direct-n".to_owned(),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (mode, output) = parse_args()?;
    if cfg!(debug_assertions) {
        return Err("section benchmarks must be built with --release".to_owned());
    }
    if BLOCK_STATE_COUNT <= usize::try_from(NEGATIVE_STATE_RAW).expect("constant fits usize") {
        return Err("negative contains sentinel is not inside target state universe".to_owned());
    }

    let settings = mode.settings();
    let cases = cases_for(mode);
    let mut timings = Vec::new();
    let mut memory = Vec::new();

    run_candidate::<DirectBlockSection<BlockStateId>>(&cases, settings, &mut timings, &mut memory);
    run_candidate::<DirectNBlockSection<BlockStateId>>(&cases, settings, &mut timings, &mut memory);
    run_candidate::<AdaptiveBlockSection<BlockStateId>>(
        &cases,
        settings,
        &mut timings,
        &mut memory,
    );
    run_candidate::<FastLocalBlockSection<BlockStateId>>(
        &cases,
        settings,
        &mut timings,
        &mut memory,
    );
    run_candidate::<PackedLocalBlockSection<BlockStateId>>(
        &cases,
        settings,
        &mut timings,
        &mut memory,
    );

    let report = render_report(mode, settings, &timings, &memory)?;
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(&path, &report)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        println!("section benchmark artifact: {}", path.display());
    } else {
        print!("{report}");
    }

    println!(
        "section benchmark: mode={} timing_records={} memory_records={} complete",
        mode.as_str(),
        timings.len(),
        memory.len()
    );
    Ok(())
}

fn parse_args() -> Result<(Mode, Option<PathBuf>), String> {
    let mut mode = Mode::Qualification;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--smoke") => mode = Mode::Smoke,
            Some("--qualification") => mode = Mode::Qualification,
            Some("--output") => {
                let path = args
                    .next()
                    .ok_or_else(|| "--output requires a path".to_owned())?;
                output = Some(PathBuf::from(path));
            }
            Some("--help") => {
                return Err(
                    "usage: section_bench [--smoke|--qualification] [--output PATH]".to_owned(),
                );
            }
            Some(other) => return Err(format!("unknown benchmark option: {other}")),
            None => return Err("benchmark arguments must be valid UTF-8".to_owned()),
        }
    }
    Ok((mode, output))
}

fn cases_for(mode: Mode) -> Vec<CaseSpec> {
    let cardinalities: &[usize] = match mode {
        Mode::Smoke => &[1, 16, 17, 256, 257, 4096],
        Mode::Qualification => &CARDINALITIES,
    };
    let mut cases = cardinalities
        .iter()
        .copied()
        .map(|pool_cardinality| CaseSpec {
            pattern: Pattern::CardinalitySpread,
            pool_cardinality,
        })
        .collect::<Vec<_>>();

    let spatial = match mode {
        Mode::Smoke => &[
            CaseSpec {
                pattern: Pattern::Layered,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::SurvivalLike,
                pool_cardinality: 32,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 257,
            },
        ][..],
        Mode::Qualification => &[
            CaseSpec {
                pattern: Pattern::Homogeneous,
                pool_cardinality: 1,
            },
            CaseSpec {
                pattern: Pattern::Layered,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::Clustered,
                pool_cardinality: 16,
            },
            CaseSpec {
                pattern: Pattern::Checker,
                pool_cardinality: 2,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 257,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 4096,
            },
            CaseSpec {
                pattern: Pattern::SurvivalLike,
                pool_cardinality: 32,
            },
            CaseSpec {
                pattern: Pattern::BuildLike,
                pool_cardinality: 64,
            },
        ][..],
    };
    cases.extend_from_slice(spatial);
    cases
}

fn run_candidate<C: BenchSection>(
    cases: &[CaseSpec],
    settings: Settings,
    timings: &mut Vec<TimingRecord>,
    memory: &mut Vec<MemoryRecord>,
) {
    for case in cases {
        let prepared = prepare::<C>(*case);
        let representation = prepared.section.representation_name();
        memory.push(MemoryRecord {
            candidate: C::NAME,
            production_candidate: C::PRODUCTION_CANDIDATE,
            pattern: case.pattern.as_str(),
            pool_cardinality: case.pool_cardinality,
            actual_cardinality: prepared.actual_cardinality,
            representation: representation.clone(),
            owned_bytes: prepared.section.owned_bytes(),
        });
        bench_prepared(&prepared, *case, settings, representation, timings);
    }

    for target in PROMOTION_TARGETS {
        bench_promotion::<C>(target, settings, timings);
    }
}

fn prepare<C: BenchSection>(case: CaseSpec) -> Prepared<C> {
    assert!((1..=4096).contains(&case.pool_cardinality));
    let states = (0..case.pool_cardinality)
        .map(|index| state_id(index + 1))
        .collect::<Vec<_>>();
    let mut section = C::filled(states[0]);
    let mut seen = vec![false; case.pool_cardinality];
    let mut rng = BENCH_SEED ^ (case.pool_cardinality as u64).rotate_left(17);

    for cell in 0..BLOCK_SECTION_CELLS {
        let state_index = pattern_state_index(case.pattern, cell, case.pool_cardinality, &mut rng);
        seen[state_index] = true;
        let _ = section.replace(pos(cell), states[state_index], &GeneratedStateFacts);
    }

    Prepared {
        section,
        actual_cardinality: seen.into_iter().filter(|value| *value).count(),
        states,
    }
}

fn pattern_state_index(pattern: Pattern, cell: usize, cardinality: usize, rng: &mut u64) -> usize {
    if cardinality == 1 {
        return 0;
    }
    let x = cell & 15;
    let z = (cell >> 4) & 15;
    let y = (cell >> 8) & 15;
    match pattern {
        Pattern::Homogeneous => 0,
        Pattern::CardinalitySpread => cell % cardinality,
        Pattern::Layered => y % cardinality,
        Pattern::Clustered => ((x >> 2) + 4 * (z >> 2) + 16 * (y >> 2)) % cardinality,
        Pattern::Checker => (x + y + z) & 1,
        Pattern::Noisy => bounded(next_rng(rng), cardinality),
        Pattern::SurvivalLike => {
            if cell % 8 != 0 {
                0
            } else {
                1 + bounded(next_rng(rng), cardinality - 1)
            }
        }
        Pattern::BuildLike => {
            let cluster = (x >> 1) + 8 * (z >> 1) + 64 * (y >> 1);
            mix_usize(cluster ^ cell.rotate_left(5)) % cardinality
        }
    }
}

fn bench_prepared<C: BenchSection>(
    prepared: &Prepared<C>,
    case: CaseSpec,
    settings: Settings,
    representation: String,
    timings: &mut Vec<TimingRecord>,
) {
    let random_positions =
        make_positions(settings.random_reads.max(settings.mutations), 0xA11C_E001);
    let mutation_positions = make_positions(settings.mutations, 0xA11C_E002);
    let high_states = make_state_stream(&prepared.states, settings.mutations, 0xA11C_E003);
    let low_count = prepared.states.len().min(4);
    let low_states = make_state_stream(
        &prepared.states[..low_count],
        settings.mutations,
        0xA11C_E004,
    );
    let same_states = mutation_positions
        .iter()
        .map(|position| prepared.section.get(*position))
        .collect::<Vec<_>>();

    push_timing::<C>(
        timings,
        case,
        &representation,
        "random-read",
        "cell-read",
        measure_immutable(
            settings,
            settings.random_reads,
            &prepared.section,
            |section, operation| {
                black_box(section.get(random_positions[operation % random_positions.len()]));
            },
        ),
    );

    push_timing::<C>(
        timings,
        case,
        &representation,
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
        case,
        &representation,
        "small-volume-read",
        "4x4x4-volume",
        measure_immutable(
            settings,
            settings.volume_queries,
            &prepared.section,
            |section, operation| {
                let base = random_positions[operation % random_positions.len()].index();
                let bx = base & 12;
                let bz = (base >> 4) & 12;
                let by = (base >> 8) & 12;
                for dy in 0..4 {
                    for dz in 0..4 {
                        for dx in 0..4 {
                            let index = (bx + dx) | ((bz + dz) << 4) | ((by + dy) << 8);
                            black_box(section.get(pos(index)));
                        }
                    }
                }
            },
        ),
    );

    push_timing::<C>(
        timings,
        case,
        &representation,
        "same-state-replace",
        "replace",
        measure_mutating(
            settings,
            settings.mutations,
            &prepared.section,
            |section, operation| {
                let index = operation % mutation_positions.len();
                black_box(section.replace(
                    mutation_positions[index],
                    same_states[index],
                    &GeneratedStateFacts,
                ));
            },
        ),
    );

    push_timing::<C>(
        timings,
        case,
        &representation,
        "low-entropy-replace",
        "replace",
        measure_mutating(
            settings,
            settings.mutations,
            &prepared.section,
            |section, operation| {
                let index = operation % mutation_positions.len();
                black_box(section.replace(
                    mutation_positions[index],
                    low_states[index],
                    &GeneratedStateFacts,
                ));
            },
        ),
    );

    push_timing::<C>(
        timings,
        case,
        &representation,
        "high-entropy-replace",
        "replace",
        measure_mutating(
            settings,
            settings.mutations,
            &prepared.section,
            |section, operation| {
                let index = operation % mutation_positions.len();
                black_box(section.replace(
                    mutation_positions[index],
                    high_states[index],
                    &GeneratedStateFacts,
                ));
            },
        ),
    );

    push_timing::<C>(
        timings,
        case,
        &representation,
        "palette-churn",
        "replace",
        measure_mutating(
            settings,
            settings.mutations,
            &prepared.section,
            |section, operation| {
                let raw = 5_000 + (operation % 512);
                black_box(section.replace(
                    mutation_positions[operation % mutation_positions.len()],
                    state_id(raw),
                    &GeneratedStateFacts,
                ));
            },
        ),
    );

    let present = prepared.states[prepared.states.len() - 1];
    push_timing::<C>(
        timings,
        case,
        &representation,
        "maybe-contains-positive",
        "query",
        measure_immutable(
            settings,
            settings.contains_queries,
            &prepared.section,
            |section, _| {
                black_box(section.maybe_contains(|state| state == present));
            },
        ),
    );

    let negative = BlockStateId::new(NEGATIVE_STATE_RAW).expect("negative sentinel is valid");
    push_timing::<C>(
        timings,
        case,
        &representation,
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

fn bench_promotion<C: BenchSection>(
    target_cardinality: usize,
    settings: Settings,
    timings: &mut Vec<TimingRecord>,
) {
    let before_cardinality = target_cardinality - 1;
    let states = (0..target_cardinality)
        .map(|index| state_id(index + 1))
        .collect::<Vec<_>>();
    let mut base = C::filled(states[0]);
    for state_index in 1..before_cardinality {
        let _ = base.replace(
            pos(state_index - 1),
            states[state_index],
            &GeneratedStateFacts,
        );
    }
    let representation_before = base.representation_name();
    let mut after = base.clone();
    let _ = after.replace(
        pos(BLOCK_SECTION_CELLS - 1),
        states[target_cardinality - 1],
        &GeneratedStateFacts,
    );
    let representation = format!("{representation_before}->{}", after.representation_name());

    let promotion_settings = Settings {
        warmup_samples: settings.warmup_samples,
        measured_samples: settings.promotion_samples,
        random_reads: 0,
        full_scans: 0,
        volume_queries: 0,
        mutations: 0,
        contains_queries: 0,
        promotion_samples: settings.promotion_samples,
    };
    let target = states[target_cardinality - 1];
    let timing = measure_mutating(promotion_settings, 1, &base, |section, _| {
        black_box(section.replace(pos(BLOCK_SECTION_CELLS - 1), target, &GeneratedStateFacts));
    });

    timings.push(TimingRecord {
        candidate: C::NAME,
        production_candidate: C::PRODUCTION_CANDIDATE,
        workload: format!("promotion-to-{target_cardinality}"),
        pattern: "promotion-boundary",
        pool_cardinality: target_cardinality,
        actual_cardinality: target_cardinality,
        representation,
        unit: "single-replace",
        timing,
    });
}

fn push_timing<C: BenchSection>(
    timings: &mut Vec<TimingRecord>,
    case: CaseSpec,
    representation: &str,
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
        actual_cardinality: case.pool_cardinality,
        representation: representation.to_owned(),
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
    SampleSummary::from_samples(samples, operations)
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
    SampleSummary::from_samples(samples, operations)
}

fn make_positions(count: usize, seed: u64) -> Vec<SectionBlockPos> {
    let mut rng = BENCH_SEED ^ seed;
    (0..count)
        .map(|_| pos(bounded(next_rng(&mut rng), BLOCK_SECTION_CELLS)))
        .collect()
}

fn make_state_stream(states: &[BlockStateId], count: usize, seed: u64) -> Vec<BlockStateId> {
    let mut rng = BENCH_SEED ^ seed;
    (0..count)
        .map(|_| states[bounded(next_rng(&mut rng), states.len())])
        .collect()
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn state_id(index: usize) -> BlockStateId {
    let raw = u32::try_from(index).expect("benchmark state index fits u32");
    BlockStateId::new(raw).expect("benchmark state ID is inside target universe")
}

fn pos(cell: usize) -> SectionBlockPos {
    debug_assert!(cell < BLOCK_SECTION_CELLS);
    let x = u8::try_from(cell & 15).expect("bounded x");
    let z = u8::try_from((cell >> 4) & 15).expect("bounded z");
    let y = u8::try_from((cell >> 8) & 15).expect("bounded y");
    SectionBlockPos::new(x, y, z).expect("bounded section position")
}

fn next_rng(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn bounded(value: u64, bound: usize) -> usize {
    let bound = u64::try_from(bound).expect("benchmark bound fits u64");
    usize::try_from(value % bound).expect("bounded value fits usize")
}

fn mix_usize(value: usize) -> usize {
    let mut value = value as u64;
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^= value >> 31;
    value as usize
}

fn render_report(
    mode: Mode,
    settings: Settings,
    timings: &[TimingRecord],
    memory: &[MemoryRecord],
) -> Result<String, String> {
    let commit_sha = command_output("git", &["rev-parse", "HEAD"])?;
    let rustc = command_output("rustc", &["--version", "--verbose"])?;
    let target = rustc
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or("unknown");
    let cpu_model = cpu_model();
    let kernel = command_output("uname", &["-srmo"]).unwrap_or_else(|_| "unknown".to_owned());
    let governor = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .map_or_else(|_| "unknown".to_owned(), |value| value.trim().to_owned());

    let mut output = String::new();
    writeln!(output, "{{").map_err(fmt_error)?;
    writeln!(output, "  \"schema\": {HARNESS_SCHEMA},").map_err(fmt_error)?;
    writeln!(output, "  \"harness_version\": \"{HARNESS_VERSION}\",").map_err(fmt_error)?;
    writeln!(output, "  \"mode\": \"{}\",", mode.as_str()).map_err(fmt_error)?;
    writeln!(
        output,
        "  \"commit_sha\": \"{}\",",
        json_escape(commit_sha.trim())
    )
    .map_err(fmt_error)?;
    writeln!(output, "  \"minecraft_version\": \"{MINECRAFT_VERSION}\",").map_err(fmt_error)?;
    writeln!(output, "  \"protocol_version\": {PROTOCOL_VERSION},").map_err(fmt_error)?;
    writeln!(output, "  \"data_version\": {DATA_VERSION},").map_err(fmt_error)?;
    writeln!(
        output,
        "  \"state_data_input_sha256\": \"{STATE_DATA_INPUT_SHA256}\","
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "  \"state_data_generation_sha256\": \"{STATE_DATA_GENERATION_SHA256}\","
    )
    .map_err(fmt_error)?;
    writeln!(output, "  \"benchmark_seed\": \"{BENCH_SEED:016x}\",").map_err(fmt_error)?;
    writeln!(output, "  \"build_profile\": \"release\",").map_err(fmt_error)?;
    writeln!(
        output,
        "  \"codegen_policy\": \"lto=thin,codegen-units=1,panic=abort\","
    )
    .map_err(fmt_error)?;
    writeln!(output, "  \"target_triple\": \"{}\",", json_escape(target)).map_err(fmt_error)?;
    writeln!(output, "  \"cpu_model\": \"{}\",", json_escape(&cpu_model)).map_err(fmt_error)?;
    writeln!(output, "  \"kernel\": \"{}\",", json_escape(kernel.trim())).map_err(fmt_error)?;
    writeln!(
        output,
        "  \"cpu0_governor\": \"{}\",",
        json_escape(&governor)
    )
    .map_err(fmt_error)?;
    writeln!(output, "  \"rustc_verbose\": \"{}\",", json_escape(&rustc)).map_err(fmt_error)?;
    writeln!(output, "  \"settings\": {{").map_err(fmt_error)?;
    writeln!(
        output,
        "    \"warmup_samples\": {},",
        settings.warmup_samples
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"measured_samples\": {},",
        settings.measured_samples
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"promotion_samples\": {}",
        settings.promotion_samples
    )
    .map_err(fmt_error)?;
    writeln!(output, "  }},").map_err(fmt_error)?;

    writeln!(output, "  \"memory\": [").map_err(fmt_error)?;
    for (index, record) in memory.iter().enumerate() {
        let suffix = if index + 1 == memory.len() { "" } else { "," };
        writeln!(
            output,
            "    {{\"candidate\":\"{}\",\"production_candidate\":{},\"pattern\":\"{}\",\"pool_cardinality\":{},\"actual_cardinality\":{},\"representation\":\"{}\",\"owned_bytes\":{}}}{suffix}",
            record.candidate,
            record.production_candidate,
            record.pattern,
            record.pool_cardinality,
            record.actual_cardinality,
            json_escape(&record.representation),
            record.owned_bytes,
        )
        .map_err(fmt_error)?;
    }
    writeln!(output, "  ],").map_err(fmt_error)?;

    writeln!(output, "  \"timings\": [").map_err(fmt_error)?;
    for (index, record) in timings.iter().enumerate() {
        let suffix = if index + 1 == timings.len() { "" } else { "," };
        write!(
            output,
            "    {{\"candidate\":\"{}\",\"production_candidate\":{},\"workload\":\"{}\",\"pattern\":\"{}\",\"pool_cardinality\":{},\"actual_cardinality\":{},\"representation\":\"{}\",\"unit\":\"{}\",\"operations_per_sample\":{},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"p50_ns_per_op\":{:.6},\"samples_ns\":[",
            record.candidate,
            record.production_candidate,
            json_escape(&record.workload),
            record.pattern,
            record.pool_cardinality,
            record.actual_cardinality,
            json_escape(&record.representation),
            record.unit,
            record.timing.operations_per_sample,
            record.timing.p50_ns,
            record.timing.p95_ns,
            record.timing.p99_ns,
            record.timing.max_ns,
            record.timing.p50_ns_per_op(),
        )
        .map_err(fmt_error)?;
        for (sample_index, sample) in record.timing.samples_ns.iter().enumerate() {
            if sample_index != 0 {
                output.push(',');
            }
            write!(output, "{sample}").map_err(fmt_error)?;
        }
        writeln!(output, "]}}{suffix}").map_err(fmt_error)?;
    }
    writeln!(output, "  ]").map_err(fmt_error)?;
    writeln!(output, "}}").map_err(fmt_error)?;
    Ok(output)
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not execute {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited unsuccessfully"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} output was not UTF-8"))
}

fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("model name\t:")
                    .map(|value| value.trim().to_owned())
            })
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn json_escape(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(result, "\\u{:04x}", u32::from(character));
            }
            character => result.push(character),
        }
    }
    result
}

fn fmt_error(_: fmt::Error) -> String {
    "formatting benchmark report unexpectedly failed".to_owned()
}

#[cfg(test)]
mod tests {
    use super::{Pattern, pattern_state_index, percentile};

    #[test]
    fn percentile_uses_nearest_rank_ceiling() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&values, 50), 30);
        assert_eq!(percentile(&values, 95), 50);
        assert_eq!(percentile(&values, 99), 50);
    }

    #[test]
    fn structured_patterns_are_deterministic_and_bounded() {
        for pattern in [
            Pattern::CardinalitySpread,
            Pattern::Homogeneous,
            Pattern::Layered,
            Pattern::Clustered,
            Pattern::Checker,
            Pattern::BuildLike,
        ] {
            let mut first_rng = 1;
            let mut second_rng = 1;
            for cell in 0..4096 {
                let first = pattern_state_index(pattern, cell, 16, &mut first_rng);
                let second = pattern_state_index(pattern, cell, 16, &mut second_rng);
                assert_eq!(first, second);
                assert!(first < 16);
            }
        }
    }
}
