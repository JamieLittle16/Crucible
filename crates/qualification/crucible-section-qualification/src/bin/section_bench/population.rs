use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{BufRead, BufReader, Read};
use std::mem;
use std::path::Path;
use std::time::Instant;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256};
use crucible_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::hardware::{self, HardwareMetadata};
use crate::model::{BenchSection, RepresentationCode};
use crate::workloads::pos;

const PACK_MAGIC: &str = "CRUCIBLE-SECTION-BENCH-PACK|1";
const PAYLOAD_BYTES_PER_SECTION: usize = BLOCK_SECTION_CELLS * mem::size_of::<u16>();
const REPORT_SCHEMA: u32 = 1;
const REPORT_VERSION: &str = "section-population-bench-v1";
const BENCH_SEED: u64 = 0x243F_6A88_85A3_08D3;

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

    const fn settings(self) -> PopulationSettings {
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
struct PopulationSettings {
    warmup_samples: usize,
    measured_samples: usize,
    random_reads: usize,
    sequential_sections: usize,
    volume_queries: usize,
    contains_queries: usize,
    control_operations: usize,
}

#[derive(Clone, Debug)]
struct PackHeader {
    population_sha256: String,
    admission_sha256: String,
    dimension: String,
    section_count: usize,
}

struct PackReader {
    reader: BufReader<File>,
    header: PackHeader,
    remaining_sections: usize,
}

impl PackReader {
    fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|error| format!("could not open population pack {}: {error}", path.display()))?;
        let mut reader = BufReader::new(file);
        let magic = read_canonical_text_line(&mut reader, "pack magic")?;
        if magic != PACK_MAGIC {
            return Err(format!("unsupported population pack magic: {magic:?}"));
        }
        let target = read_canonical_text_line(&mut reader, "pack target")?;
        validate_target_header(&target)?;
        let population = read_canonical_text_line(&mut reader, "pack population")?;
        let (population_sha256, admission_sha256) = parse_population_header(&population)?;
        let dimension = read_canonical_text_line(&mut reader, "pack dimension")?;
        let (dimension, section_count) = parse_dimension_header(&dimension)?;
        let data = read_canonical_text_line(&mut reader, "pack data marker")?;
        if data != "DATA" {
            return Err("population pack is missing DATA marker".to_owned());
        }
        if section_count == 0 {
            return Err("population pack must contain at least one section".to_owned());
        }
        Ok(Self {
            reader,
            header: PackHeader {
                population_sha256,
                admission_sha256,
                dimension,
                section_count,
            },
            remaining_sections: section_count,
        })
    }

    fn header(&self) -> &PackHeader {
        &self.header
    }

    fn read_section(&mut self, buffer: &mut [u8]) -> Result<bool, String> {
        if self.remaining_sections == 0 {
            return Ok(false);
        }
        if buffer.len() != PAYLOAD_BYTES_PER_SECTION {
            return Err("population section scratch buffer has wrong size".to_owned());
        }
        self.reader
            .read_exact(buffer)
            .map_err(|error| format!("population pack payload ended early: {error}"))?;
        self.remaining_sections -= 1;
        Ok(true)
    }

    fn finish(mut self) -> Result<(), String> {
        if self.remaining_sections != 0 {
            return Err(format!(
                "population pack has {} unread sections",
                self.remaining_sections
            ));
        }
        let mut trailing = [0_u8; 1];
        match self.reader.read(&mut trailing) {
            Ok(0) => Ok(()),
            Ok(_) => Err("population pack contains trailing bytes".to_owned()),
            Err(error) => Err(format!("could not verify population pack EOF: {error}")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ProcessMemory {
    rss_kib: u64,
    high_water_kib: u64,
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
    fn p50_ps_per_op(&self) -> u128 {
        let operations =
            u128::try_from(self.operations_per_sample).expect("operation count fits u128");
        self.p50_ns.saturating_mul(1_000) / operations
    }
}

#[derive(Clone, Debug)]
struct TimingRecord {
    workload: &'static str,
    unit: &'static str,
    timing: SampleSummary,
}

#[derive(Debug)]
struct LoadedCandidate<C> {
    sections: Vec<C>,
    construction: SampleSummary,
    construction_transitions: usize,
    logical_backing_allocations: usize,
    logical_owned_bytes: usize,
    max_owned_bytes: usize,
    representations: BTreeMap<String, usize>,
    rss_baseline_kib: u64,
    rss_loaded_kib: u64,
    rss_loaded_delta_kib: u64,
    rss_high_water_kib: u64,
    harness_bookkeeping_bytes_at_rss: usize,
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
    let mut reader = PackReader::open(pack_path)?;
    let header = reader.header().clone();
    let mut scratch = vec![0_u8; PAYLOAD_BYTES_PER_SECTION];
    let baseline = process_memory()?;

    let mut sections = Vec::with_capacity(header.section_count);
    let mut construction_samples = Vec::with_capacity(header.section_count);
    let mut construction_transitions = 0_usize;
    let mut logical_backing_allocations = 0_usize;
    let mut logical_owned_bytes = 0_usize;
    let mut max_owned_bytes = 0_usize;

    while reader.read_section(&mut scratch)? {
        let start = Instant::now();
        let built = build_candidate::<C>(&scratch)?;
        construction_samples.push(start.elapsed().as_nanos());
        construction_transitions = construction_transitions
            .checked_add(built.transitions)
            .ok_or_else(|| "population construction transition count overflow".to_owned())?;
        logical_backing_allocations = logical_backing_allocations
            .checked_add(built.logical_allocations)
            .ok_or_else(|| "population logical allocation count overflow".to_owned())?;
        logical_owned_bytes = logical_owned_bytes
            .checked_add(built.owned_bytes)
            .ok_or_else(|| "population logical owned-byte count overflow".to_owned())?;
        max_owned_bytes = max_owned_bytes.max(built.owned_bytes);
        sections.push(built.section);
    }
    reader.finish()?;
    if sections.len() != header.section_count {
        return Err(format!(
            "population pack declared {} sections but constructed {}",
            header.section_count,
            sections.len()
        ));
    }
    black_box(&sections);

    let loaded_memory = process_memory()?;
    let harness_bookkeeping_bytes_at_rss = construction_samples
        .capacity()
        .checked_mul(mem::size_of::<u128>())
        .unwrap_or(usize::MAX);
    let mut representations = BTreeMap::new();
    for section in &sections {
        *representations.entry(section.representation_name()).or_insert(0_usize) += 1;
    }

    let construction = summarize(construction_samples, 1);
    let loaded = LoadedCandidate {
        sections,
        construction,
        construction_transitions,
        logical_backing_allocations,
        logical_owned_bytes,
        max_owned_bytes,
        representations,
        rss_baseline_kib: baseline.rss_kib,
        rss_loaded_kib: loaded_memory.rss_kib,
        rss_loaded_delta_kib: loaded_memory.rss_kib.saturating_sub(baseline.rss_kib),
        rss_high_water_kib: loaded_memory.high_water_kib,
        harness_bookkeeping_bytes_at_rss,
    };

    let timings = steady_state_timings(&loaded.sections, settings)?;
    let hardware = hardware::collect()?;
    render_report::<C>(mode, settings, &header, &loaded, &timings, &hardware)
}

struct BuiltCandidate<C> {
    section: C,
    transitions: usize,
    logical_allocations: usize,
    owned_bytes: usize,
}

fn build_candidate<C: BenchSection>(bytes: &[u8]) -> Result<BuiltCandidate<C>, String> {
    let first = decode_state(bytes, 0)?;
    let mut section = C::filled(first);
    let mut transitions = 0_usize;
    let mut logical_allocations = C::initial_logical_allocations();
    for cell in 1..BLOCK_SECTION_CELLS {
        let state = decode_state(bytes, cell)?;
        if state == first {
            continue;
        }
        let before = section.representation_code();
        let _ = section.replace(pos(cell), state, &GeneratedStateFacts);
        let after = section.representation_code();
        if before != after {
            transitions = transitions
                .checked_add(1)
                .ok_or_else(|| "population representation transition count overflow".to_owned())?;
            logical_allocations = logical_allocations
                .checked_add(C::transition_logical_allocations(before, after))
                .ok_or_else(|| "population logical allocation count overflow".to_owned())?;
        }
    }
    for cell in 0..BLOCK_SECTION_CELLS {
        let expected = decode_state(bytes, cell)?;
        let actual = section.get(pos(cell));
        if actual != expected {
            return Err(format!(
                "{} population reconstruction mismatch at cell {cell}: expected {}, got {}",
                C::NAME,
                expected.as_usize(),
                actual.as_usize()
            ));
        }
    }
    let owned_bytes = section.owned_bytes();
    Ok(BuiltCandidate {
        section,
        transitions,
        logical_allocations,
        owned_bytes,
    })
}

fn decode_state(bytes: &[u8], cell: usize) -> Result<BlockStateId, String> {
    let offset = cell
        .checked_mul(2)
        .ok_or_else(|| "population pack cell offset overflow".to_owned())?;
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| format!("population pack is missing cell {cell}"))?;
    let raw = u16::from_le_bytes([pair[0], pair[1]]);
    BlockStateId::new(u32::from(raw)).ok_or_else(|| {
        format!(
            "population pack state {raw} at cell {cell} lies outside 0..{}",
            BLOCK_STATE_COUNT - 1
        )
    })
}

fn steady_state_timings<C: BenchSection>(
    sections: &[C],
    settings: PopulationSettings,
) -> Result<Vec<TimingRecord>, String> {
    if sections.is_empty() {
        return Err("population benchmark has no loaded sections".to_owned());
    }
    let random_plan = query_plan(settings.random_reads, sections.len(), BENCH_SEED ^ 0x10);
    let volume_plan = query_plan(settings.volume_queries, sections.len(), BENCH_SEED ^ 0x20);
    let contains_plan = query_plan(settings.contains_queries, sections.len(), BENCH_SEED ^ 0x30);
    let scan_count = settings.sequential_sections.min(sections.len());
    let scan_plan = section_plan(scan_count, sections.len(), BENCH_SEED ^ 0x40);
    let positive_states = sections
        .iter()
        .map(|section| section.get(pos(0)))
        .collect::<Vec<_>>();
    let negative = globally_absent_state(sections)?;

    let random = measure(
        settings,
        settings.random_reads,
        |operation| {
            let (section, cell) = random_plan[operation];
            black_box(sections[section].get(pos(cell)));
        },
    );

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
        let needle = positive_states[section_index];
        black_box(sections[section_index].maybe_contains(|state| state == needle));
    });
    let negative_timing = measure(settings, settings.contains_queries, |operation| {
        let (section_index, _) = contains_plan[operation];
        black_box(sections[section_index].maybe_contains(|state| state == negative));
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
            timing: negative_timing,
        },
        TimingRecord {
            workload: "control-integer-loop",
            unit: "iteration",
            timing: control,
        },
    ])
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
    let operations = settings.control_operations;
    measure(settings, operations, |index| {
        let mut value = (index as u64) ^ 0x9E37_79B9_7F4A_7C15;
        value = value.rotate_left(17).wrapping_mul(0xD6E8_FEB8_6659_FD93);
        value ^= value >> 23;
        black_box(value);
    })
}

