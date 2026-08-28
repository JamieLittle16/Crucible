use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::fs;

use helve_generated::{BLOCK_STATE_COUNT, STATE_DATA_GENERATION_SHA256, STATE_DATA_INPUT_SHA256};
use helve_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};

use crate::hardware::HardwareMetadata;
use crate::model::BenchSection;

use super::pack::{LoadedCandidate, RSS_PROTOCOL};
use super::{PopulationMode, PopulationSettings, SampleSummary, TimingRecord};

const REPORT_SCHEMA: u32 = 1;
const REPORT_VERSION: &str = "section-population-bench-v1";
const BUILD_PROFILE: &str = "release";
const CODEGEN_POLICY: &str = "lto=thin,codegen-units=1,panic=abort,strip=debuginfo";

pub(super) fn render<C: BenchSection>(
    mode: PopulationMode,
    settings: PopulationSettings,
    loaded: &LoadedCandidate<C>,
    timings: &[TimingRecord],
    hardware: &HardwareMetadata,
) -> Result<String, String> {
    let affinity_cpu = single_cpu(&hardware.cpus_allowed_list);
    let affinity_frequency = affinity_cpu.map(cpu_frequency_metadata);
    let mut out = String::new();
    writeln!(out, "{{").map_err(fmt_error)?;
    number(&mut out, 2, "schema", u128::from(REPORT_SCHEMA), true)?;
    string(&mut out, 2, "harness_version", REPORT_VERSION, true)?;
    string(&mut out, 2, "mode", mode.as_str(), true)?;
    string(&mut out, 2, "candidate", C::NAME, true)?;
    boolean(
        &mut out,
        2,
        "production_candidate",
        C::PRODUCTION_CANDIDATE,
        true,
    )?;
    string(&mut out, 2, "build_profile", BUILD_PROFILE, true)?;
    string(&mut out, 2, "codegen_policy", CODEGEN_POLICY, true)?;
    string(&mut out, 2, "minecraft_version", MINECRAFT_VERSION, true)?;
    number(
        &mut out,
        2,
        "protocol_version",
        u128::from(PROTOCOL_VERSION),
        true,
    )?;
    number(&mut out, 2, "data_version", u128::from(DATA_VERSION), true)?;
    number(
        &mut out,
        2,
        "state_count",
        u128::try_from(BLOCK_STATE_COUNT).expect("state count fits u128"),
        true,
    )?;
    string(
        &mut out,
        2,
        "state_data_generation_sha256",
        STATE_DATA_GENERATION_SHA256,
        true,
    )?;
    string(
        &mut out,
        2,
        "state_data_input_sha256",
        STATE_DATA_INPUT_SHA256,
        true,
    )?;
    string(
        &mut out,
        2,
        "population_sha256",
        &loaded.header.population_sha256,
        true,
    )?;
    string(
        &mut out,
        2,
        "admission_sha256",
        &loaded.header.admission_sha256,
        true,
    )?;
    string(&mut out, 2, "dimension", &loaded.header.dimension, true)?;
    number(
        &mut out,
        2,
        "section_count",
        u128::try_from(loaded.header.section_count).expect("section count fits u128"),
        true,
    )?;

    write_hardware(
        &mut out,
        hardware,
        affinity_cpu,
        affinity_frequency.as_ref(),
    )?;
    write_settings(&mut out, settings)?;
    write_memory(&mut out, loaded)?;
    write_map(&mut out, "representations", &loaded.representations)?;
    out.push_str(",\n  \"construction\": ");
    write_summary(&mut out, &loaded.construction)?;
    out.push_str(",\n  \"timings\": [\n");
    for (index, timing) in timings.iter().enumerate() {
        write!(
            out,
            "    {{\"workload\":\"{}\",\"unit\":\"{}\",\"timing\":",
            escape(timing.workload),
            escape(timing.unit)
        )
        .map_err(fmt_error)?;
        write_summary(&mut out, &timing.timing)?;
        out.push('}');
        if index + 1 != timings.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    Ok(out)
}

fn write_hardware(
    out: &mut String,
    hardware: &HardwareMetadata,
    affinity_cpu: Option<usize>,
    affinity_frequency: Option<&FrequencyMetadata>,
) -> Result<(), String> {
    string(out, 2, "commit_sha", &hardware.commit_sha, true)?;
    string(out, 2, "target_triple", &hardware.target_triple, true)?;
    string(out, 2, "cpu_model", &hardware.cpu_model, true)?;
    string(out, 2, "kernel", &hardware.kernel, true)?;
    string(
        out,
        2,
        "cpus_allowed_list",
        &hardware.cpus_allowed_list,
        true,
    )?;
    string(
        out,
        2,
        "mems_allowed_list",
        &hardware.mems_allowed_list,
        true,
    )?;
    string(out, 2, "load_average", &hardware.load_average, true)?;
    string(out, 2, "rustflags", &hardware.rustflags, true)?;
    string(
        out,
        2,
        "cargo_encoded_rustflags",
        &hardware.cargo_encoded_rustflags,
        true,
    )?;
    string(out, 2, "rustc_verbose", &hardware.rustc_verbose, true)?;
    string(out, 2, "cpu0_governor", &hardware.cpu_governor, true)?;
    string(out, 2, "cpu0_current_khz", &hardware.cpu_current_khz, true)?;
    string(out, 2, "cpu0_min_khz", &hardware.cpu_min_khz, true)?;
    string(out, 2, "cpu0_max_khz", &hardware.cpu_max_khz, true)?;
    match affinity_cpu {
        Some(cpu) => number(
            out,
            2,
            "affinity_frequency_cpu",
            u128::try_from(cpu).expect("CPU index fits u128"),
            true,
        )?,
        None => writeln!(out, "  \"affinity_frequency_cpu\": null,").map_err(fmt_error)?,
    }
    let unknown = FrequencyMetadata::unknown();
    let frequency = affinity_frequency.unwrap_or(&unknown);
    string(out, 2, "affinity_cpu_governor", &frequency.governor, true)?;
    string(
        out,
        2,
        "affinity_cpu_current_khz",
        &frequency.current_khz,
        true,
    )?;
    string(out, 2, "affinity_cpu_min_khz", &frequency.min_khz, true)?;
    string(out, 2, "affinity_cpu_max_khz", &frequency.max_khz, true)?;
    string(out, 2, "intel_pstate_no_turbo", &hardware.no_turbo, true)
}

fn write_settings(out: &mut String, settings: PopulationSettings) -> Result<(), String> {
    writeln!(out, "  \"settings\": {{").map_err(fmt_error)?;
    let values = [
        ("warmup_samples", settings.warmup_samples),
        ("measured_samples", settings.measured_samples),
        ("random_reads", settings.random_reads),
        ("sequential_sections", settings.sequential_sections),
        ("volume_queries", settings.volume_queries),
        ("contains_queries", settings.contains_queries),
        ("control_operations", settings.control_operations),
    ];
    for (index, (key, value)) in values.iter().enumerate() {
        number(
            out,
            4,
            key,
            u128::try_from(*value).expect("setting fits u128"),
            index + 1 != values.len(),
        )?;
    }
    writeln!(out, "  }},").map_err(fmt_error)
}

fn write_memory<C>(out: &mut String, loaded: &LoadedCandidate<C>) -> Result<(), String> {
    writeln!(out, "  \"memory\": {{").map_err(fmt_error)?;
    string(out, 4, "rss_protocol", RSS_PROTOCOL, true)?;
    number(
        out,
        4,
        "rss_baseline_kib",
        u128::from(loaded.rss_baseline_kib),
        true,
    )?;
    number(
        out,
        4,
        "rss_loaded_kib",
        u128::from(loaded.rss_loaded_kib),
        true,
    )?;
    signed_number(
        out,
        4,
        "rss_loaded_delta_kib",
        i128::from(loaded.rss_loaded_delta_kib),
        true,
    )?;
    let values = [
        (
            "rss_baseline_high_water_kib",
            u128::from(loaded.rss_baseline_high_water_kib),
        ),
        (
            "rss_loaded_high_water_kib",
            u128::from(loaded.rss_loaded_high_water_kib),
        ),
        (
            "logical_owned_bytes",
            u128::try_from(loaded.logical_owned_bytes).expect("owned bytes fit u128"),
        ),
        (
            "max_owned_bytes",
            u128::try_from(loaded.max_owned_bytes).expect("owned bytes fit u128"),
        ),
        (
            "known_prebaseline_harness_bytes",
            u128::try_from(loaded.known_prebaseline_harness_bytes).expect("harness bytes fit u128"),
        ),
        (
            "construction_transitions",
            u128::try_from(loaded.construction_transitions).expect("transition count fits u128"),
        ),
        (
            "logical_backing_allocations",
            u128::try_from(loaded.logical_backing_allocations).expect("allocation count fits u128"),
        ),
    ];
    for (index, (key, value)) in values.iter().enumerate() {
        number(out, 4, key, *value, index + 1 != values.len())?;
    }
    writeln!(out, "  }},").map_err(fmt_error)
}

fn write_summary(out: &mut String, summary: &SampleSummary) -> Result<(), String> {
    write!(
        out,
        "{{\"operations_per_sample\":{},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"p50_ps_per_op\":{},\"samples_ns\":[",
        summary.operations_per_sample,
        summary.p50_ns,
        summary.p95_ns,
        summary.p99_ns,
        summary.max_ns,
        summary.p50_ps_per_op()
    )
    .map_err(fmt_error)?;
    for (index, sample) in summary.samples_ns.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        write!(out, "{sample}").map_err(fmt_error)?;
    }
    out.push_str("]}");
    Ok(())
}

