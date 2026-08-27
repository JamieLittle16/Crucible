use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use helve_generated::{BLOCK_STATE_COUNT, BlockStateId, STATE_DATA_GENERATION_SHA256};
use helve_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use helve_world_contract::BLOCK_SECTION_CELLS;

use super::{CorpusHeader, CorpusPurpose, CorpusSection, SectionKey};

const MAGIC: &str = "CRUCIBLE-SECTION-CORPUS|1";
const SOURCE_KIND: &str = "vanilla-save";
const SECTION_LINE_INITIAL_CAPACITY: usize = 32 * 1024;
const STATE_SEEN_WORDS: usize = BLOCK_STATE_COUNT.div_ceil(64);

pub(super) struct CorpusReader<R: BufRead> {
    reader: R,
    header: CorpusHeader,
    next_line_number: usize,
    previous_key: Option<SectionKey>,
    line_buffer: String,
}

impl CorpusReader<BufReader<File>> {
    pub(super) fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|error| format!("could not open corpus {}: {error}", path.display()))?;
        Self::from_reader(BufReader::new(file))
    }
}

impl<R: BufRead> CorpusReader<R> {
    pub(super) fn from_reader(mut reader: R) -> Result<Self, String> {
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
            line_buffer: String::with_capacity(SECTION_LINE_INITIAL_CAPACITY),
        })
    }

    pub(super) fn header(&self) -> &CorpusHeader {
        &self.header
    }

    pub(super) fn next_section(&mut self) -> Result<Option<CorpusSection>, String> {
        let line_number = self.next_line_number;
        self.line_buffer.clear();
        if !read_canonical_line_into(&mut self.reader, &mut self.line_buffer, line_number)? {
            return Ok(None);
        }
        self.next_line_number += 1;
        let section = parse_section_line(&self.line_buffer, line_number)?;
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
    let target_state_count = u64::try_from(BLOCK_STATE_COUNT).expect("target state count fits u64");

    if minecraft != MINECRAFT_VERSION
        || protocol != u64::from(PROTOCOL_VERSION)
        || data != u64::from(DATA_VERSION)
        || state_count != target_state_count
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

    let mut states = Vec::with_capacity(BLOCK_SECTION_CELLS);
    let mut seen = [0_u64; STATE_SEEN_WORDS];
    let mut cardinality = 0_usize;

    for (cell, raw) in parts[5].split(',').enumerate() {
        if cell >= BLOCK_SECTION_CELLS {
            return Err(format!(
                "line {line_number}: section has more than {BLOCK_SECTION_CELLS} cells"
            ));
        }
        let state = parse_state_id(raw, line_number, cell)?;
        let index = state.as_usize();
        let word = index >> 6;
        let bit = 1_u64 << (index & 63);
        if seen[word] & bit == 0 {
            seen[word] |= bit;
            cardinality += 1;
        }
        states.push(state);
    }

    if states.len() != BLOCK_SECTION_CELLS {
        return Err(format!(
            "line {line_number}: section has {} cells; expected {BLOCK_SECTION_CELLS}",
            states.len()
        ));
    }

    Ok(CorpusSection {
        key,
        states: states.into_boxed_slice(),
        cardinality,
    })
}

fn parse_state_id(raw: &str, line_number: usize, cell: usize) -> Result<BlockStateId, String> {
    if !is_canonical_unsigned(raw) {
        return Err(format!(
            "line {line_number} cell {cell}: noncanonical state ID {raw:?}"
        ));
    }
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("line {line_number} cell {cell}: state ID {raw} does not fit u32"))?;
    BlockStateId::new(value).ok_or_else(|| {
        format!(
            "line {line_number} cell {cell}: state ID {value} outside 0..{}",
            BLOCK_STATE_COUNT - 1
        )
    })
}

fn read_required_line<R: BufRead>(reader: &mut R, line_number: usize) -> Result<String, String> {
    let mut line = String::new();
    if !read_canonical_line_into(reader, &mut line, line_number)? {
        return Err(format!(
            "corpus ended before required header line {line_number}"
        ));
    }
    Ok(line)
}

fn read_canonical_line_into<R: BufRead>(
    reader: &mut R,
    line: &mut String,
    line_number: usize,
) -> Result<bool, String> {
    let read = reader
        .read_line(line)
        .map_err(|error| format!("could not read corpus line {line_number}: {error}"))?;
    if read == 0 {
        return Ok(false);
    }
    if line.contains('\r') {
        return Err(format!(
            "line {line_number}: corpus must use canonical LF line endings"
        ));
    }
    if !line.ends_with('\n') {
        return Err(format!(
            "line {line_number}: corpus must end every record with LF"
        ));
    }
    line.pop();
    if line.is_empty() {
        return Err(format!(
            "line {line_number}: corpus must not contain blank lines"
        ));
    }
    Ok(true)
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
        return Err(format!(
            "{label} is not a canonical decimal integer: {raw:?}"
        ));
    }
    raw.parse::<i64>()
        .map_err(|_| format!("{label} is outside the supported i64 coordinate range"))
}

fn parse_canonical_u64(raw: &str, label: &str) -> Result<u64, String> {
    if !is_canonical_unsigned(raw) {
        return Err(format!(
            "{label} is not a canonical nonnegative integer: {raw:?}"
        ));
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
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
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