fn with_operations(mut summary: SampleSummary, operations: usize) -> SampleSummary {
    summary.operations_per_sample = operations;
    summary
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

fn query_plan(count: usize, section_count: usize, seed: u64) -> Vec<(usize, usize)> {
    let mut state = seed;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        state = xorshift64(state);
        let section = (state as usize) % section_count;
        state = xorshift64(state);
        let cell = (state as usize) % BLOCK_SECTION_CELLS;
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

fn globally_absent_state<C: BenchSection>(sections: &[C]) -> Result<BlockStateId, String> {
    for raw in (0..BLOCK_STATE_COUNT).rev() {
        let candidate = BlockStateId::new(u32::try_from(raw).expect("target state ID fits u32"))
            .expect("target state ID exists");
        let present = sections.iter().any(|section| {
            (0..BLOCK_SECTION_CELLS).any(|cell| section.get(pos(cell)) == candidate)
        });
        if !present {
            return Ok(candidate);
        }
    }
    Err("population uses every target block state; no negative membership needle exists".to_owned())
}

fn process_memory() -> Result<ProcessMemory, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|error| format!("could not read /proc/self/status for RSS evidence: {error}"))?;
    let rss_kib = status_value_kib(&status, "VmRSS:")?;
    let high_water_kib = status_value_kib(&status, "VmHWM:")?;
    Ok(ProcessMemory {
        rss_kib,
        high_water_kib,
    })
}

