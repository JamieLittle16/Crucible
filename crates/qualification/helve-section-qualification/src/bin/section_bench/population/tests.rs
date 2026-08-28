use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use helve_generated::{
    AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
};
use helve_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use helve_world_contract::BlockSection;
use helve_world_section::DirectNBlockSection;

use crate::model::BenchSection;
use crate::workloads::pos;

use super::PopulationMode;
use super::measure::{percentile, positive_needles};
use super::pack::{
    PACK_MAGIC, PAYLOAD_BYTES_PER_SECTION, PackReader, RSS_PROTOCOL, canonical_usize,
    prefault_common_scratch, signed_rss_delta, status_value_kib,
};
use super::report::single_cpu;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_pack(payload_sections: usize, trailing: &[u8], target_generation: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "helve-section-population-pack-{}-{id}.bin",
        std::process::id()
    ));
    let mut file = fs::File::create(&path).expect("create pack");
    write!(
        file,
        "{PACK_MAGIC}\nTARGET|minecraft={MINECRAFT_VERSION}|protocol={PROTOCOL_VERSION}|data={DATA_VERSION}|state_count={BLOCK_STATE_COUNT}|generation_sha256={target_generation}\nPOPULATION|population_sha256={}|admission_sha256={}\nDIMENSION|name=minecraft:overworld|section_count={payload_sections}\nDATA\n",
        "a".repeat(64),
        "b".repeat(64),
    )
    .expect("write header");
    file.write_all(&vec![0_u8; payload_sections * PAYLOAD_BYTES_PER_SECTION])
        .expect("write payload");
    file.write_all(trailing).expect("write trailing");
    path
}

#[test]
fn pack_reader_accepts_exact_payload_and_rejects_trailing_bytes() {
    let path = temp_pack(1, &[], STATE_DATA_GENERATION_SHA256);
    let mut reader = PackReader::open(&path).expect("valid pack");
    let mut scratch = vec![0_u8; PAYLOAD_BYTES_PER_SECTION];
    assert!(reader.read_section(&mut scratch).expect("section"));
    assert!(!reader.read_section(&mut scratch).expect("eof"));
    reader.finish().expect("exact eof");
    fs::remove_file(path).expect("remove pack");

    let bad = temp_pack(1, b"x", STATE_DATA_GENERATION_SHA256);
    let mut reader = PackReader::open(&bad).expect("headers valid");
    assert!(reader.read_section(&mut scratch).expect("section"));
    assert!(reader.finish().is_err());
    fs::remove_file(bad).expect("remove bad pack");
}

#[test]
fn zero_section_pack_is_rejected() {
    let path = temp_pack(0, &[], STATE_DATA_GENERATION_SHA256);
    assert!(PackReader::open(&path).is_err());
    fs::remove_file(path).expect("remove pack");
}

#[test]
fn target_generation_drift_is_rejected() {
    let path = temp_pack(1, &[], &"c".repeat(64));
    assert!(PackReader::open(&path).is_err());
    fs::remove_file(path).expect("remove pack");
}

#[test]
fn truncated_payload_is_rejected() {
    let path = temp_pack(1, &[], STATE_DATA_GENERATION_SHA256);
    let metadata = fs::metadata(&path).expect("pack metadata");
    let shortened = metadata.len() - 1;
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open pack")
        .set_len(shortened)
        .expect("truncate pack");
    let mut reader = PackReader::open(&path).expect("headers remain valid");
    let mut scratch = vec![0_u8; PAYLOAD_BYTES_PER_SECTION];
    assert!(reader.read_section(&mut scratch).is_err());
    fs::remove_file(path).expect("remove pack");
}

#[test]
fn percentile_and_canonical_numbers_are_stable() {
    assert_eq!(percentile(&[10, 20, 30, 40, 50], 50), 30);
    assert_eq!(percentile(&[10, 20, 30, 40, 50], 95), 50);
    assert_eq!(percentile(&[10, 20, 30, 40, 50], 99), 50);
    assert_eq!(canonical_usize("0", "test"), Ok(0));
    assert_eq!(canonical_usize("17", "test"), Ok(17));
    assert!(canonical_usize("01", "test").is_err());
    assert!(canonical_usize("+1", "test").is_err());
    assert!(canonical_usize("", "test").is_err());
}

#[test]
fn affinity_frequency_cpu_requires_one_exact_cpu() {
    assert_eq!(single_cpu("7"), Some(7));
    assert_eq!(single_cpu("0-3"), None);
    assert_eq!(single_cpu("1,3"), None);
    assert_eq!(single_cpu(""), None);
}

#[test]
fn proc_status_memory_parser_requires_kilobytes() {
    let status = "Name:\ttest\nVmRSS:\t1234 kB\nVmHWM:\t2345 kB\n";
    assert_eq!(status_value_kib(status, "VmRSS:"), Ok(1234));
    assert_eq!(status_value_kib(status, "VmHWM:"), Ok(2345));
    assert!(status_value_kib("VmRSS: 10 MB\n", "VmRSS:").is_err());
    assert!(status_value_kib("Name: test\n", "VmRSS:").is_err());
}

#[test]
fn common_rss_scratch_prefault_restores_canonical_initial_values() {
    let mut raw = vec![0_u8; PAYLOAD_BYTES_PER_SECTION * 2];
    let mut decoded = vec![AIR; 4096];
    let mut observed = vec![0_u64; BLOCK_STATE_COUNT.div_ceil(64)];
    let mut construction = vec![0_u128; 257];

    prefault_common_scratch(&mut raw, &mut decoded, &mut observed, &mut construction);

    assert!(raw.iter().all(|value| *value == 0));
    assert!(decoded.iter().all(|state| *state == AIR));
    assert!(observed.iter().all(|value| *value == 0));
    assert!(construction.iter().all(|value| *value == 0));
    assert_eq!(
        RSS_PROTOCOL,
        "candidate-delta-after-explicit-prefaulted-common-scratch"
    );
}

#[test]
fn rss_delta_preserves_negative_measurements_instead_of_saturating() {
    assert_eq!(signed_rss_delta(1_500, 1_000), Ok(500));
    assert_eq!(signed_rss_delta(1_000, 1_500), Ok(-500));
    assert_eq!(signed_rss_delta(1_234, 1_234), Ok(0));
}

#[test]
fn positive_membership_needles_follow_planned_cells_not_fixed_cell_zero() {
    let mut section = <DirectNBlockSection<BlockStateId> as BenchSection>::filled(AIR);
    let marker = BlockStateId::new(1).expect("qualified target contains state 1");
    let _ = section.replace(pos(37), marker, &GeneratedStateFacts);
    let plan = [(0_usize, 37_usize), (0, 0)];

    let needles = positive_needles(&[section], &plan);

    assert_eq!(needles, vec![marker, AIR]);
}

#[test]
fn qualification_settings_are_stronger_than_smoke() {
    let smoke = PopulationMode::Smoke.settings();
    let qualification = PopulationMode::Qualification.settings();
    assert!(qualification.measured_samples > smoke.measured_samples);
    assert!(qualification.random_reads > smoke.random_reads);
    assert!(qualification.sequential_sections > smoke.sequential_sections);
    assert!(qualification.volume_queries > smoke.volume_queries);
    assert!(qualification.contains_queries > smoke.contains_queries);
    assert!(qualification.control_operations > smoke.control_operations);
}
