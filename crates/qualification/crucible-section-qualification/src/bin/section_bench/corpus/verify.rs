use std::collections::BTreeMap;
use std::path::Path;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
use crucible_world_contract::BLOCK_SECTION_CELLS;
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::BenchSection;
use crate::workloads::pos;

use super::parser::CorpusReader;
use super::{CandidateImportSummary, CorpusHeader, CorpusSection};

#[derive(Debug)]
pub(super) struct VerifiedCorpus {
    pub(super) header: CorpusHeader,
    pub(super) section_count: usize,
    pub(super) total_cells: usize,
    pub(super) distinct_state_ids: usize,
    pub(super) cardinality_histogram: BTreeMap<usize, usize>,
    pub(super) dimensions: BTreeMap<String, usize>,
    pub(super) candidates: Vec<CandidateImportSummary>,
}

pub(super) fn read_header(path: &Path) -> Result<CorpusHeader, String> {
    let reader = CorpusReader::open(path)?;
    Ok(reader.header().clone())
}

pub(super) fn verify_corpus(path: &Path) -> Result<VerifiedCorpus, String> {
    let mut reader = CorpusReader::open(path)?;
    let header = reader.header().clone();
    let mut section_count = 0_usize;
    let mut observed_states = vec![false; BLOCK_STATE_COUNT];
    let mut cardinality_histogram = BTreeMap::new();
    let mut dimensions = BTreeMap::new();

    let mut direct_reference = CandidateAccumulator::new::<DirectBlockSection<BlockStateId>>();
    let mut direct = CandidateAccumulator::new::<DirectNBlockSection<BlockStateId>>();
    let mut adaptive = CandidateAccumulator::new::<AdaptiveBlockSection<BlockStateId>>();
    let mut fast_local = CandidateAccumulator::new::<FastLocalBlockSection<BlockStateId>>();
    let mut packed_local = CandidateAccumulator::new::<PackedLocalBlockSection<BlockStateId>>();

    while let Some(section) = reader.next_section()? {
        section_count = section_count
            .checked_add(1)
            .ok_or_else(|| "section count overflow".to_owned())?;
        *cardinality_histogram
            .entry(section.cardinality)
            .or_insert(0) += 1;
        *dimensions.entry(section.key.dimension.clone()).or_insert(0) += 1;
        for state in &section.states {
            observed_states[state.as_usize()] = true;
        }

        direct_reference.record::<DirectBlockSection<BlockStateId>>(&section)?;
        direct.record::<DirectNBlockSection<BlockStateId>>(&section)?;
        adaptive.record::<AdaptiveBlockSection<BlockStateId>>(&section)?;
        fast_local.record::<FastLocalBlockSection<BlockStateId>>(&section)?;
        packed_local.record::<PackedLocalBlockSection<BlockStateId>>(&section)?;
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
    let candidates = vec![
        direct_reference.finish(),
        direct.finish(),
        adaptive.finish(),
        fast_local.finish(),
        packed_local.finish(),
    ];
    if candidates.iter().any(|candidate| candidate.sections != section_count) {
        return Err("candidate importer section counts diverged".to_owned());
    }

    Ok(VerifiedCorpus {
        header,
        section_count,
        total_cells,
        distinct_state_ids,
        cardinality_histogram,
        dimensions,
        candidates,
    })
}

#[derive(Debug)]
struct CandidateAccumulator {
    candidate: &'static str,
    production_candidate: bool,
    sections: usize,
    total_owned_bytes: usize,
    max_owned_bytes: usize,
    construction_transitions: usize,
    logical_backing_allocations: usize,
    representations: BTreeMap<String, usize>,
}

impl CandidateAccumulator {
    fn new<C: BenchSection>() -> Self {
        Self {
            candidate: C::NAME,
            production_candidate: C::PRODUCTION_CANDIDATE,
            sections: 0,
            total_owned_bytes: 0,
            max_owned_bytes: 0,
            construction_transitions: 0,
            logical_backing_allocations: 0,
            representations: BTreeMap::new(),
        }
    }

    fn record<C: BenchSection>(&mut self, section: &CorpusSection) -> Result<(), String> {
        let inspected = inspect_candidate_section::<C>(section)?;
        self.sections = self
            .sections
            .checked_add(1)
            .ok_or_else(|| format!("{} section count overflow", C::NAME))?;
        self.total_owned_bytes = self
            .total_owned_bytes
            .checked_add(inspected.owned_bytes)
            .ok_or_else(|| format!("{} total owned-byte count overflow", C::NAME))?;
        self.max_owned_bytes = self.max_owned_bytes.max(inspected.owned_bytes);
        self.construction_transitions = self
            .construction_transitions
            .checked_add(inspected.transitions)
            .ok_or_else(|| format!("{} transition count overflow", C::NAME))?;
        self.logical_backing_allocations = self
            .logical_backing_allocations
            .checked_add(inspected.logical_allocations)
            .ok_or_else(|| format!("{} logical allocation count overflow", C::NAME))?;
        *self
            .representations
            .entry(inspected.representation)
            .or_insert(0) += 1;
        Ok(())
    }

    fn finish(self) -> CandidateImportSummary {
        CandidateImportSummary {
            candidate: self.candidate,
            production_candidate: self.production_candidate,
            sections: self.sections,
            total_owned_bytes: self.total_owned_bytes,
            max_owned_bytes: self.max_owned_bytes,
            construction_transitions: self.construction_transitions,
            logical_backing_allocations: self.logical_backing_allocations,
            representations: self.representations,
        }
    }
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
        if state == first {
            continue;
        }
        let before = candidate.representation_code();
        let _ = candidate.replace(pos(cell), state, &GeneratedStateFacts);
        let after = candidate.representation_code();
        if before != after {
            transitions += 1;
            logical_allocations = logical_allocations
                .checked_add(C::transition_logical_allocations(before, after))
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