fn status_value_kib(status: &str, key: &str) -> Result<u64, String> {
    let line = status
        .lines()
        .find(|line| line.starts_with(key))
        .ok_or_else(|| format!("/proc/self/status is missing {key}"))?;
    let mut parts = line[key.len()..].split_whitespace();
    let value = parts
        .next()
        .ok_or_else(|| format!("{key} has no numeric value"))?
        .parse::<u64>()
        .map_err(|_| format!("{key} value is not an integer"))?;
    let unit = parts.next().unwrap_or("");
    if unit != "kB" {
        return Err(format!("{key} has unexpected unit {unit:?}"));
    }
    Ok(value)
}

fn render_report<C: BenchSection>(
    mode: PopulationMode,
    settings: PopulationSettings,
    header: &PackHeader,
    loaded: &LoadedCandidate<C>,
    timings: &[TimingRecord],
    hardware: &HardwareMetadata,
) -> Result<String, String> {
    let frequency_cpu = single_cpu(&hardware.cpus_allowed_list);
    let (affinity_governor, affinity_current_khz, affinity_min_khz, affinity_max_khz) =
        frequency_cpu.map_or_else(
            || ("unknown".to_owned(), "unknown".to_owned(), "unknown".to_owned(), "unknown".to_owned()),
            |cpu| cpu_frequency_metadata(cpu),
        );
    let mut output = String::new();
    writeln!(output, "{{").map_err(fmt_error)?;
    writeln!(output, "  \"schema\": {REPORT_SCHEMA},").map_err(fmt_error)?;
    writeln!(output, "  \"harness_version\": \"{REPORT_VERSION}\",").map_err(fmt_error)?;
    writeln!(output, "  \"mode\": \"{}\",", mode.as_str()).map_err(fmt_error)?;
    write_json_string(&mut output, "candidate", C::NAME, true)?;
    writeln!(
        output,
        "  \"production_candidate\": {},",
        C::PRODUCTION_CANDIDATE
    )
    .map_err(fmt_error)?;
    write_json_string(&mut output, "minecraft_version", MINECRAFT_VERSION, true)?;
    writeln!(output, "  \"protocol_version\": {PROTOCOL_VERSION},").map_err(fmt_error)?;
    writeln!(output, "  \"data_version\": {DATA_VERSION},").map_err(fmt_error)?;
    writeln!(output, "  \"state_count\": {BLOCK_STATE_COUNT},").map_err(fmt_error)?;
    write_json_string(
        &mut output,
        "state_data_generation_sha256",
        STATE_DATA_GENERATION_SHA256,
        true,
    )?;
    write_json_string(&mut output, "population_sha256", &header.population_sha256, true)?;
    write_json_string(&mut output, "admission_sha256", &header.admission_sha256, true)?;
    write_json_string(&mut output, "dimension", &header.dimension, true)?;
    writeln!(output, "  \"section_count\": {},", header.section_count).map_err(fmt_error)?;
    write_json_string(&mut output, "commit_sha", &hardware.commit_sha, true)?;
    write_json_string(&mut output, "target_triple", &hardware.target_triple, true)?;
    write_json_string(&mut output, "cpu_model", &hardware.cpu_model, true)?;
    write_json_string(&mut output, "kernel", &hardware.kernel, true)?;
    write_json_string(
        &mut output,
        "cpus_allowed_list",
        &hardware.cpus_allowed_list,
        true,
    )?;
    write_json_string(&mut output, "load_average", &hardware.load_average, true)?;
    write_json_string(&mut output, "rustflags", &hardware.rustflags, true)?;
    write_json_string(
        &mut output,
        "cargo_encoded_rustflags",
        &hardware.cargo_encoded_rustflags,
        true,
    )?;
    write_json_string(&mut output, "rustc_verbose", &hardware.rustc_verbose, true)?;
    match frequency_cpu {
        Some(cpu) => writeln!(output, "  \"affinity_frequency_cpu\": {cpu},").map_err(fmt_error)?,
        None => writeln!(output, "  \"affinity_frequency_cpu\": null,").map_err(fmt_error)?,
    }
    write_json_string(&mut output, "affinity_cpu_governor", &affinity_governor, true)?;
    write_json_string(
        &mut output,
        "affinity_cpu_current_khz",
        &affinity_current_khz,
        true,
    )?;
    write_json_string(&mut output, "affinity_cpu_min_khz", &affinity_min_khz, true)?;
    write_json_string(&mut output, "affinity_cpu_max_khz", &affinity_max_khz, true)?;
    write_json_string(&mut output, "intel_pstate_no_turbo", &hardware.no_turbo, true)?;
    writeln!(output, "  \"settings\": {{").map_err(fmt_error)?;
    writeln!(output, "    \"warmup_samples\": {},", settings.warmup_samples).map_err(fmt_error)?;
    writeln!(output, "    \"measured_samples\": {},", settings.measured_samples).map_err(fmt_error)?;
    writeln!(output, "    \"random_reads\": {},", settings.random_reads).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"sequential_sections\": {},",
        settings.sequential_sections
    )
    .map_err(fmt_error)?;
    writeln!(output, "    \"volume_queries\": {},", settings.volume_queries).map_err(fmt_error)?;
    writeln!(output, "    \"contains_queries\": {},", settings.contains_queries).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"control_operations\": {}",
        settings.control_operations
    )
    .map_err(fmt_error)?;
    writeln!(output, "  }},").map_err(fmt_error)?;
    writeln!(output, "  \"memory\": {{").map_err(fmt_error)?;
    writeln!(output, "    \"unit\": \"KiB\",").map_err(fmt_error)?;
    writeln!(output, "    \"rss_baseline_kib\": {},", loaded.rss_baseline_kib).map_err(fmt_error)?;
    writeln!(output, "    \"rss_loaded_kib\": {},", loaded.rss_loaded_kib).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"rss_loaded_delta_kib\": {},",
        loaded.rss_loaded_delta_kib
    )
    .map_err(fmt_error)?;
    writeln!(output, "    \"rss_high_water_kib\": {},", loaded.rss_high_water_kib).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"logical_owned_bytes\": {},",
        loaded.logical_owned_bytes
    )
    .map_err(fmt_error)?;
    writeln!(output, "    \"max_owned_bytes\": {},", loaded.max_owned_bytes).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"harness_bookkeeping_bytes_at_rss\": {},",
        loaded.harness_bookkeeping_bytes_at_rss
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"construction_transitions\": {},",
        loaded.construction_transitions
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"logical_backing_allocations\": {}",
        loaded.logical_backing_allocations
    )
    .map_err(fmt_error)?;
    writeln!(output, "  }},").map_err(fmt_error)?;
    write_string_usize_map(&mut output, "representations", &loaded.representations)?;
    output.push_str(",\n");
    writeln!(output, "  \"construction\": ").map_err(fmt_error)?;
    write_sample_summary(&mut output, &loaded.construction, 2)?;
    output.push_str(",\n  \"timings\": [\n");
    for (index, timing) in timings.iter().enumerate() {
        write!(
            output,
            "    {{\"workload\":\"{}\",\"unit\":\"{}\",\"timing\":",
            json_escape(timing.workload),
            json_escape(timing.unit)
        )
        .map_err(fmt_error)?;
        write_sample_summary(&mut output, &timing.timing, 0)?;
        output.push('}');
        if index + 1 != timings.len() {
            output.push(',');
        }
        output.push('\n');
    }
    output.push_str("  ]\n}\n");
    Ok(output)
}

