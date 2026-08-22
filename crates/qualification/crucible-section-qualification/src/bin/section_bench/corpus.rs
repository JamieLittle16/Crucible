use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crucible_generated::{
    BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
};
use crucible_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::BenchSection;
use crate::workloads::pos;

const MAGIC: &str = "CRUCIBLE-SECTION-CORPUS|1";
const SOURCE_KIND: &str = "vanilla-save";
const PARSER_ADMISSION_EXTRACTOR: &str = "vanilla-save-region-v1-stored-sections";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CorpusPurpose {
    ParserAdmission,
    Unclassified,
}

impl CorpusPurpose {
    pub(crate) const fn as_str(&self) -> &'static str {
        match self {
            Self::ParserAdmission => "parser-admission",
            Self::Unclassified => "unclassified",
        }
    }

    pub(crate) const fn decision_eligible(&self) -> bool {
        false
    }

    fn from_extractor(extractor: &str) -> Self {
        if extractor == PARSER_ADMISSION_EXTRACTOR {
            Self::ParserAdmission
        } else {
            Self::Unclassified
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
pub(crate) struct CorpusImportCheck {
    pub(crate) header: CorpusHeader,
    pub(crate) section_count: usize,
    pub(crate) total_cells: usize,
    pub(crate) distinct_state_ids: usize,
    pub(crate) cardinality_histogram: BTreeMap<usize, usize>,
    pub(crate) dimensions: BTreeMap<String, usize>,
    pub(crate) candidates: Vec<CandidateImportSummary>,
}

impl CorpusImportCheck {
    pub(crate) fn to_json(&self, decision_requested: bool) -> String {
        let mut output = String::new();
        output.push_str("{\n");
        output.push_str("  \"schema\": 1,\n");
        output.push_str("  \"kind\": \"section-corpus-import-check\",\n");
        output.push_str(&format!(
            "  \"minecraft_version\": \"{MINECRAFT_VERSION}\",\n"
        ));
        output.push_str(&format!("  \"protocol_version\": {PROTOCOL_VERSION},\n"));
        output.push_str(&format!("  \"data_version\": {DATA_VERSION},\n"));
        output.push_str(&format!("  \"state_count\": {BLOCK_STATE_COUNT},\n"));
        output.push_str(&format!(
            "  \"state_data_generation_sha256\": \"{STATE_DATA_GENERATION_SHA256}\",\n"
        ));
        output.push_str(&format!(
            "  \"source_inventory_sha256\": \"{}\",\n",
            self.header.inventory_sha256
        ));
        output.push_str(&format!(
            "  \"extractor\": \"{}\",\n",
            self.header.extractor
        ));
        output.push_str(&format!(
            "  \"purpose\": \"{}\",\n",
            self.header.purpose.as_str()
        ));
        output.push_str(&format!(
            "  \"decision_requested\": {decision_requested},\n"
        ));
        output.push_str(&format!(
            "  \"decision_eligible\": {},\n",
            self.header.decision_eligible()
        ));
        output.push_str(&format!("  \"section_count\": {},\n", self.section_count));
        output.push_str(&format!("  \"total_cells\": {},\n", self.total_cells));
        output.push_str(&format!(
            "  \"distinct_state_ids\": {},\n",
            self.distinct_state_ids
        ));
        write_usize_map(&mut output, "cardinality_histogram", &self.cardinality_histogram, 2);
        output.push_str(",\n");
        write_string_usize_map(&mut output, "dimensions", &self.dimensions, 2);
        output.push_str(",\n");
        output.push_str("  \"candidates\": [\n");
        for (index, candidate) in self.candidates.iter().enumerate() {
            output.push_str("    {");
            output.push_str(&format!(
                "\"candidate\":\"{}\",\"production_candidate\":{},\"sections\":{},\"total_owned_bytes\":{},\"max_owned_bytes\":{},\"construction_transitions\":{},\"logical_backing_allocations\":{},\"representations\":",
                candidate.candidate,
                candidate.production_candidate,
                candidate.sections,
                candidate.total_owned_bytes,
                candidate.max_owned_bytes,
                candidate.construction_transitions,
                candidate.logical_backing_allocations,
            ));
            write_inline_string_usize_map(&mut output, &candidate.representations);
            output.push('}');
            if index + 1 != self.candidates.len() {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("  ]\n}\n");
        output
    }
}

pub(crate) struct CorpusReader<R: BufRead> {
    reader: R,
    header: CorpusHeader,
    next_line_number: usize,
    previous_key: Option<SectionKey>,
}

impl CorpusReader<BufReader<File>> {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|error| format!("could not open corpus {}: {error}", path.display()))?;
        Self::from_reader(BufReader::new(file))
    }
}

impl<R: BufRead> CorpusReader<R> {
    pub(crate) fn from_reader(mut reader: R) -> Result<Self, String> {
        let magic = read_required_line(&mut reader, 1)?;
        if magic != MAGIC {
            return Err(format!("unsupported corpus magic/schema: {magic:?}"));
        }
        let target = read_required_line(&mut reader, 2)?;
        validate_target_header(&target)?;
        let source = read_required_line(&mut reader, 3)?;
        let header = parse_source_header(&source)?;
        Ok(Self {
            reader,
            header,
            next_line_number: 4,
            previous_key: None,
        })
    }

    pub(crate) fn header(&self) -> &CorpusHeader {
        &self.header
    }

    pub(crate) fn next_section(&mut self) -> Result<Option<CorpusSection>, String> {
        let line_number = self.next_line_number;
        let Some(line) = read_canonical_line(&mut self.reader, line_number)? else {
            return Ok(None);
        };
        self.next_line_number += 1;
        let section = parse_section_line(&line, line_number)?;
        if let Some(previous) = &self.previous_key
            && section.key <= *previous
        {
            let relation = if section.key == *previous {
                "duplicate"
            } else {
                "out of order"
            };
            return Err(format!(
                "line {line_number}: section coordinate is {relation}: {:?}",
                section.key
            ));
        }
        self.previous_key = Some(section.key.clone());
        Ok(Some(section))
    }
}

pub(crate) fn check_corpus(path: &Path, decision_requested: bool) -> Result<CorpusImportCheck, String> {
    let metadata = scan_metadata(path)?;
    if decision_requested && !metadata.header.decision_eligible() {
        return Err(format!(
            "corpus extractor {} has purpose {} and is not decision-eligible",
            metadata.header.extractor,
            metadata.header.purpose.as_str()
        ));
    }

    let candidates = vec![
        check_candidate::<DirectBlockSection<BlockStateId>>(path, &metadata.header)?,
        check_candidate::<DirectNBlockSection<BlockStateId>>(path, &metadata.header)?,
        check_candidate::<AdaptiveBlockSection<BlockStateId>>(path, &metadata.header)?,
        check_candidate::<FastLocalBlockSection<BlockStateId>>(path, &metadata.header)?,
        check_candidate::<PackedLocalBlockSection<BlockStateId>>(path, &metadata.header)?,
    ];

    Ok(CorpusImportCheck {
        header: metadata.header,
        section_count: metadata.section_count,
        total_cells: metadata.total_cells,
        distinct_state_ids: metadata.distinct_state_ids,
        cardinality_histogram: metadata.cardinality_histogram,
        dimensions: metadata.dimensions,
        candidates,
    })
}

#[derive(Debug)]
struct MetadataSummary {
    header: CorpusHeader,
    section_count: usize,
    total_cells: usize,
    distinct_state_ids: usize,
    cardinality_histogram: BTreeMap<usize, usize>,
    dimensions: BTreeMap<String, usize>,
}

fn scan_metadata(path: &Path) -> Result<MetadataSummary, String> {
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
        *cardinality_histogram.entry(section.cardinality).or_insert(0) += 1;
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
    let distinct_state_ids = observed_states.into_iter().filter(|present| *present).count();

    Ok(MetadataSummary {
        header,
        section_count,
        total_cells,
        distinct_state_ids,
        cardinality_histogram,
        dimensions,
    })
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
struct InspectedCandidate {
    owned_bytes: usize,
    transitions: usize,
    logical_allocations: usize,
    representation: String,
}

fn inspect_candidate_section<C: BenchSection>(section: &CorpusSection) -> Result<InspectedCandidate, String> {
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

fn validate_target_header(line: &str) -> Result<(), String> {
    let parts = line.split('|').collect::<Vec<_>>();
    if parts.len() != 6 || parts[0] != "TARGET" {
        return Err("TARGET header has the wrong shape".to_owned());
    }
    let minecraft = field(parts[1], "minecraft")?;
    let protocol = parse_canonical_u64(field(parts[2], "protocol")?, "protocol")?;
    let data = parse_canonical_u64(field(parts[3], "data")?, "data")?;
    let state_count = parse_canonical_u64(field(parts[4], "state_count")?, "state_count")?;
    let generation = field(parts[5], "generation_sha256")?;

    if minecraft != MINECRAFT_VERSION
        || protocol != u64::from(PROTOCOL_VERSION)
        || data != u64::from(DATA_VERSION)
        || state_count
            != u64::try_from(BLOCK_STATE_COUNT).expect("target state count fits in u64")
        || generation != STATE_DATA_GENERATION_SHA256
    {
        return Err("corpus TARGET header does not match frozen generated target data".to_owned());
    }
    Ok(())
}

fn parse_source_header(line: &str) -> Result<CorpusHeader, String> {
    let parts = line.split('|').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "SOURCE" {
        return Err("SOURCE header has the wrong shape".to_owned());
    }
    let kind = field(parts[1], "kind")?;
    if kind != SOURCE_KIND {
        return Err(format!("unsupported corpus source kind: {kind}"));
    }
    let inventory_sha256 = field(parts[2], "inventory_sha256")?;
    if !is_lower_sha256(inventory_sha256) {
        return Err("source inventory_sha256 must be lowercase SHA-256".to_owned());
    }
    let extractor = field(parts[3], "extractor")?;
    if !is_token(extractor) {
        return Err("source extractor identifier is not canonical".to_owned());
    }
    Ok(CorpusHeader {
        inventory_sha256: inventory_sha256.to_owned(),
        extractor: extractor.to_owned(),
        purpose: CorpusPurpose::from_extractor(extractor),
    })
}

fn parse_section_line(line: &str, line_number: usize) -> Result<CorpusSection, String> {
    let parts = line.splitn(6, '|').collect::<Vec<_>>();
    if parts.len() != 6 || parts[0] != "SECTION" {
        return Err(format!(
            "line {line_number}: expected SECTION record with six fields"
        ));
    }
    if !is_resource_location(parts[1]) {
        return Err(format!(
            "line {line_number}: invalid dimension resource location {:?}",
            parts[1]
        ));
    }
    let key = SectionKey {
        dimension: parts[1].to_owned(),
        chunk_x: parse_canonical_i64(parts[2], &format!("line {line_number} chunk_x"))?,
        chunk_z: parse_canonical_i64(parts[3], &format!("line {line_number} chunk_z"))?,
        section_y: parse_canonical_i64(parts[4], &format!("line {line_number} section_y"))?,
    };
    let raw_states = parts[5].split(',').collect::<Vec<_>>();
    if raw_states.len() != BLOCK_SECTION_CELLS {
        return Err(format!(
            "line {line_number}: section has {} cells; expected {BLOCK_SECTION_CELLS}",
            raw_states.len()
        ));
    }
    let mut states = Vec::with_capacity(BLOCK_SECTION_CELLS);
    let mut unique = BTreeSet::new();
    for (cell, raw) in raw_states.into_iter().enumerate() {
        let value = parse_canonical_u64(raw, &format!("line {line_number} cell {cell}"))?;
        let raw_u32 = u32::try_from(value).map_err(|_| {
            format!("line {line_number} cell {cell}: state ID {value} does not fit u32")
        })?;
        let state = BlockStateId::new(raw_u32).ok_or_else(|| {
            format!(
                "line {line_number} cell {cell}: state ID {value} outside 0..{}",
                BLOCK_STATE_COUNT - 1
            )
        })?;
        unique.insert(state);
        states.push(state);
    }

    Ok(CorpusSection {
        key,
        states: states.into_boxed_slice(),
        cardinality: unique.len(),
    })
}

fn read_required_line<R: BufRead>(reader: &mut R, line_number: usize) -> Result<String, String> {
    read_canonical_line(reader, line_number)?.ok_or_else(|| {
        format!("corpus ended before required header line {line_number}")
    })
}

fn read_canonical_line<R: BufRead>(
    reader: &mut R,
    line_number: usize,
) -> Result<Option<String>, String> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .map_err(|error| format!("could not read corpus line {line_number}: {error}"))?;
    if read == 0 {
        return Ok(None);
    }
    if line.contains('\r') {
        return Err(format!("line {line_number}: corpus must use canonical LF line endings"));
    }
    if !line.ends_with('\n') {
        return Err(format!("line {line_number}: corpus must end every record with LF"));
    }
    line.pop();
    if line.is_empty() {
        return Err(format!("line {line_number}: corpus must not contain blank lines"));
    }
    Ok(Some(line))
}

fn field<'a>(part: &'a str, name: &str) -> Result<&'a str, String> {
    let prefix = format!("{name}=");
    let value = part
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("header expected field {name}"))?;
    if value.is_empty() {
        return Err(format!("header field {name} is empty"));
    }
    Ok(value)
}

