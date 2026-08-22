use std::collections::BTreeMap;
use std::path::Path;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionSummary};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::BenchSection;
use crate::workloads::pos;

use super::parser::CorpusReader;
use super::{CandidateImportSummary, CorpusHeader, CorpusSection, DimensionImportSummary};

#[derive(Debug)]
pub(super) struct VerifiedCorpus {
    pub(super) header: CorpusHeader,
    pub(super) section_count: usize,
    pub(super) total_cells: usize,
    pub(super) distinct_state_ids: usize,
    pub(super) cardinality_histogram: BTreeMap<usize, usize>,
    pub(super) dimensions: BTreeMap<String, usize>,
    pub(super) per_dimension: BTreeMap<String, DimensionImportSummary>,
    pub(super) candidates: Vec<CandidateImportSummary>,
}

pub(super) fn verify_corpus(
    path: &Path,
    decision_requested: bool,
) -> Result<VerifiedCorpus, String> {
    let mut reader = CorpusReader::open(path)?;
    let header = reader.header().clone();
    if decision_requested && !header.decision_eligible() {
        return Err(format!(
            "corpus extractor {} has purpose {} and is not decision-eligible",
            header.extractor,
            header.purpose.as_str()
        ));
    }

    let mut overall_observed_states = vec![false; BLOCK_STATE_COUNT];
    let mut dimension_accumulators: BTreeMap<String, DimensionAccumulator> = BTreeMap::new();

    while let Some(section) = reader.next_section()? {
        for state in &section.states {
            overall_observed_states[state.as_usize()] = true;
        }
        let expected_summary = recompute_section_summary(&section);
        dimension_accumulators
            .entry(section.key.dimension.clone())
            .or_insert_with(DimensionAccumulator::new)
            .record(&section, expected_summary)?;
    }

    if dimension_accumulators.is_empty() {
        return Err("corpus must contain at least one section".to_owned());
    }

    let per_dimension = dimension_accumulators
        .into_iter()
        .map(|(dimension, accumulator)| (dimension, accumulator.finish()))
        .collect::<BTreeMap<_, _>>();

    let mut section_count = 0_usize;
    let mut cardinality_histogram = BTreeMap::new();
    let mut dimensions = BTreeMap::new();
    for (dimension, summary) in &per_dimension {
        section_count = section_count
            .checked_add(summary.section_count)
            .ok_or_else(|| "section count overflow".to_owned())?;
        dimensions.insert(dimension.clone(), summary.section_count);
        for (cardinality, count) in &summary.cardinality_histogram {
            let entry = cardinality_histogram.entry(*cardinality).or_insert(0_usize);
            *entry = entry
                .checked_add(*count)
                .ok_or_else(|| "cardinality histogram count overflow".to_owned())?;
        }
    }

    let total_cells = section_count
        .checked_mul(BLOCK_SECTION_CELLS)
        .ok_or_else(|| "corpus cell count overflow".to_owned())?;
    let distinct_state_ids = overall_observed_states
        .into_iter()
        .filter(|present| *present)
        .count();
    let candidates = aggregate_candidate_summaries(&per_dimension)?;

    Ok(VerifiedCorpus {
        header,
        section_count,
        total_cells,
        distinct_state_ids,
        cardinality_histogram,
        dimensions,
        per_dimension,
        candidates,
    })
}

fn recompute_section_summary(section: &CorpusSection) -> SectionSummary {
    let mut non_air_count = 0_u16;
    let mut fluid_count = 0_u16;
    let mut random_block_present = false;
    let mut random_fluid_present = false;

    for state in section.states.iter().copied() {
        let facts = <GeneratedStateFacts as BlockStateFacts<BlockStateId>>::facts(
            &GeneratedStateFacts,
            state,
        );
        non_air_count += u16::from(facts.non_air());
        fluid_count += u16::from(facts.counted_fluid());
        random_block_present |= facts.random_block();
        random_fluid_present |= facts.random_fluid();
    }

    SectionSummary {
        non_air_count,
        fluid_count,
        random_block_present,
        random_fluid_present,
    }
}

#[derive(Debug)]
struct DimensionAccumulator {
    section_count: usize,
    observed_states: Vec<bool>,
    cardinality_histogram: BTreeMap<usize, usize>,
    direct_reference: CandidateAccumulator,
    direct: CandidateAccumulator,
    adaptive: CandidateAccumulator,
    fast_local: CandidateAccumulator,
    packed_local: CandidateAccumulator,
}

impl DimensionAccumulator {
    fn new() -> Self {
        Self {
            section_count: 0,
            observed_states: vec![false; BLOCK_STATE_COUNT],
            cardinality_histogram: BTreeMap::new(),
            direct_reference: CandidateAccumulator::new::<DirectBlockSection<BlockStateId>>(),
            direct: CandidateAccumulator::new::<DirectNBlockSection<BlockStateId>>(),
            adaptive: CandidateAccumulator::new::<AdaptiveBlockSection<BlockStateId>>(),
            fast_local: CandidateAccumulator::new::<FastLocalBlockSection<BlockStateId>>(),
            packed_local: CandidateAccumulator::new::<PackedLocalBlockSection<BlockStateId>>(),
        }
    }

