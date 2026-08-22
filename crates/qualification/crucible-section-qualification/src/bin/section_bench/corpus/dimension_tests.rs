use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use crucible_generated::{BLOCK_STATE_COUNT, STATE_DATA_GENERATION_SHA256};

use super::check_corpus;

static SERIAL: AtomicU64 = AtomicU64::new(0);

fn section_line(dimension: &str, section_y: i64, states: impl Iterator<Item = u32>) -> String {
    let payload = states
        .map(|state| state.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("SECTION|{dimension}|0|0|{section_y}|{payload}")
}

fn corpus_text() -> String {
    let overworld = section_line("minecraft:overworld", 0, std::iter::repeat_n(0_u32, 4096));
    let end = section_line(
        "minecraft:the_end",
        0,
        (0..4096).map(|index| u32::try_from(index & 1).expect("bounded state")),
    );
    format!(
        "CRUCIBLE-SECTION-CORPUS|1\nTARGET|minecraft=26.2|protocol=776|data=4903|state_count={BLOCK_STATE_COUNT}|generation_sha256={STATE_DATA_GENERATION_SHA256}\nSOURCE|kind=vanilla-save|inventory_sha256={}|extractor=vanilla-save-region-v2-representative-member\n{overworld}\n{end}\n",
        "a".repeat(64)
    )
}

#[test]
fn per_dimension_evidence_is_not_implicitly_cross_weighted() {
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "crucible-section-dimension-evidence-{}-{serial}.corpus",
        std::process::id()
    ));
    fs::write(&path, corpus_text()).expect("write corpus");
    let checked = check_corpus(&path, false).expect("dimension-separated corpus");
    let _ = fs::remove_file(&path);

    assert_eq!(checked.section_count, 2);
    assert_eq!(checked.dimensions.get("minecraft:overworld"), Some(&1));
    assert_eq!(checked.dimensions.get("minecraft:the_end"), Some(&1));

    let overworld = checked
        .per_dimension
        .get("minecraft:overworld")
        .expect("overworld evidence");
    let end = checked
        .per_dimension
        .get("minecraft:the_end")
        .expect("end evidence");
    assert_eq!(overworld.section_count, 1);
    assert_eq!(overworld.cardinality_histogram.get(&1), Some(&1));
    assert_eq!(end.section_count, 1);
    assert_eq!(end.cardinality_histogram.get(&2), Some(&1));
    assert!(
        overworld
            .candidates
            .iter()
            .all(|candidate| candidate.sections == 1)
    );
    assert!(
        end.candidates
            .iter()
            .all(|candidate| candidate.sections == 1)
    );
    assert!(
        checked
            .candidates
            .iter()
            .all(|candidate| candidate.sections == 2)
    );

    let json = checked.to_json(false);
    assert!(json.contains("\"per_dimension\""));
    assert!(json.contains("\"minecraft:overworld\""));
    assert!(json.contains("\"minecraft:the_end\""));
}
