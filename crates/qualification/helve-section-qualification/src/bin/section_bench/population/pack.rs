use std::collections::BTreeMap;
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{BufRead, BufReader, Read};
use std::mem;
use std::path::Path;
use std::time::Instant;

use helve_generated::{
    AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
};
use helve_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use helve_world_contract::{BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionSummary};

use crate::model::BenchSection;
use crate::workloads::pos;

use super::{SampleSummary, measure};

pub(super) const PACK_MAGIC: &str = "CRUCIBLE-SECTION-BENCH-PACK|1";
pub(super) const PAYLOAD_BYTES_PER_SECTION: usize = BLOCK_SECTION_CELLS * mem::size_of::<u16>();
pub(super) const RSS_PROTOCOL: &str = "candidate-delta-after-explicit-prefaulted-common-scratch";
const STATE_SEEN_WORDS: usize = BLOCK_STATE_COUNT.div_ceil(64);

#[derive(Clone, Debug)]
pub(super) struct PackHeader {
    pub(super) population_sha256: String,
    pub(super) admission_sha256: String,
    pub(super) dimension: String,
    pub(super) section_count: usize,
}

pub(super) struct LoadedCandidate<C> {
    pub(super) header: PackHeader,
    pub(super) sections: Vec<C>,
    pub(super) negative_state: BlockStateId,
    pub(super) construction: SampleSummary,
    pub(super) construction_transitions: usize,
    pub(super) logical_backing_allocations: usize,
    pub(super) logical_owned_bytes: usize,
    pub(super) max_owned_bytes: usize,
    pub(super) representations: BTreeMap<String, usize>,
    pub(super) rss_baseline_kib: u64,
    pub(super) rss_loaded_kib: u64,
    pub(super) rss_loaded_delta_kib: i64,
    pub(super) rss_baseline_high_water_kib: u64,
    pub(super) rss_loaded_high_water_kib: u64,
    pub(super) known_prebaseline_harness_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
struct ProcessMemory {
    rss_kib: u64,
    high_water_kib: u64,
}

pub(super) struct PackReader {
    reader: BufReader<File>,
    header: PackHeader,
    remaining_sections: usize,
}

impl PackReader {
    pub(super) fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| {
            format!("could not open population pack {}: {error}", path.display())
        })?;
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

    pub(super) fn header(&self) -> &PackHeader {
        &self.header
    }