fn write_map(out: &mut String, key: &str, values: &BTreeMap<String, usize>) -> Result<(), String> {
    write!(out, "  \"{}\": {{", escape(key)).map_err(fmt_error)?;
    for (index, (name, value)) in values.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        write!(out, "\"{}\":{value}", escape(name)).map_err(fmt_error)?;
    }
    out.push('}');
    Ok(())
}

fn string(
    out: &mut String,
    indent: usize,
    key: &str,
    value: &str,
    comma: bool,
) -> Result<(), String> {
    writeln!(
        out,
        "{}\"{}\": \"{}\"{}",
        " ".repeat(indent),
        escape(key),
        escape(value),
        if comma { "," } else { "" }
    )
    .map_err(fmt_error)
}

fn number(
    out: &mut String,
    indent: usize,
    key: &str,
    value: u128,
    comma: bool,
) -> Result<(), String> {
    writeln!(
        out,
        "{}\"{}\": {}{}",
        " ".repeat(indent),
        escape(key),
        value,
        if comma { "," } else { "" }
    )
    .map_err(fmt_error)
}

fn signed_number(
    out: &mut String,
    indent: usize,
    key: &str,
    value: i128,
    comma: bool,
) -> Result<(), String> {
    writeln!(
        out,
        "{}\"{}\": {}{}",
        " ".repeat(indent),
        escape(key),
        value,
        if comma { "," } else { "" }
    )
    .map_err(fmt_error)
}

