use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crucible_generated::{
    BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
    STATE_DATA_INPUT_SHA256,
};
use crucible_world_contract::{BlockStateFacts, SectionStateFacts};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

use crate::model::BenchSection;

use super::parser::CorpusReader;
use super::verify::{inspect_candidate_section, recompute_section_summary_for_test};
use super::{CorpusPurpose, CorpusSection, SectionKey, check_corpus};

static TEMP_CORPUS_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempCorpus {
    path: PathBuf,
}

impl TempCorpus {
    fn write(text: &str) -> Self {
        let serial = TEMP_CORPUS_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "crucible-section-corpus-test-{}-{serial}.corpus",
            std::process::id()
        ));
        fs::write(&path, text).expect("write temporary corpus");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCorpus {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

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

fn states_with_cardinality(cardinality: usize) -> Vec<u32> {
    (0..4096)
        .map(|index| u32::try_from(index % cardinality).expect("bounded"))
        .collect()
}

fn one_section(states: &[u32]) -> String {
    corpus(
        &[section_line("minecraft:overworld", "0", "0", "0", states)],
        "vanilla-save-region-v1-stored-sections",
    )
}

#[test]
fn valid_corpus_preserves_cell_order_and_cardinality() {
    let states = states_with_cardinality(17);
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
    let known = corpus(
        std::slice::from_ref(&line),
        "vanilla-save-region-v1-stored-sections",
    );
    let reader = CorpusReader::from_reader(Cursor::new(known.as_bytes())).expect("known corpus");
    assert_eq!(reader.header().purpose, CorpusPurpose::ParserAdmission);
    assert!(!reader.header().decision_eligible());

    let unknown = corpus(&[line], "future-policy-v9");
    let reader =
        CorpusReader::from_reader(Cursor::new(unknown.as_bytes())).expect("canonical policy");
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

#[test]
fn empty_corpus_is_rejected_by_full_import_gate() {
    let text = corpus(&[], "vanilla-save-region-v1-stored-sections");
    let file = TempCorpus::write(&text);
    assert!(check_corpus(file.path(), false).is_err());
}

#[test]
fn parser_admission_and_unknown_corpora_are_never_decision_eligible() {
    for extractor in ["vanilla-save-region-v1-stored-sections", "future-policy-v9"] {
        let text = corpus(
            &[section_line(
                "minecraft:overworld",
                "0",
                "0",
                "0",
                &zeros(4096),
            )],
            extractor,
        );
        let file = TempCorpus::write(&text);
        let error = check_corpus(file.path(), true).expect_err("decision check must fail closed");
        assert!(error.contains("not decision-eligible"));
    }
}

#[test]
fn full_import_aggregates_metadata_and_all_candidates_from_one_image() {
    let text = corpus(
        &[
            section_line("minecraft:overworld", "0", "0", "0", &zeros(4096)),
            section_line(
                "minecraft:overworld",
                "0",
                "0",
                "1",
                &states_with_cardinality(17),
            ),
        ],
        "vanilla-save-region-v1-stored-sections",
    );
    let file = TempCorpus::write(&text);
    let checked = check_corpus(file.path(), false).expect("full import passes");

    assert_eq!(checked.section_count, 2);
    assert_eq!(checked.total_cells, 8192);
    assert_eq!(checked.distinct_state_ids, 17);
    assert_eq!(checked.cardinality_histogram.get(&1), Some(&1));
    assert_eq!(checked.cardinality_histogram.get(&17), Some(&1));
    assert_eq!(checked.dimensions.get("minecraft:overworld"), Some(&2));
    assert_eq!(checked.candidates.len(), 5);
    assert!(
        checked
            .candidates
            .iter()
            .all(|candidate| candidate.sections == 2)
    );

    let json = checked.to_json(false);
    assert!(json.contains(STATE_DATA_GENERATION_SHA256));
    assert!(json.contains(STATE_DATA_INPUT_SHA256));
    assert!(json.contains("\"purpose\": \"parser-admission\""));
    assert!(json.contains("\"decision_eligible\": false"));
}

fn corpus_section(cardinality: usize) -> CorpusSection {
    let states = states_with_cardinality(cardinality)
        .into_iter()
        .map(|raw| BlockStateId::new(raw).expect("test state exists"))
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
    let expected_summary = recompute_section_summary_for_test(section);
    let inspected = inspect_candidate_section::<C>(section, expected_summary)
        .expect("candidate reconstructs corpus");
    assert!(inspected.owned_bytes > 0 || inspected.representation == "uniform");
}

#[test]
fn every_benchmark_candidate_reconstructs_exact_corpus_image_and_summary() {
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
    let expected_summary = recompute_section_summary_for_test(&section);
    let adaptive =
        inspect_candidate_section::<AdaptiveBlockSection<BlockStateId>>(&section, expected_summary)
            .expect("adaptive reconstructs corpus");
    let packed = inspect_candidate_section::<PackedLocalBlockSection<BlockStateId>>(
        &section,
        expected_summary,
    )
    .expect("packed reconstructs corpus");
    assert!(adaptive.transitions >= 2);
    assert!(adaptive.logical_allocations >= 4);
    assert!(packed.transitions >= 2);
    assert!(packed.logical_allocations >= 4);
}

fn find_target_state(mut predicate: impl FnMut(SectionStateFacts) -> bool) -> BlockStateId {
    for raw in 0..BLOCK_STATE_COUNT {
        let raw = u32::try_from(raw).expect("target state ID fits u32");
        let state = BlockStateId::new(raw).expect("bounded target state");
        let facts = <GeneratedStateFacts as BlockStateFacts<BlockStateId>>::facts(
            &GeneratedStateFacts,
            state,
        );
        if predicate(facts) {
            return state;
        }
    }
    panic!("target state universe lacks required semantic fact class");
}

#[test]
fn mixed_real_target_fact_classes_survive_import_reconstruction() {
    let air = find_target_state(|facts| !facts.non_air());
    let solid = find_target_state(|facts| facts.non_air() && !facts.counted_fluid());
    let fluid = find_target_state(|facts| facts.counted_fluid());
    let random_block = find_target_state(|facts| facts.random_block());
    let random_fluid = find_target_state(|facts| facts.random_fluid());

    let mut states = vec![air.as_u32(); 4096];
    states[1] = solid.as_u32();
    states[2] = fluid.as_u32();
    states[3] = random_block.as_u32();
    states[4] = random_fluid.as_u32();
    let text = one_section(&states);
    let file = TempCorpus::write(&text);
    let checked = check_corpus(file.path(), false).expect("mixed target-fact corpus passes");

    assert_eq!(checked.section_count, 1);
    assert_eq!(checked.candidates.len(), 5);
    assert!(
        checked
            .candidates
            .iter()
            .all(|candidate| candidate.sections == 1)
    );
}