fn write_sample_summary(
    output: &mut String,
    summary: &SampleSummary,
    indent: usize,
) -> Result<(), String> {
    let prefix = " ".repeat(indent);
    write!(
        output,
        "{prefix}{{\"operations_per_sample\":{},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"p50_ps_per_op\":{},\"samples_ns\":[",
        summary.operations_per_sample,
        summary.p50_ns,
        summary.p95_ns,
        summary.p99_ns,
        summary.max_ns,
        summary.p50_ps_per_op()
    )
    .map_err(fmt_error)?;
    for (index, sample) in summary.samples_ns.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "{sample}").map_err(fmt_error)?;
    }
    output.push_str("]}");
    Ok(())
}

fn write_string_usize_map(
    output: &mut String,
    key: &str,
    values: &BTreeMap<String, usize>,
) -> Result<(), String> {
    write!(output, "  \"{}\": {{", json_escape(key)).map_err(fmt_error)?;
    for (index, (name, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "\"{}\":{value}", json_escape(name)).map_err(fmt_error)?;
    }
    output.push('}');
    Ok(())
}

fn write_json_string(
    output: &mut String,
    key: &str,
    value: &str,
    comma: bool,
) -> Result<(), String> {
    writeln!(
        output,
        "  \"{}\": \"{}\"{}",
        json_escape(key),
        json_escape(value),
        if comma { "," } else { "" }
    )
    .map_err(fmt_error)
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(control));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn fmt_error(_: fmt::Error) -> String {
    "formatting population benchmark report unexpectedly failed".to_owned()
}