fn boolean(
    out: &mut String,
    indent: usize,
    key: &str,
    value: bool,
    comma: bool,
) -> Result<(), String> {
    writeln!(
        out,
        "{}\"{}\": {}{}",
        " ".repeat(indent),
        escape(key),
        value,
        if comma { "," } else { "" }
    )
    .map_err(fmt_error)
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(control));
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn fmt_error(_: fmt::Error) -> String {
    "formatting population benchmark report unexpectedly failed".to_owned()
}

pub(super) fn single_cpu(allowed: &str) -> Option<usize> {
    if !allowed.is_empty() && allowed.bytes().all(|byte| byte.is_ascii_digit()) {
        allowed.parse().ok()
    } else {
        None
    }
}

#[derive(Debug)]
struct FrequencyMetadata {
    governor: String,
    current_khz: String,
    min_khz: String,
    max_khz: String,
}

impl FrequencyMetadata {
    fn unknown() -> Self {
        Self {
            governor: "unknown".to_owned(),
            current_khz: "unknown".to_owned(),
            min_khz: "unknown".to_owned(),
            max_khz: "unknown".to_owned(),
        }
    }
}

fn cpu_frequency_metadata(cpu: usize) -> FrequencyMetadata {
    let root = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq");
    FrequencyMetadata {
        governor: read_trimmed(&format!("{root}/scaling_governor")),
        current_khz: read_trimmed(&format!("{root}/scaling_cur_freq")),
        min_khz: read_trimmed(&format!("{root}/scaling_min_freq")),
        max_khz: read_trimmed(&format!("{root}/scaling_max_freq")),
    }
}

fn read_trimmed(path: &str) -> String {
    fs::read_to_string(path).map_or_else(|_| "unknown".to_owned(), |value| value.trim().to_owned())
}
