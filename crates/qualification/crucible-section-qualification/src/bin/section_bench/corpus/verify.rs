use std::collections::BTreeMap;
use std::path::Path;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::BenchSection;
use crate::workloads::pos;

use super::parser::CorpusReader;
use super::{CandidateImportSummary, CorpusHeader, CorpusSection};

#[derive(Debug)]
pub(super) struct MetadataSummary {
    pub(super) header: CorpusHeader,
    pub(super) section_count: usize,
    pub(super) total_cells: usize,
    pub(super) distinct_state_ids: usize,
    pub(super) cardinality_histogram: BTreeMap<usize, usize>,
    pub(super) dimensions: BTreeMap<String, usize>,
}

pub(super) fn scan_metadata(path: &Path) -> Result<MetadataSummary, String> {
    let mut reader = CorpusReader::open(path)?;
    let header = reader.header().clone();
    let mut section_count = 0_usize;
    let mut observed_states = vec![false; BLOCK_STATE_COUNT];
    let mut cardinality_histogram = BTreeMap::new();
    let mut dimensions = BTreeMap::new();

    while let Some(section) = reader.next_section()? {
        section_count = section_count
            .checked_add(1)
            .ok_or_else(|| "section count overflow".to_owned())?;
        *cardinality_histogram
            .entry(section.cardinality)
            .or_insert(0) += 1;
        *dimensions.entry(section.key.dimension).or_insert(0) += 1;
        for state in &section.states {
            observed_states[state.as_usize()] = true;
        }
    }
    if section_count == 0 {
        return Err("corpus must contain at least one section".to_owned());
    }

    let total_cells = section_count
        .checked_mul(BLOCK_SECTION_CELLS)
        .ok_or_else(|| "corpus cell count overflow".to_owned())?;
    let distinct_state_ids = observed_states
        .into_iter()
        .filter(|present| *present)
        .count();

    Ok(MetadataSummary {
        header,
        section_count,
        total_cells,
        distinct_state_ids,
        cardinality_histogram,
        dimensions,
    })
}

pub(super) fn check_all_candidates(
    path: &Path,
    expected_header: &CorpusHeader,
) -> Result<Vec<CandidateImportSummary>, String> {
    Ok(vec![
        check_candidate::<DirectBlockSection<BlockStateId>>(path, expected_header)?,
        check_candidate::<DirectNBlockSection<BlockStateId>>(path, expected_header)?,
        check_candidate::<AdaptiveBlockSection<BlockStateId>>(path, expected_header)?,
        check_candidate::<FastLocalBlockSection<BlockStateId>>(path, expected_header)?,
        check_candidate::<PackedLocalBlockSection<BlockStateId>>(path, expected_header)?,
    ])
}

fn check_candidate<C: BenchSection>(
    path: &Path,
    expected_header: &CorpusHeader,
) -> Result<CandidateImportSummary, String> {
    let mut reader = CorpusReader::open(path)?;
    if reader.header() != expected_header {
        return Err("corpus header changed between importer passes".to_owned());
    }

    let mut sections = 0_usize;
    let mut total_owned_bytes = 0_usize;
    let mut max_owned_bytes = 0_usize;
    let mut construction_transitions = 0_usize;
    let mut logical_backing_allocations = 0_usize;
    let mut representations = BTreeMap::new();

    while let Some(section) = reader.next_section()? {
        let inspected = inspect_candidate_section::<C>(&section)?;
        sections = sections
            .checked_add(1)
            .ok_or_else(|| format!("{} section count overflow", C::NAME))?;
        total_owned_bytes = total_owned_bytes
            .checked_add(inspected.owned_bytes)
            .ok_or_else(|| format!("{} total owned-byte count overflow", C::NAME))?;
        max_owned_bytes = max_owned_bytes.max(inspected.owned_bytes);
        construction_transitions = construction_transitions
            .checked_add(inspected.transitions)
            .ok_or_else(|| format!("{} transition count overflow", C::NAME))?;
        logical_backing_allocations = logical_backing_allocations
            .checked_add(inspected.logical_allocations)
            .ok_or_else(|| format!("{} logical allocation count overflow", C::NAME))?;
        *representations.entry(inspected.representation).or_insert(0) += 1;
    }

    Ok(CandidateImportSummary {
        candidate: C::NAME,
        production_candidate: C::PRODUCTION_CANDIDATE,
        sections,
        total_owned_bytes,
        max_owned_bytes,
        construction_transitions,
        logical_backing_allocations,
        representations,
    })
}

#[derive(Debug)]
pub(super) struct InspectedCandidate {
    pub(super) owned_bytes: usize,
    pub(super) transitions: usize,
    pub(super) logical_allocations: usize,
    pub(super) representation: String,
}

pub(super) fn inspect_candidate_section<C: BenchSection>(
    section: &CorpusSection,
) -> Result<InspectedCandidate, String> {
    let first = *section
        .states
        .first()
        .ok_or_else(|| "corpus section unexpectedly has no cells".to_owned())?;
    let mut candidate = C::filled(first);
    let mut transitions = 0_usize;
    let mut logical_allocations = C::initial_logical_allocations();

    for (cell, state) in section.states.iter().copied().enumerate().skip(1) {
        let before = candidate.representation_name();
        let _ = candidate.replace(pos(cell), state, &GeneratedStateFacts);
        let after = candidate.representation_name();
        if before != after {
            transitions += 1;
            logical_allocations = logical_allocations
                .checked_add(C::transition_logical_allocations(&before, &after))
                .ok_or_else(|| format!("{} logical allocation count overflow", C::NAME))?;
        }
    }

    for (cell, expected) in section.states.iter().copied().enumerate() {
        let actual = candidate.get(pos(cell));
        if actual != expected {
            return Err(format!(
                "{} corpus reconstruction mismatch at {:?} cell {cell}: expected state {}, got {}",
                C::NAME,
                section.key,
                expected.as_usize(),
                actual.as_usize(),
            ));
        }
    }

    Ok(InspectedCandidate {
        owned_bytes: candidate.owned_bytes(),
        transitions,
        logical_allocations,
        representation: candidate.representation_name(),
    })
}
