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

const EXTRACTOR: &str = "vanilla-save-region-v1-stored-sections";
const DIMENSION: &str = "minecraft:overworld";
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

fn section_line(dimension: &str, x: i64, z: i64, y: i64, states: &[u32]) -> String {
    let payload = states
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("SECTION|{dimension}|{x}|{z}|{y}|{payload}")
}

fn raw_section_line(dimension: &str, x: &str, z: &str, y: &str, states: &[u32]) -> String {
    let payload = states
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("SECTION|{dimension}|{x}|{z}|{y}|{payload}")
}

fn corpus(lines: &[String], extractor: &str) -> String {
    let mut records = vec![
        "CRUCIBLE-SECTION-CORPUS|1".to_owned(),
        target_header(),
        source_header(extractor),
    ];
    records.extend_from_slice(lines);
    records.join("\n") + "\n"
}

fn one_section(states: &[u32]) -> String {
    corpus(&[section_line(DIMENSION, 0, 0, 0, states)], EXTRACTOR)
}

fn states_with_cardinality(cardinality: usize) -> Vec<u32> {
    assert!(cardinality > 0);
    (0..4096)
        .map(|index| u32::try_from(index % cardinality).expect("bounded state ID"))
        .collect()
}

fn parse_all(text: &str) -> Result<Vec<CorpusSection>, String> {
    let mut reader = CorpusReader::from_reader(Cursor::new(text.as_bytes()))?;
    let mut sections = Vec::new();
    while let Some(section) = reader.next_section()? {
        sections.push(section);
    }
    Ok(sections)
}

fn synthetic_section(cardinality: usize) -> CorpusSection {
    let states = states_with_cardinality(cardinality)
        .into_iter()
        .map(|raw| BlockStateId::new(raw).expect("test state exists"))
        .collect::<Vec<_>>();
    CorpusSection {
        key: SectionKey {
            dimension: DIMENSION.to_owned(),
            chunk_x: 0,
            chunk_z: 0,
            section_y: 0,
        },
        states: states.into_boxed_slice(),
        cardinality,
    }
}

fn assert_candidate_exact<C: BenchSection>(section: &CorpusSection) {
    let expected = recompute_section_summary_for_test(section);
    let inspected = inspect_candidate_section::<C>(section, expected)
        .expect("candidate exactly reconstructs imported image and summary");
    assert!(inspected.owned_bytes > 0 || inspected.representation == "uniform");
}

fn assert_all_candidates_exact(section: &CorpusSection) {
    assert_candidate_exact::<DirectBlockSection<BlockStateId>>(section);
    assert_candidate_exact::<DirectNBlockSection<BlockStateId>>(section);
    assert_candidate_exact::<AdaptiveBlockSection<BlockStateId>>(section);
    assert_candidate_exact::<FastLocalBlockSection<BlockStateId>>(section);
    assert_candidate_exact::<PackedLocalBlockSection<BlockStateId>>(section);
}

#[test]
fn canonical_corpus_preserves_exact_cell_order_and_cardinality() {
    let states = states_with_cardinality(17);
    let text = corpus(&[section_line(DIMENSION, -1, 2, 0, &states)], EXTRACTOR);
    let parsed = parse_all(&text).expect("canonical corpus");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].key.chunk_x, -1);
    assert_eq!(parsed[0].cardinality, 17);
    for (index, state) in parsed[0].states.iter().enumerate() {
        assert_eq!(state.as_usize(), index % 17);
    }
}