fn parse_canonical_i64(raw: &str, label: &str) -> Result<i64, String> {
    if !is_canonical_signed(raw) {
        return Err(format!("{label} is not a canonical decimal integer: {raw:?}"));
    }
    raw.parse::<i64>()
        .map_err(|_| format!("{label} is outside the supported i64 coordinate range"))
}

fn parse_canonical_u64(raw: &str, label: &str) -> Result<u64, String> {
    if !is_canonical_unsigned(raw) {
        return Err(format!("{label} is not a canonical nonnegative integer: {raw:?}"));
    }
    raw.parse::<u64>()
        .map_err(|_| format!("{label} is outside the supported u64 range"))
}

fn is_canonical_signed(raw: &str) -> bool {
    if raw == "0" {
        return true;
    }
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_canonical_unsigned(raw: &str) -> bool {
    raw == "0"
        || (!raw.is_empty()
            && !raw.starts_with('0')
            && raw.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_lower_sha256(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
        })
}

fn is_resource_location(raw: &str) -> bool {
    let Some((namespace, path)) = raw.split_once(':') else {
        return false;
    };
    if namespace.is_empty() || path.is_empty() || path.contains(':') {
        return false;
    }
    namespace.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'.' | b'-')
    }) && path.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'.' | b'/' | b'-')
    })
}

