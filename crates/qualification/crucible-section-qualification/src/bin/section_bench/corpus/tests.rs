use std::io::Cursor;

use crucible_generated::{BLOCK_STATE_COUNT, BlockStateId, STATE_DATA_GENERATION_SHA256};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::BenchSection;

use super::parser::CorpusReader;
use super::verify::inspect_candidate_section;
use super::{CorpusPurpose, CorpusSection, SectionKey};

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

fn one_section(states: &[u32]) -> String {
    corpus(
        &[section_line(
            "minecraft:overworld",
            "0",
            "0",
            "0",
            states,
        )],
        "vanilla-save-region-v1-stored-sections",
    )
}

#[test]
fn valid_corpus_preserves_cell_order_and_cardinality() {
    let states = (0..4096)
        .map(|index| u32::try_from(index % 17).expect("bounded"))
        .collect::<Vec<_>>();
    let text = corpus(
        &[section_line(
            "minecraft:overworld",
            "-1",
            "2",
            "0",
            &states,
        )],
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
    let base = one_section(&zeros(4096));
    let mutations = vec![
        ("minecraft=26.2".to_owned(), "minecraft=26.3".to_owned()),
        ("protocol=776".to_owned(), "protocol=777".to_owned()),
        ("data=4903".to_owned(), "data=4904".to_owned()),
        (
            format!("state_count={BLOCK_STATE_COUNT}"),
            "state_count=1".to_owned(),
        ),
        (
            format!("generation_sha256={STATE_DATA_GENERATION_SHA256}"),
            format!("generation_sha256={}", "b".repeat(64)),
        ),
    ];
    for (from, to) in mutations {
        let changed = base.replacen(&from, &to, 1);
        assert!(CorpusReader::from_reader(Cursor::new(changed.as_bytes())).is_err());
    }
}

#[test]
fn canonical_line_rules_are_fail_closed() {
    let valid = one_section(&zeros(4096));
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

    for invalid in [
        known.replace("kind=vanilla-save", "kind=other"),
        known.replace(&"a".repeat(64), "ABC"),
        known.replace(
            "extractor=vanilla-save-region-v1-stored-sections",
            "extractor=bad/value",
        ),
    ] {
        assert!(CorpusReader::from_reader(Cursor::new(invalid.as_bytes())).is_err());
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
        &[section_line(
            "Minecraft:Overworld",
            "0",
            "0",
            "0",
            &zeros(4096),
        )],
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
        assert!(parse_all(&one_section(&zeros(count))).is_err());
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

fn corpus_section(cardinality: usize) -> CorpusSection {
    let states = (0..4096)
        .map(|index| {
            let raw = u32::try_from(index % cardinality).expect("bounded state ID");
            BlockStateId::new(raw).expect("test state exists")
        })
        .collect::<Vec<_>>();
    CorpusSection {
        key: SectionKey {
            dimension: "minecraft:overworld".to_owned(),
            chunk_x: 0,
            chunk_z: 0,
            section_y: 0,
        },
        states: states.into_boxed_slice(),
        cardinality,
    }
}

fn assert_candidate_equivalent<C: BenchSection>(section: &CorpusSection) {
    let inspected = inspect_candidate_section::<C>(section).expect("candidate reconstructs corpus");
    assert!(inspected.owned_bytes > 0 || inspected.representation == "uniform");
}

#[test]
fn every_benchmark_candidate_reconstructs_exact_corpus_image() {
    let section = corpus_section(17);
    assert_candidate_equivalent::<DirectBlockSection<BlockStateId>>(&section);
    assert_candidate_equivalent::<DirectNBlockSection<BlockStateId>>(&section);
    assert_candidate_equivalent::<AdaptiveBlockSection<BlockStateId>>(&section);
    assert_candidate_equivalent::<FastLocalBlockSection<BlockStateId>>(&section);
    assert_candidate_equivalent::<PackedLocalBlockSection<BlockStateId>>(&section);
}

#[test]
fn imported_boundary_image_exercises_real_representation_transitions() {
    let section = corpus_section(17);
    let adaptive = inspect_candidate_section::<AdaptiveBlockSection<BlockStateId>>(&section)
        .expect("adaptive reconstructs corpus");
    let packed = inspect_candidate_section::<PackedLocalBlockSection<BlockStateId>>(&section)
        .expect("packed reconstructs corpus");
    assert!(adaptive.transitions >= 2);
    assert!(adaptive.logical_allocations >= 4);
    assert!(packed.transitions >= 2);
    assert!(packed.logical_allocations >= 4);
}