#[test]
fn target_identity_drift_is_rejected_field_by_field() {
    let base = one_section(&vec![0; 4096]);
    let mutations = [
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
fn canonical_text_rules_fail_closed() {
    let valid = one_section(&vec![0; 4096]);
    let crlf = valid.replace('\n', "\r\n");
    assert!(CorpusReader::from_reader(Cursor::new(crlf.as_bytes())).is_err());

    let missing_terminal_lf = valid.trim_end_matches('\n');
    let mut reader = CorpusReader::from_reader(Cursor::new(missing_terminal_lf.as_bytes()))
        .expect("headers remain canonical");
    assert!(reader.next_section().is_err());

    let blank = valid.replacen("SOURCE|", "\nSOURCE|", 1);
    assert!(CorpusReader::from_reader(Cursor::new(blank.as_bytes())).is_err());
}

#[test]
fn source_identity_and_policy_are_fail_closed() {
    let line = section_line(DIMENSION, 0, 0, 0, &vec![0; 4096]);
    let known = corpus(std::slice::from_ref(&line), EXTRACTOR);
    let reader = CorpusReader::from_reader(Cursor::new(known.as_bytes())).expect("known source");
    assert_eq!(reader.header().purpose, CorpusPurpose::ParserAdmission);
    assert!(!reader.header().decision_eligible());

    let unknown = corpus(std::slice::from_ref(&line), "future-policy-v9");
    let reader = CorpusReader::from_reader(Cursor::new(unknown.as_bytes())).expect("unknown token");
    assert_eq!(reader.header().purpose, CorpusPurpose::Unclassified);
    assert!(!reader.header().decision_eligible());

    for malformed in [
        known.replace("kind=vanilla-save", "kind=other"),
        known.replace(&"a".repeat(64), "ABC"),
        known.replace(&format!("extractor={EXTRACTOR}"), "extractor=bad/value"),
    ] {
        assert!(CorpusReader::from_reader(Cursor::new(malformed.as_bytes())).is_err());
    }
}

#[test]
fn coordinates_dimensions_and_record_order_are_strict() {
    for bad_coordinate in ["+1", "01", "-0"] {
        let text = corpus(
            &[raw_section_line(
                DIMENSION,
                bad_coordinate,
                "0",
                "0",
                &vec![0; 4096],
            )],
            EXTRACTOR,
        );
        assert!(parse_all(&text).is_err());
    }

    let bad_dimension = corpus(
        &[raw_section_line(
            "Minecraft:Overworld",
            "0",
            "0",
            "0",
            &vec![0; 4096],
        )],
        EXTRACTOR,
    );
    assert!(parse_all(&bad_dimension).is_err());

    let line = section_line(DIMENSION, 0, 0, 0, &vec![0; 4096]);
    assert!(parse_all(&corpus(&[line.clone(), line], EXTRACTOR)).is_err());

    let out_of_order = corpus(
        &[
            section_line(DIMENSION, 0, 0, 0, &vec![0; 4096]),
            section_line(DIMENSION, -1, 0, 0, &vec![0; 4096]),
        ],
        EXTRACTOR,
    );
    assert!(parse_all(&out_of_order).is_err());
}

#[test]
fn section_shape_and_state_ids_are_strict() {
    for cell_count in [4095, 4097] {
        assert!(parse_all(&one_section(&vec![0; cell_count])).is_err());
    }

    let canonical = section_line(DIMENSION, 0, 0, 0, &vec![0; 4096]);
    for invalid in ["00", "-1", "x"] {
        let changed = canonical.replacen("|0,", &format!("|{invalid},"), 1);
        assert!(parse_all(&corpus(&[changed], EXTRACTOR)).is_err());
    }

    let outside = u32::try_from(BLOCK_STATE_COUNT).expect("target count fits u32");
    let changed = canonical.replacen("|0,", &format!("|{outside},"), 1);
    assert!(parse_all(&corpus(&[changed], EXTRACTOR)).is_err());
}

#[test]
fn empty_and_non_decision_corpora_fail_their_respective_gates() {
    let empty = TempCorpus::write(&corpus(&[], EXTRACTOR));
    assert!(check_corpus(empty.path(), false).is_err());

    for extractor in [EXTRACTOR, "future-policy-v9"] {
        let text = corpus(
            &[section_line(DIMENSION, 0, 0, 0, &vec![0; 4096])],
            extractor,
        );
        let file = TempCorpus::write(&text);
        let error = check_corpus(file.path(), true).expect_err("decision gate must reject");
        assert!(error.contains("not decision-eligible"));
    }
}

#[test]
fn full_import_aggregates_one_stream_and_binds_generated_state_identity() {
    let text = corpus(
        &[
            section_line(DIMENSION, 0, 0, 0, &vec![0; 4096]),
            section_line(DIMENSION, 0, 0, 1, &states_with_cardinality(17)),
        ],
        EXTRACTOR,
    );
    let file = TempCorpus::write(&text);
    let checked = check_corpus(file.path(), false).expect("full import");

    assert_eq!(checked.section_count, 2);
    assert_eq!(checked.total_cells, 8192);
    assert_eq!(checked.distinct_state_ids, 17);
    assert_eq!(checked.cardinality_histogram.get(&1), Some(&1));
    assert_eq!(checked.cardinality_histogram.get(&17), Some(&1));
    assert_eq!(checked.dimensions.get(DIMENSION), Some(&2));
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

#[test]
fn every_candidate_is_exact_at_palette_boundaries() {
    for cardinality in [1, 2, 15, 16, 17, 255, 256, 257] {
        let section = synthetic_section(cardinality);
        assert_all_candidates_exact(&section);
    }
}

#[test]
fn boundary_images_exercise_adaptive_and_packed_transitions() {
    let section = synthetic_section(17);
    let expected = recompute_section_summary_for_test(&section);
    let adaptive =
        inspect_candidate_section::<AdaptiveBlockSection<BlockStateId>>(&section, expected)
            .expect("adaptive exact");
    let packed =
        inspect_candidate_section::<PackedLocalBlockSection<BlockStateId>>(&section, expected)
            .expect("packed exact");
    assert!(adaptive.transitions >= 2);
    assert!(adaptive.logical_allocations >= 4);
    assert!(packed.transitions >= 2);
    assert!(packed.logical_allocations >= 4);
}

fn find_target_state(mut predicate: impl FnMut(SectionStateFacts) -> bool) -> u32 {
    for raw in 0..BLOCK_STATE_COUNT {
        let raw = u32::try_from(raw).expect("target state ID fits u32");
        let state = BlockStateId::new(raw).expect("bounded target state");
        let facts = <GeneratedStateFacts as BlockStateFacts<BlockStateId>>::facts(
            &GeneratedStateFacts,
            state,
        );
        if predicate(facts) {
            return raw;
        }
    }
    panic!("target state universe lacks required semantic fact class");
}

#[test]
fn real_generated_fact_classes_survive_full_import_and_summary_checks() {
    let air = find_target_state(|facts| !facts.non_air());
    let solid = find_target_state(|facts| facts.non_air() && !facts.counted_fluid());
    let fluid = find_target_state(SectionStateFacts::counted_fluid);
    let random_block = find_target_state(SectionStateFacts::random_block);
    let random_fluid = find_target_state(SectionStateFacts::random_fluid);

    let mut states = vec![air; 4096];
    states[1] = solid;
    states[2] = fluid;
    states[3] = random_block;
    states[4] = random_fluid;

    let file = TempCorpus::write(&one_section(&states));
    let checked = check_corpus(file.path(), false).expect("mixed target facts import");
    assert_eq!(checked.section_count, 1);
    assert_eq!(checked.candidates.len(), 5);
    assert!(
        checked
            .candidates
            .iter()
            .all(|candidate| candidate.sections == 1)
    );
}