fn validate_target_header(line: &str) -> Result<(), String> {
    let parts = line.split('|').collect::<Vec<_>>();
    if parts.len() != 6 || parts[0] != "TARGET" {
        return Err("population pack TARGET header has the wrong shape".to_owned());
    }
    let minecraft = field(parts[1], "minecraft")?;
    let protocol = canonical_usize(field(parts[2], "protocol")?, "protocol")?;
    let data = canonical_usize(field(parts[3], "data")?, "data")?;
    let state_count = canonical_usize(field(parts[4], "state_count")?, "state_count")?;
    let generation = field(parts[5], "generation_sha256")?;
    if minecraft != MINECRAFT_VERSION
        || protocol != PROTOCOL_VERSION as usize
        || data != DATA_VERSION as usize
        || state_count != BLOCK_STATE_COUNT
        || generation != STATE_DATA_GENERATION_SHA256
    {
        return Err("population pack TARGET header does not match frozen target data".to_owned());
    }
    Ok(())
}

fn parse_population_header(line: &str) -> Result<(String, String), String> {
    let parts = line.split('|').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != "POPULATION" {
        return Err("population pack POPULATION header has the wrong shape".to_owned());
    }
    let population = field(parts[1], "population_sha256")?;
    let admission = field(parts[2], "admission_sha256")?;
    if !is_lower_sha256(population) || !is_lower_sha256(admission) {
        return Err("population/admission identities must be lowercase SHA-256".to_owned());
    }
    Ok((population.to_owned(), admission.to_owned()))
}

