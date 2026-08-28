use std::fmt::{self, Write as _};

use helve_generated::{BLOCK_STATE_COUNT, STATE_DATA_GENERATION_SHA256, STATE_DATA_INPUT_SHA256};
use helve_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};

use crate::hardware::HardwareMetadata;
use crate::model::{BenchSection, CaseSpec, PROMOTION_TARGETS, SampleSummary, TimingRecord};

use super::measure::{TargetSyntheticOutput, control_workload_name, expected_timing_records};
use super::{
    BUILD_PROFILE, CODEGEN_POLICY, REPORT_SCHEMA, REPORT_VERSION, TargetSyntheticMode,
    TargetSyntheticSettings,
};

pub(super) fn render<C: BenchSection>(
    mode: TargetSyntheticMode,
    settings: TargetSyntheticSettings,
    cases: &[CaseSpec],
    measured: &TargetSyntheticOutput,
    hardware: &HardwareMetadata,
) -> Result<String, String> {
    let expected_records = expected_timing_records(cases.len());
    if measured.timings.len() != expected_records {
        return Err(format!(
            "target synthetic timing count mismatch: expected {expected_records}, got {}",
            measured.timings.len()
        ));
    }

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
    string(&mut out, 2, "scope", "synthetic-mechanism-stress", true)?;
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
    write_hardware(&mut out, hardware)?;
    write_settings(&mut out, settings, cases.len())?;
    out.push_str("  \"promotion_targets\": [");
    for (index, target) in PROMOTION_TARGETS.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        write!(out, "{target}").map_err(fmt_error)?;
    }
    out.push_str("],\n");
    write!(
        out,
        "  \"control\": {{\"workload\":\"{}\",\"unit\":\"iteration\",\"timing\":",
        control_workload_name()
    )
    .map_err(fmt_error)?;
    write_summary(&mut out, &measured.control)?;
    out.push_str("},\n  \"timings\": [\n");
    for (index, timing) in measured.timings.iter().enumerate() {
        write_timing(&mut out, timing)?;
        if index + 1 != measured.timings.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    Ok(out)
}

fn write_hardware(out: &mut String, hardware: &HardwareMetadata) -> Result<(), String> {
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
    string(out, 2, "intel_pstate_no_turbo", &hardware.no_turbo, true)
}

fn write_settings(
    out: &mut String,
    settings: TargetSyntheticSettings,
    case_count: usize,
) -> Result<(), String> {
    writeln!(out, "  \"settings\": {{").map_err(fmt_error)?;
    let values = [
        ("warmup_samples", settings.benchmark.warmup_samples),
        ("measured_samples", settings.benchmark.measured_samples),
        ("mutations", settings.benchmark.mutations),
        ("promotion_samples", settings.benchmark.promotion_samples),
        ("control_operations", settings.control_operations),
        ("case_count", case_count),
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

fn write_timing(out: &mut String, timing: &TimingRecord) -> Result<(), String> {
    write!(
        out,
        "    {{\"workload\":\"{}\",\"pattern\":\"{}\",\"pool_cardinality\":{},\"actual_cardinality\":{},\"representation\":\"{}\",\"unit\":\"{}\",\"timing\":",
        escape(&timing.workload),
        escape(timing.pattern),
        timing.pool_cardinality,
        timing.actual_cardinality,
        escape(&timing.representation),
        escape(timing.unit),
    )
    .map_err(fmt_error)?;
    write_summary(out, &timing.timing)?;
    out.push('}');
    Ok(())
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
    "formatting target synthetic benchmark report unexpectedly failed".to_owned()
}
