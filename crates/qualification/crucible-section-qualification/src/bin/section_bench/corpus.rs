mod parser;
mod verify;

#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crucible_generated::{
    BLOCK_STATE_COUNT, BlockStateId, STATE_DATA_GENERATION_SHA256, STATE_DATA_INPUT_SHA256,
};
use crucible_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};

const PARSER_ADMISSION_EXTRACTOR: &str = "vanilla-save-region-v1-stored-sections";
const REPRESENTATIVE_MEMBER_EXTRACTOR: &str = "vanilla-save-region-v2-representative-member";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CorpusPurpose {
    ParserAdmission,
    RepresentativeMember,
    Unclassified,
}

impl CorpusPurpose {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::ParserAdmission => "parser-admission",
            Self::RepresentativeMember => "representative-member",
            Self::Unclassified => "unclassified",
        }
    }

    pub(crate) const fn decision_eligible(&self) -> bool {
        match self {
            Self::ParserAdmission | Self::RepresentativeMember | Self::Unclassified => false,
        }
    }

    fn from_extractor(extractor: &str) -> Self {
        match extractor {
            PARSER_ADMISSION_EXTRACTOR => Self::ParserAdmission,
            REPRESENTATIVE_MEMBER_EXTRACTOR => Self::RepresentativeMember,
            _ => Self::Unclassified,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CorpusHeader {
    pub(crate) inventory_sha256: String,
    pub(crate) extractor: String,
    pub(crate) purpose: CorpusPurpose,
}

impl CorpusHeader {
    pub(crate) const fn decision_eligible(&self) -> bool {
        self.purpose.decision_eligible()
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SectionKey {
    pub(crate) dimension: String,
    pub(crate) chunk_x: i64,
    pub(crate) chunk_z: i64,
    pub(crate) section_y: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct CorpusSection {
    pub(crate) key: SectionKey,
    pub(crate) states: Box<[BlockStateId]>,
    pub(crate) cardinality: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct CandidateImportSummary {
    pub(crate) candidate: &'static str,
    pub(crate) production_candidate: bool,
    pub(crate) sections: usize,
    pub(crate) total_owned_bytes: usize,
    pub(crate) max_owned_bytes: usize,
    pub(crate) construction_transitions: usize,
    pub(crate) logical_backing_allocations: usize,
    pub(crate) representations: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct DimensionImportSummary {
    pub(crate) section_count: usize,
    pub(crate) total_cells: usize,
    pub(crate) distinct_state_ids: usize,
    pub(crate) cardinality_histogram: BTreeMap<usize, usize>,
    pub(crate) candidates: Vec<CandidateImportSummary>,
}

#[derive(Clone, Debug)]
pub(crate) struct CorpusImportCheck {
    pub(crate) header: CorpusHeader,
    pub(crate) section_count: usize,
    pub(crate) total_cells: usize,
    pub(crate) distinct_state_ids: usize,
    pub(crate) cardinality_histogram: BTreeMap<usize, usize>,
    pub(crate) dimensions: BTreeMap<String, usize>,
    pub(crate) per_dimension: BTreeMap<String, DimensionImportSummary>,
    pub(crate) candidates: Vec<CandidateImportSummary>,
}

impl CorpusImportCheck {
    pub(crate) fn to_json(&self, decision_requested: bool) -> String {
        let mut output = String::new();
        writeln!(output, "{{").expect("writing to String cannot fail");
        writeln!(output, "  \"schema\": 1,").expect("writing to String cannot fail");
        writeln!(output, "  \"kind\": \"section-corpus-import-check\",")
            .expect("writing to String cannot fail");
        writeln!(output, "  \"minecraft_version\": \"{MINECRAFT_VERSION}\",")
            .expect("writing to String cannot fail");
        writeln!(output, "  \"protocol_version\": {PROTOCOL_VERSION},")
            .expect("writing to String cannot fail");
        writeln!(output, "  \"data_version\": {DATA_VERSION},")
            .expect("writing to String cannot fail");
        writeln!(output, "  \"state_count\": {BLOCK_STATE_COUNT},")
            .expect("writing to String cannot fail");
        writeln!(
            output,
            "  \"state_data_generation_sha256\": \"{STATE_DATA_GENERATION_SHA256}\","
        )
        .expect("writing to String cannot fail");
        writeln!(
            output,
            "  \"state_data_input_sha256\": \"{STATE_DATA_INPUT_SHA256}\","
        )
        .expect("writing to String cannot fail");
        writeln!(
            output,
            "  \"source_inventory_sha256\": \"{}\",",
            self.header.inventory_sha256
        )
        .expect("writing to String cannot fail");
        writeln!(output, "  \"extractor\": \"{}\",", self.header.extractor)
            .expect("writing to String cannot fail");
        writeln!(
            output,
            "  \"purpose\": \"{}\",",
            self.header.purpose.as_str()
        )
        .expect("writing to String cannot fail");
        writeln!(output, "  \"decision_requested\": {decision_requested},")
            .expect("writing to String cannot fail");
        writeln!(
            output,
            "  \"decision_eligible\": {},",
            self.header.decision_eligible()
        )
        .expect("writing to String cannot fail");
        writeln!(output, "  \"section_count\": {},", self.section_count)
            .expect("writing to String cannot fail");
        writeln!(output, "  \"total_cells\": {},", self.total_cells)
            .expect("writing to String cannot fail");
        writeln!(
            output,
            "  \"distinct_state_ids\": {},",
            self.distinct_state_ids
        )
        .expect("writing to String cannot fail");
        write_usize_map(
            &mut output,
            "cardinality_histogram",
            &self.cardinality_histogram,
            2,
        );
        output.push_str(",\n");
        write_string_usize_map(&mut output, "dimensions", &self.dimensions, 2);
        output.push_str(",\n");
        write_dimension_summaries(&mut output, &self.per_dimension, 2);
        output.push_str(",\n  \"candidates\": ");
        write_candidate_array(&mut output, &self.candidates, 2);
        output.push_str("\n}\n");
        output
    }
}

pub(crate) fn check_corpus(
    path: &Path,
    decision_requested: bool,
) -> Result<CorpusImportCheck, String> {
    let verified = verify::verify_corpus(path, decision_requested)?;
    Ok(CorpusImportCheck {
        header: verified.header,
        section_count: verified.section_count,
        total_cells: verified.total_cells,
        distinct_state_ids: verified.distinct_state_ids,
        cardinality_histogram: verified.cardinality_histogram,
        dimensions: verified.dimensions,
        per_dimension: verified.per_dimension,
        candidates: verified.candidates,
    })
}

fn write_dimension_summaries(
    output: &mut String,
    values: &BTreeMap<String, DimensionImportSummary>,
    indent: usize,
) {
    let prefix = " ".repeat(indent);
    writeln!(output, "{prefix}\"per_dimension\": {{").expect("writing to String cannot fail");
    for (index, (dimension, summary)) in values.iter().enumerate() {
        let entry_prefix = " ".repeat(indent + 2);
        writeln!(output, "{entry_prefix}\"{dimension}\": {{")
            .expect("writing to String cannot fail");
        writeln!(
            output,
            "{entry_prefix}  \"section_count\": {},",
            summary.section_count
        )
        .expect("writing to String cannot fail");
        writeln!(
            output,
            "{entry_prefix}  \"total_cells\": {},",
            summary.total_cells
        )
        .expect("writing to String cannot fail");
        writeln!(
            output,
            "{entry_prefix}  \"distinct_state_ids\": {},",
            summary.distinct_state_ids
        )
        .expect("writing to String cannot fail");
        write_usize_map(
            output,
            "cardinality_histogram",
            &summary.cardinality_histogram,
            indent + 4,
        );
        output.push_str(",\n");
        write!(output, "{entry_prefix}  \"candidates\": ")
            .expect("writing to String cannot fail");
        write_candidate_array(output, &summary.candidates, indent + 4);
        output.push('\n');
        write!(output, "{entry_prefix}}}").expect("writing to String cannot fail");
        if index + 1 != values.len() {
            output.push(',');
        }
        output.push('\n');
    }
    write!(output, "{prefix}}}").expect("writing to String cannot fail");
}

fn write_candidate_array(output: &mut String, candidates: &[CandidateImportSummary], indent: usize) {
    output.push_str("[\n");
    let item_prefix = " ".repeat(indent + 2);
    for (index, candidate) in candidates.iter().enumerate() {
        write!(
            output,
            "{item_prefix}{{\"candidate\":\"{}\",\"production_candidate\":{},\"sections\":{},\"total_owned_bytes\":{},\"max_owned_bytes\":{},\"construction_transitions\":{},\"logical_backing_allocations\":{},\"representations\":",
            candidate.candidate,
            candidate.production_candidate,
            candidate.sections,
            candidate.total_owned_bytes,
            candidate.max_owned_bytes,
            candidate.construction_transitions,
            candidate.logical_backing_allocations,
        )
        .expect("writing to String cannot fail");
        write_inline_string_usize_map(output, &candidate.representations);
        output.push('}');
        if index + 1 != candidates.len() {
            output.push(',');
        }
        output.push('\n');
    }
    let prefix = " ".repeat(indent);
    write!(output, "{prefix}]").expect("writing to String cannot fail");
}

fn write_usize_map(
    output: &mut String,
    name: &str,
    values: &BTreeMap<usize, usize>,
    indent: usize,
) {
    let prefix = " ".repeat(indent);
    write!(output, "{prefix}\"{name}\": {{").expect("writing to String cannot fail");
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "\"{key}\":{value}").expect("writing to String cannot fail");
    }
    output.push('}');
}

fn write_string_usize_map(
    output: &mut String,
    name: &str,
    values: &BTreeMap<String, usize>,
    indent: usize,
) {
    let prefix = " ".repeat(indent);
    write!(output, "{prefix}\"{name}\": ").expect("writing to String cannot fail");
    write_inline_string_usize_map(output, values);
}

fn write_inline_string_usize_map(output: &mut String, values: &BTreeMap<String, usize>) {
    output.push('{');
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "\"{key}\":{value}").expect("writing to String cannot fail");
    }
    output.push('}');
}

#[cfg(test)]
mod purpose_tests {
    use super::{CorpusPurpose, PARSER_ADMISSION_EXTRACTOR, REPRESENTATIVE_MEMBER_EXTRACTOR};

    #[test]
    fn representative_members_are_known_but_not_individually_decision_eligible() {
        let parser = CorpusPurpose::from_extractor(PARSER_ADMISSION_EXTRACTOR);
        assert_eq!(parser, CorpusPurpose::ParserAdmission);
        assert_eq!(parser.as_str(), "parser-admission");
        assert!(!parser.decision_eligible());

        let representative = CorpusPurpose::from_extractor(REPRESENTATIVE_MEMBER_EXTRACTOR);
        assert_eq!(representative, CorpusPurpose::RepresentativeMember);
        assert_eq!(representative.as_str(), "representative-member");
        assert!(!representative.decision_eligible());

        let unknown = CorpusPurpose::from_extractor("future-policy-v99");
        assert_eq!(unknown, CorpusPurpose::Unclassified);
        assert!(!unknown.decision_eligible());
    }
}