fn parse_dimension_header(line: &str) -> Result<(String, usize), String> {
    let parts = line.split('|').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != "DIMENSION" {
        return Err("population pack DIMENSION header has the wrong shape".to_owned());
    }
    let dimension = field(parts[1], "name")?;
    if !is_resource_location(dimension) {
        return Err(format!("invalid population pack dimension {dimension:?}"));
    }
    let section_count = canonical_usize(field(parts[2], "section_count")?, "section_count")?;
    Ok((dimension.to_owned(), section_count))
}

fn read_canonical_text_line<R: BufRead>(reader: &mut R, label: &str) -> Result<String, String> {
    let mut line = String::new();
    let count = reader
        .read_line(&mut line)
        .map_err(|error| format!("could not read {label}: {error}"))?;
    if count == 0 {
        return Err(format!("population pack ended before {label}"));
    }
    if line.contains('\r') || !line.ends_with('\n') {
        return Err(format!("{label} must use canonical LF line ending"));
    }
    line.pop();
    if line.is_empty() {
        return Err(format!("{label} must not be empty"));
    }
    Ok(line)
}

fn field<'a>(part: &'a str, name: &str) -> Result<&'a str, String> {
    let prefix = format!("{name}=");
    let value = part
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("population pack header expected field {name}"))?;
    if value.is_empty() {
        return Err(format!("population pack header field {name} is empty"));
    }
    Ok(value)
}

fn canonical_usize(raw: &str, label: &str) -> Result<usize, String> {
    if raw != "0"
        && (raw.is_empty()
            || raw.starts_with('0')
            || !raw.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!("{label} is not a canonical nonnegative integer"));
    }
    raw.parse::<usize>()
        .map_err(|_| format!("{label} does not fit usize"))
}

fn is_lower_sha256(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_resource_location(raw: &str) -> bool {
    let Some((namespace, path)) = raw.split_once(':') else {
        return false;
    };
    if namespace.is_empty() || path.is_empty() || path.contains(':') {
        return false;
    }
    namespace.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
    }) && path.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'.' | b'/' | b'-')
    })
}

fn single_cpu(allowed: &str) -> Option<usize> {
    if allowed.bytes().all(|byte| byte.is_ascii_digit()) && !allowed.is_empty() {
        allowed.parse().ok()
    } else {
        None
    }
}

fn cpu_frequency_metadata(cpu: usize) -> (String, String, String, String) {
    let root = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq");
    (
        read_trimmed(format!("{root}/scaling_governor")),
        read_trimmed(format!("{root}/scaling_cur_freq")),
        read_trimmed(format!("{root}/scaling_min_freq")),
        read_trimmed(format!("{root}/scaling_max_freq")),
    )
}