fn write_usize_map(
    output: &mut String,
    name: &str,
    values: &BTreeMap<usize, usize>,
    indent: usize,
) {
    let prefix = " ".repeat(indent);
    output.push_str(&format!("{prefix}\"{name}\": {{"));
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&format!("\"{key}\":{value}"));
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
    output.push_str(&format!("{prefix}\"{name}\": "));
    write_inline_string_usize_map(output, values);
}

fn write_inline_string_usize_map(output: &mut String, values: &BTreeMap<String, usize>) {
    output.push('{');
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&format!("\"{key}\":{value}"));
    }
    output.push('}');
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, STATE_DATA_GENERATION_SHA256};
    use crucible_world_reference::DirectBlockSection;
    use crucible_world_section::{
        AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
    };

    use super::{
        CorpusPurpose, CorpusReader, CorpusSection, SectionKey, inspect_candidate_section,
    };

    fn target_header() -> String {
        format!(
            "TARGET|minecraft=26.2|protocol=776|data=4903|state_count={BLOCK_STATE_COUNT}|generation_sha256={STATE_DATA_GENERATION_SHA256}"
        )
    }

    fn source_header(extractor: &str) -> String {
        format!(
            "SOURCE|kind=vanilla-save|inventory_sha256={}|extractor={extractor}",
            "a".repeat(64)
        )
    }

    fn section_line(dimension: &str, x: &str, z: &str, y: &str, states: &[u32]) -> String {
        let payload = states
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!("SECTION|{dimension}|{x}|{z}|{y}|{payload}")
    }

    fn corpus(section_lines: &[String], extractor: &str) -> String {
        let mut lines = vec![
            "CRUCIBLE-SECTION-CORPUS|1".to_owned(),
            target_header(),
            source_header(extractor),
        ];
        lines.extend_from_slice(section_lines);
        lines.join("\n") + "\n"
    }

    fn parse_all(text: &str) -> Result<Vec<CorpusSection>, String> {
        let mut reader = CorpusReader::from_reader(Cursor::new(text.as_bytes()))?;
        let mut sections = Vec::new();
        while let Some(section) = reader.next_section()? {
            sections.push(section);
        }
        Ok(sections)
    }

    fn zeros(count: usize) -> Vec<u32> {
        vec![0; count]
    }

    #[test]
    fn valid_corpus_preserves_cell_order_and_cardinality() {
        let states = (0..4096)
            .map(|index| u32::try_from(index % 17).expect("bounded"))
            .collect::<Vec<_>>();
        let text = corpus(
            &[section_line("minecraft:overworld", "-1", "2", "0", &states)],
            "vanilla-save-region-v1-stored-sections",
        );
        let parsed = parse_all(&text).expect("valid corpus");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].cardinality, 17);
        assert_eq!(parsed[0].key.chunk_x, -1);
        for (index, state) in parsed[0].states.iter().enumerate() {
            assert_eq!(state.as_usize(), index % 17);
        }
    }

    #[test]
    fn target_header_drift_is_rejected_field_by_field() {
        let base = corpus(
            &[section_line("minecraft:overworld", "0", "0", "0", &zeros(4096))],
            "vanilla-save-region-v1-stored-sections",
        );
        for (from, to) in [
            ("minecraft=26.2", "minecraft=26.3"),
            ("protocol=776", "protocol=777"),
            ("data=4903", "data=4904"),
            (
                &format!("state_count={BLOCK_STATE_COUNT}"),
                "state_count=1",
            ),
            (
                &format!("generation_sha256={STATE_DATA_GENERATION_SHA256}"),
                &format!("generation_sha256={}", "b".repeat(64)),
            ),
        ] {
            let changed = base.replacen(from, to, 1);
            assert!(CorpusReader::from_reader(Cursor::new(changed.as_bytes())).is_err());
        }
    }

    #[test]
    fn canonical_line_rules_are_fail_closed() {
        let line = section_line("minecraft:overworld", "0", "0", "0", &zeros(4096));
        let valid = corpus(&[line], "vanilla-save-region-v1-stored-sections");
        let crlf = valid.replace('\n', "\r\n");
        assert!(CorpusReader::from_reader(Cursor::new(crlf.as_bytes())).is_err());

        let missing_newline = valid.trim_end_matches('\n');
        let mut reader = CorpusReader::from_reader(Cursor::new(missing_newline.as_bytes()))
            .expect("headers still have newlines");
        assert!(reader.next_section().is_err());

        let blank = valid.replacen("SOURCE|", "\nSOURCE|", 1);
        assert!(CorpusReader::from_reader(Cursor::new(blank.as_bytes())).is_err());
    }

    #[test]
    fn source_header_and_purpose_are_strict() {
        let line = section_line("minecraft:overworld", "0", "0", "0", &zeros(4096));
        let known = corpus(&[line.clone()], "vanilla-save-region-v1-stored-sections");
        let reader = CorpusReader::from_reader(Cursor::new(known.as_bytes())).expect("known corpus");
        assert_eq!(reader.header().purpose, CorpusPurpose::ParserAdmission);
        assert!(!reader.header().decision_eligible());

        let unknown = corpus(&[line], "future-policy-v9");
        let reader = CorpusReader::from_reader(Cursor::new(unknown.as_bytes())).expect("canonical policy");
        assert_eq!(reader.header().purpose, CorpusPurpose::Unclassified);
        assert!(!reader.header().decision_eligible());

        for bad in [
            known.replace("kind=vanilla-save", "kind=other"),
            known.replace(&"a".repeat(64), "ABC"),
            known.replace(
                "extractor=vanilla-save-region-v1-stored-sections",
                "extractor=bad/value",
            ),
        ] {
            assert!(CorpusReader::from_reader(Cursor::new(bad.as_bytes())).is_err());
        }
    }

    #[test]
    fn coordinates_resource_locations_and_order_are_strict() {
        for coordinate in ["+1", "01", "-0"] {
            let text = corpus(
                &[section_line(
                    "minecraft:overworld",
                    coordinate,
                    "0",
                    "0",
                    &zeros(4096),
                )],
                "vanilla-save-region-v1-stored-sections",
            );
            assert!(parse_all(&text).is_err());
        }
        let bad_dimension = corpus(
            &[section_line("Minecraft:Overworld", "0", "0", "0", &zeros(4096))],
            "vanilla-save-region-v1-stored-sections",
        );
        assert!(parse_all(&bad_dimension).is_err());

        let duplicate_line = section_line("minecraft:overworld", "0", "0", "0", &zeros(4096));
        let duplicate = corpus(
            &[duplicate_line.clone(), duplicate_line],
            "vanilla-save-region-v1-stored-sections",
        );
        assert!(parse_all(&duplicate).is_err());

        let out_of_order = corpus(
            &[
                section_line("minecraft:overworld", "0", "0", "0", &zeros(4096)),
                section_line("minecraft:overworld", "-1", "0", "0", &zeros(4096)),
            ],
            "vanilla-save-region-v1-stored-sections",
        );
        assert!(parse_all(&out_of_order).is_err());
    }

    #[test]
    fn exact_cell_count_and_state_spelling_are_strict() {
        for count in [4095, 4097] {
            let text = corpus(
                &[section_line("minecraft:overworld", "0", "0", "0", &zeros(count))],
                "vanilla-save-region-v1-stored-sections",
            );
            assert!(parse_all(&text).is_err());
        }

        let valid_line = section_line("minecraft:overworld", "0", "0", "0", &zeros(4096));
        for invalid in ["00", "-1", "x"] {
            let changed = valid_line.replacen("|0,", &format!("|{invalid},"), 1);
            let text = corpus(&[changed], "vanilla-save-region-v1-stored-sections");
            assert!(parse_all(&text).is_err());
        }

        let outside = u32::try_from(BLOCK_STATE_COUNT).expect("target count fits u32");
        let changed = valid_line.replacen("|0,", &format!("|{outside},"), 1);
        let text = corpus(&[changed], "vanilla-save-region-v1-stored-sections");
        assert!(parse_all(&text).is_err());
    }

    fn assert_candidate_equivalent<C: super::BenchSection>(section: &CorpusSection) {
        let inspected = inspect_candidate_section::<C>(section).expect("candidate reconstructs corpus");
        assert!(inspected.owned_bytes > 0 || inspected.representation == "uniform");
    }

    #[test]
    fn every_benchmark_candidate_reconstructs_exact_corpus_image() {
        let states = (0..4096)
            .map(|index| BlockStateId::new(u32::try_from(index % 17).expect("bounded")).unwrap())
            .collect::<Vec<_>>();
        let section = CorpusSection {
            key: SectionKey {
                dimension: "minecraft:overworld".to_owned(),
                chunk_x: 0,
                chunk_z: 0,
                section_y: 0,
            },
            cardinality: 17,
            states: states.into_boxed_slice(),
        };

        assert_candidate_equivalent::<DirectBlockSection<BlockStateId>>(&section);
        assert_candidate_equivalent::<DirectNBlockSection<BlockStateId>>(&section);
        assert_candidate_equivalent::<AdaptiveBlockSection<BlockStateId>>(&section);
        assert_candidate_equivalent::<FastLocalBlockSection<BlockStateId>>(&section);
        assert_candidate_equivalent::<PackedLocalBlockSection<BlockStateId>>(&section);
    }
}