    pub(super) fn read_section(&mut self, buffer: &mut [u8]) -> Result<bool, String> {
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

    pub(super) fn finish(mut self) -> Result<(), String> {
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

pub(super) fn load_candidate<C: BenchSection>(
    pack_path: &Path,
) -> Result<LoadedCandidate<C>, String> {
    let mut reader = PackReader::open(pack_path)?;
    let header = reader.header().clone();

    // Common parsing/measurement scratch is both allocated and explicitly dirtied before the RSS
    // baseline. Merely reserving capacity is insufficient on demand-paged systems: later writes
    // could otherwise fault common harness pages after the baseline and falsely attribute them to
    // the candidate. The retained candidate vector is deliberately allocated only afterwards.
    let mut raw_scratch = vec![0_u8; PAYLOAD_BYTES_PER_SECTION];
    let mut decoded_states = vec![AIR; BLOCK_SECTION_CELLS];
    let mut observed_states = [0_u64; STATE_SEEN_WORDS];
    let mut construction_samples = vec![0_u128; header.section_count];
    prefault_common_scratch(
        &mut raw_scratch,
        &mut decoded_states,
        &mut observed_states,
        &mut construction_samples,
    );
    let known_prebaseline_harness_bytes = raw_scratch
        .capacity()
        .checked_add(
            decoded_states
                .capacity()
                .checked_mul(mem::size_of::<BlockStateId>())
                .ok_or_else(|| "decoded-state scratch byte count overflow".to_owned())?,
        )
        .and_then(|value| {
            value.checked_add(
                construction_samples
                    .len()
                    .checked_mul(mem::size_of::<u128>())?,
            )
        })
        .ok_or_else(|| "prebaseline harness byte count overflow".to_owned())?;
    let baseline = process_memory()?;

    let mut sections = Vec::with_capacity(header.section_count);
    let mut construction_sample_index = 0_usize;
    let mut construction_transitions = 0_usize;
    let mut logical_backing_allocations = 0_usize;
    let mut logical_owned_bytes = 0_usize;
    let mut max_owned_bytes = 0_usize;

    while reader.read_section(&mut raw_scratch)? {
        decode_section(&raw_scratch, &mut decoded_states, &mut observed_states)?;

        let start = Instant::now();
        let built = construct_candidate::<C>(&decoded_states)?;
        let elapsed = start.elapsed().as_nanos();
        let sample = construction_samples
            .get_mut(construction_sample_index)
            .ok_or_else(|| {
                "population construction sample count exceeded pack header".to_owned()
            })?;
        *sample = elapsed;
        construction_sample_index += 1;

        verify_candidate::<C>(&built.section, &decoded_states)?;
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

    if sections.len() != header.section_count || construction_sample_index != header.section_count {
        return Err(format!(
            "population pack declared {} sections but constructed {} with {} timing samples",
            header.section_count,
            sections.len(),
            construction_sample_index
        ));
    }
    black_box(&sections);
    let loaded_memory = process_memory()?;
    let negative_state = absent_state(&observed_states)?;

    // Representation-map allocation occurs only after the RSS snapshot.
    let mut representations = BTreeMap::new();
    for section in &sections {
        let entry = representations
            .entry(section.representation_name())
            .or_insert(0_usize);
        *entry = entry
            .checked_add(1)
            .ok_or_else(|| "population representation count overflow".to_owned())?;
    }

    Ok(LoadedCandidate {
        header,
        sections,
        negative_state,
        construction: measure::summarize(construction_samples, 1),
        construction_transitions,
        logical_backing_allocations,
        logical_owned_bytes,
        max_owned_bytes,
        representations,
        rss_baseline_kib: baseline.rss_kib,
        rss_loaded_kib: loaded_memory.rss_kib,
        rss_loaded_delta_kib: signed_rss_delta(loaded_memory.rss_kib, baseline.rss_kib)?,
        rss_baseline_high_water_kib: baseline.high_water_kib,
        rss_loaded_high_water_kib: loaded_memory.high_water_kib,
        known_prebaseline_harness_bytes,
    })
}

pub(super) fn prefault_common_scratch(
    raw_scratch: &mut [u8],
    decoded_states: &mut [BlockStateId],
    observed_states: &mut [u64],
    construction_samples: &mut [u128],
) {
    // Write a non-zero marker before restoring the canonical initial value. The black-box barriers
    // make the writes observably relevant to the benchmark process and prevent the optimizer from
    // collapsing the prefault pass into an untouched zero allocation.
    black_box(&mut *raw_scratch).fill(0xA5);
    black_box(&*raw_scratch);
    raw_scratch.fill(0);

    let marker = BlockStateId::new(1).expect("qualified target contains block state 1");
    black_box(&mut *decoded_states).fill(marker);
    black_box(&*decoded_states);
    decoded_states.fill(AIR);

    black_box(&mut *observed_states).fill(u64::MAX);
    black_box(&*observed_states);
    observed_states.fill(0);

    black_box(&mut *construction_samples).fill(1);
    black_box(&*construction_samples);
    construction_samples.fill(0);

    black_box(&*raw_scratch);
    black_box(&*decoded_states);
    black_box(&*observed_states);
    black_box(&*construction_samples);
}

pub(super) fn signed_rss_delta(loaded_kib: u64, baseline_kib: u64) -> Result<i64, String> {
    let loaded = i64::try_from(loaded_kib)
        .map_err(|_| "loaded RSS does not fit signed qualification range".to_owned())?;
    let baseline = i64::try_from(baseline_kib)
        .map_err(|_| "baseline RSS does not fit signed qualification range".to_owned())?;
    loaded
        .checked_sub(baseline)
        .ok_or_else(|| "RSS delta overflow".to_owned())
}

struct BuiltCandidate<C> {
    section: C,
    transitions: usize,
    logical_allocations: usize,
    owned_bytes: usize,
}

fn construct_candidate<C: BenchSection>(
    states: &[BlockStateId],
) -> Result<BuiltCandidate<C>, String> {
    let first = *states
        .first()
        .ok_or_else(|| "decoded population section unexpectedly has no cells".to_owned())?;
    let mut section = C::filled(first);
    let mut transitions = 0_usize;
    let mut logical_allocations = C::initial_logical_allocations();

    for (cell, state) in states.iter().copied().enumerate().skip(1) {
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

    let owned_bytes = section.owned_bytes();
    Ok(BuiltCandidate {
        section,
        transitions,
        logical_allocations,
        owned_bytes,
    })
}

fn verify_candidate<C: BenchSection>(candidate: &C, states: &[BlockStateId]) -> Result<(), String> {
    for (cell, expected) in states.iter().copied().enumerate() {
        let actual = candidate.get(pos(cell));
        if actual != expected {
            return Err(format!(
                "{} population reconstruction mismatch at cell {cell}: expected {}, got {}",
                C::NAME,
                expected.as_usize(),
                actual.as_usize()
            ));
        }
    }

    let expected_summary = recompute_summary(states);
    let actual_summary = <C as BlockSection<BlockStateId>>::summary(candidate);
    if actual_summary != expected_summary {
        return Err(format!(
            "{} population summary mismatch: expected {expected_summary:?}, got {actual_summary:?}",
            C::NAME
        ));
    }
    Ok(())
}

fn recompute_summary(states: &[BlockStateId]) -> SectionSummary {
    let mut summary = SectionSummary::default();
    for state in states.iter().copied() {
        let facts = <GeneratedStateFacts as BlockStateFacts<BlockStateId>>::facts(
            &GeneratedStateFacts,
            state,
        );
        summary.non_air_count += u16::from(facts.non_air());
        summary.fluid_count += u16::from(facts.counted_fluid());
        summary.random_block_present |= facts.random_block();
        summary.random_fluid_present |= facts.random_fluid();
    }
    summary
}

fn decode_section(
    bytes: &[u8],
    states: &mut [BlockStateId],
    observed: &mut [u64; STATE_SEEN_WORDS],
) -> Result<(), String> {
    if bytes.len() != PAYLOAD_BYTES_PER_SECTION || states.len() != BLOCK_SECTION_CELLS {
        return Err("population section scratch shape mismatch".to_owned());
    }
    for (cell, state) in states.iter_mut().enumerate() {
        let offset = cell * 2;
        let raw = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let decoded = BlockStateId::new(u32::from(raw)).ok_or_else(|| {
            format!(
                "population pack state {raw} at cell {cell} lies outside 0..{}",
                BLOCK_STATE_COUNT - 1
            )
        })?;
        *state = decoded;
        let index = decoded.as_usize();
        observed[index >> 6] |= 1_u64 << (index & 63);
    }
    Ok(())
}

fn absent_state(observed: &[u64; STATE_SEEN_WORDS]) -> Result<BlockStateId, String> {
    for raw in (0..BLOCK_STATE_COUNT).rev() {
        let word = raw >> 6;
        let bit = 1_u64 << (raw & 63);
        if observed[word] & bit == 0 {
            return BlockStateId::new(u32::try_from(raw).expect("target state ID fits u32"))
                .ok_or_else(|| "target state bitset contained invalid identity".to_owned());
        }
    }
    Err("population uses every target block state; no negative membership needle exists".to_owned())
}

fn process_memory() -> Result<ProcessMemory, String> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|error| format!("could not read /proc/self/status for RSS evidence: {error}"))?;
    Ok(ProcessMemory {
        rss_kib: status_value_kib(&status, "VmRSS:")?,
        high_water_kib: status_value_kib(&status, "VmHWM:")?,
    })
}

pub(super) fn status_value_kib(status: &str, key: &str) -> Result<u64, String> {
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
    let expected_protocol = usize::try_from(PROTOCOL_VERSION).expect("protocol version fits usize");
    let expected_data = usize::try_from(DATA_VERSION).expect("data version fits usize");
    if minecraft != MINECRAFT_VERSION
        || protocol != expected_protocol
        || data != expected_data
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

pub(super) fn canonical_usize(raw: &str, label: &str) -> Result<usize, String> {
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