fn read_trimmed(path: String) -> String {
    fs::read_to_string(path).map_or_else(|_| "unknown".to_owned(), |value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        PACK_MAGIC, PAYLOAD_BYTES_PER_SECTION, PackReader, PopulationMode, canonical_usize,
        decode_state, percentile, single_cpu, status_value_kib,
    };
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crucible_generated::{BLOCK_STATE_COUNT, STATE_DATA_GENERATION_SHA256};
    use crucible_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_pack(payload_sections: usize, trailing: &[u8]) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "crucible-section-population-pack-{}-{id}.bin",
            std::process::id()
        ));
        let mut file = fs::File::create(&path).expect("create pack");
        write!(
            file,
            "{PACK_MAGIC}TARGET|minecraft={MINECRAFT_VERSION}|protocol={PROTOCOL_VERSION}|data={DATA_VERSION}|state_count={BLOCK_STATE_COUNT}|generation_sha256={STATE_DATA_GENERATION_SHA256}\nPOPULATION|population_sha256={}|admission_sha256={}\nDIMENSION|name=minecraft:overworld|section_count={payload_sections}\nDATA\n",
            "a".repeat(64),
            "b".repeat(64),
        )
        .expect("write header");
        let mut payload = vec![0_u8; payload_sections * PAYLOAD_BYTES_PER_SECTION];
        if payload_sections > 0 {
            payload[2] = 1;
        }
        file.write_all(&payload).expect("write payload");
        file.write_all(trailing).expect("write trailing");
        path
    }

    #[test]
    fn pack_reader_accepts_exact_payload_and_rejects_trailing_bytes() {
        let path = temp_pack(1, &[]);
        let mut reader = PackReader::open(&path).expect("valid pack");
        let mut scratch = vec![0_u8; PAYLOAD_BYTES_PER_SECTION];
        assert!(reader.read_section(&mut scratch).expect("section"));
        assert!(!reader.read_section(&mut scratch).expect("eof"));
        reader.finish().expect("exact eof");
        assert_eq!(decode_state(&scratch, 1).expect("state").as_usize(), 1);
        fs::remove_file(path).expect("remove pack");

        let bad = temp_pack(1, b"x");
        let mut reader = PackReader::open(&bad).expect("headers valid");
        assert!(reader.read_section(&mut scratch).expect("section"));
        assert!(reader.finish().is_err());
        fs::remove_file(bad).expect("remove bad pack");
    }

    #[test]
    fn zero_section_pack_is_rejected() {
        let path = temp_pack(0, &[]);
        assert!(PackReader::open(&path).is_err());
        fs::remove_file(path).expect("remove pack");
    }

    #[test]
    fn state_outside_target_is_rejected() {
        let mut bytes = vec![0_u8; PAYLOAD_BYTES_PER_SECTION];
        let raw = u16::try_from(BLOCK_STATE_COUNT).expect("target count fits u16");
        bytes[..2].copy_from_slice(&raw.to_le_bytes());
        assert!(decode_state(&bytes, 0).is_err());
    }

    #[test]
    fn percentile_and_canonical_numbers_are_stable() {
        assert_eq!(percentile(&[10, 20, 30, 40, 50], 50), 30);
        assert_eq!(percentile(&[10, 20, 30, 40, 50], 99), 50);
        assert_eq!(canonical_usize("0", "test"), Ok(0));
        assert_eq!(canonical_usize("17", "test"), Ok(17));
        assert!(canonical_usize("01", "test").is_err());
    }

    #[test]
    fn affinity_frequency_cpu_requires_single_exact_cpu() {
        assert_eq!(single_cpu("7"), Some(7));
        assert_eq!(single_cpu("0-3"), None);
        assert_eq!(single_cpu("1,3"), None);
    }

    #[test]
    fn proc_status_memory_parser_requires_kilobytes() {
        let status = "Name:\ttest\nVmRSS:\t1234 kB\nVmHWM:\t2345 kB\n";
        assert_eq!(status_value_kib(status, "VmRSS:"), Ok(1234));
        assert_eq!(status_value_kib(status, "VmHWM:"), Ok(2345));
        assert!(status_value_kib("VmRSS: 10 MB\n", "VmRSS:").is_err());
    }

    #[test]
    fn qualification_settings_are_stronger_than_smoke() {
        let smoke = PopulationMode::Smoke.settings();
        let qualification = PopulationMode::Qualification.settings();
        assert!(qualification.measured_samples > smoke.measured_samples);
        assert!(qualification.random_reads > smoke.random_reads);
        assert!(qualification.control_operations > smoke.control_operations);
    }
}