    fn record(
        &mut self,
        section: &CorpusSection,
        expected_summary: SectionSummary,
    ) -> Result<(), String> {
        self.section_count = self
            .section_count
            .checked_add(1)
            .ok_or_else(|| "dimension section count overflow".to_owned())?;
        let histogram_entry = self
            .cardinality_histogram
            .entry(section.cardinality)
            .or_insert(0_usize);
        *histogram_entry = histogram_entry
            .checked_add(1)
            .ok_or_else(|| "dimension cardinality histogram count overflow".to_owned())?;
        for state in &section.states {
            self.observed_states[state.as_usize()] = true;
        }

        self.direct_reference
            .record::<DirectBlockSection<BlockStateId>>(section, expected_summary)?;
        self.direct
            .record::<DirectNBlockSection<BlockStateId>>(section, expected_summary)?;
        self.adaptive
            .record::<AdaptiveBlockSection<BlockStateId>>(section, expected_summary)?;
        self.fast_local
            .record::<FastLocalBlockSection<BlockStateId>>(section, expected_summary)?;
        self.packed_local
            .record::<PackedLocalBlockSection<BlockStateId>>(section, expected_summary)?;
        Ok(())
    }

    fn finish(self) -> DimensionImportSummary {
        let distinct_state_ids = self
            .observed_states
            .into_iter()
            .filter(|present| *present)
            .count();
        DimensionImportSummary {
            section_count: self.section_count,
            total_cells: self.section_count * BLOCK_SECTION_CELLS,
            distinct_state_ids,
            cardinality_histogram: self.cardinality_histogram,
            candidates: vec![
                self.direct_reference.finish(),
                self.direct.finish(),
                self.adaptive.finish(),
                self.fast_local.finish(),
                self.packed_local.finish(),
            ],
        }
    }
}

fn aggregate_candidate_summaries(
    per_dimension: &BTreeMap<String, DimensionImportSummary>,
) -> Result<Vec<CandidateImportSummary>, String> {
    let mut direct_reference = CandidateAccumulator::new::<DirectBlockSection<BlockStateId>>();
    let mut direct = CandidateAccumulator::new::<DirectNBlockSection<BlockStateId>>();
    let mut adaptive = CandidateAccumulator::new::<AdaptiveBlockSection<BlockStateId>>();
    let mut fast_local = CandidateAccumulator::new::<FastLocalBlockSection<BlockStateId>>();
    let mut packed_local = CandidateAccumulator::new::<PackedLocalBlockSection<BlockStateId>>();

    for summary in per_dimension.values() {
        for candidate in &summary.candidates {
            match candidate.candidate {
                DirectBlockSection::<BlockStateId>::NAME => direct_reference.merge(candidate)?,
                DirectNBlockSection::<BlockStateId>::NAME => direct.merge(candidate)?,
                AdaptiveBlockSection::<BlockStateId>::NAME => adaptive.merge(candidate)?,
                FastLocalBlockSection::<BlockStateId>::NAME => fast_local.merge(candidate)?,
                PackedLocalBlockSection::<BlockStateId>::NAME => packed_local.merge(candidate)?,
                other => return Err(format!("unknown candidate in dimension summary: {other}")),
            }
        }
    }

    Ok(vec![
        direct_reference.finish(),
        direct.finish(),
        adaptive.finish(),
        fast_local.finish(),
        packed_local.finish(),
    ])
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

    fn record<C: BenchSection>(
        &mut self,
        section: &CorpusSection,
        expected_summary: SectionSummary,
    ) -> Result<(), String> {
        let inspected = inspect_candidate_section::<C>(section, expected_summary)?;
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
        let representation_entry = self
            .representations
            .entry(inspected.representation)
            .or_insert(0_usize);
        *representation_entry = representation_entry
            .checked_add(1)
            .ok_or_else(|| format!("{} representation count overflow", C::NAME))?;
        Ok(())
    }

    fn merge(&mut self, summary: &CandidateImportSummary) -> Result<(), String> {
        if summary.candidate != self.candidate
            || summary.production_candidate != self.production_candidate
        {
            return Err(format!(
                "candidate summary identity mismatch: expected {}, got {}",
                self.candidate, summary.candidate
            ));
        }
        self.sections = self
            .sections
            .checked_add(summary.sections)
            .ok_or_else(|| format!("{} section count overflow", self.candidate))?;
        self.total_owned_bytes = self
            .total_owned_bytes
            .checked_add(summary.total_owned_bytes)
            .ok_or_else(|| format!("{} total owned-byte count overflow", self.candidate))?;
        self.max_owned_bytes = self.max_owned_bytes.max(summary.max_owned_bytes);
        self.construction_transitions = self
            .construction_transitions
            .checked_add(summary.construction_transitions)
            .ok_or_else(|| format!("{} transition count overflow", self.candidate))?;
        self.logical_backing_allocations = self
            .logical_backing_allocations
            .checked_add(summary.logical_backing_allocations)
            .ok_or_else(|| format!("{} logical allocation count overflow", self.candidate))?;
        for (representation, count) in &summary.representations {
            let entry = self
                .representations
                .entry(representation.clone())
                .or_insert(0_usize);
            *entry = entry
                .checked_add(*count)
                .ok_or_else(|| format!("{} representation count overflow", self.candidate))?;
        }
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
    expected_summary: SectionSummary,
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

    let actual_summary = <C as BlockSection<BlockStateId>>::summary(&candidate);
    if actual_summary != expected_summary {
        return Err(format!(
            "{} corpus summary mismatch at {:?}: expected {:?}, got {:?}",
            C::NAME,
            section.key,
            expected_summary,
            actual_summary
        ));
    }

    Ok(InspectedCandidate {
        owned_bytes: candidate.owned_bytes(),
        transitions,
        logical_allocations,
        representation: candidate.representation_name(),
    })
}

#[cfg(test)]
pub(super) fn recompute_section_summary_for_test(section: &CorpusSection) -> SectionSummary {
    recompute_section_summary(section)
}
