use std::fmt::{self, Write as _};

use crucible_generated::{STATE_DATA_GENERATION_SHA256, STATE_DATA_INPUT_SHA256};
use crucible_section_qualification::{DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};

use crate::hardware::HardwareMetadata;
use crate::measure::BenchmarkOutput;
use crate::model::{BENCH_SEED, HARNESS_SCHEMA, HARNESS_VERSION, Mode, Settings};

pub(crate) fn render_report(
    mode: Mode,
    settings: Settings,
    benchmark: &BenchmarkOutput,
    hardware: &HardwareMetadata,
) -> Result<String, String> {
    let mut output = String::new();
    writeln!(output, "{{").map_err(fmt_error)?;
    write_identity(&mut output, mode, hardware)?;
    write_settings(&mut output, settings)?;
    write_memory(&mut output, benchmark)?;
    write_lifetimes(&mut output, benchmark)?;
    write_timings(&mut output, benchmark)?;
    writeln!(output, "}}").map_err(fmt_error)?;
    Ok(output)
}

fn write_identity(
    output: &mut String,
    mode: Mode,
    hardware: &HardwareMetadata,
) -> Result<(), String> {
    writeln!(output, "  \"schema\": {HARNESS_SCHEMA},").map_err(fmt_error)?;
    writeln!(output, "  \"harness_version\": \"{HARNESS_VERSION}\",").map_err(fmt_error)?;
    writeln!(output, "  \"mode\": \"{}\",", mode.as_str()).map_err(fmt_error)?;
    write_string(output, "commit_sha", &hardware.commit_sha)?;
    writeln!(output, "  \"minecraft_version\": \"{MINECRAFT_VERSION}\",").map_err(fmt_error)?;
    writeln!(output, "  \"protocol_version\": {PROTOCOL_VERSION},").map_err(fmt_error)?;
    writeln!(output, "  \"data_version\": {DATA_VERSION},").map_err(fmt_error)?;
    writeln!(
        output,
        "  \"state_data_input_sha256\": \"{STATE_DATA_INPUT_SHA256}\","
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "  \"state_data_generation_sha256\": \"{STATE_DATA_GENERATION_SHA256}\","
    )
    .map_err(fmt_error)?;
    writeln!(output, "  \"benchmark_seed\": \"{BENCH_SEED:016x}\",").map_err(fmt_error)?;
    writeln!(output, "  \"build_profile\": \"release\",").map_err(fmt_error)?;
    writeln!(
        output,
        "  \"codegen_policy\": \"lto=thin,codegen-units=1,panic=abort\","
    )
    .map_err(fmt_error)?;
    write_string(output, "target_triple", &hardware.target_triple)?;
    write_string(output, "cpu_model", &hardware.cpu_model)?;
    write_string(output, "kernel", &hardware.kernel)?;
    write_string(output, "cpu_governor", &hardware.cpu_governor)?;
    write_string(output, "cpu_current_khz", &hardware.cpu_current_khz)?;
    write_string(output, "cpu_min_khz", &hardware.cpu_min_khz)?;
    write_string(output, "cpu_max_khz", &hardware.cpu_max_khz)?;
    write_string(output, "cpus_allowed_list", &hardware.cpus_allowed_list)?;
    write_string(output, "load_average", &hardware.load_average)?;
    write_string(output, "intel_pstate_no_turbo", &hardware.no_turbo)?;
    write_string(output, "rustflags", &hardware.rustflags)?;
    write_string(
        output,
        "cargo_encoded_rustflags",
        &hardware.cargo_encoded_rustflags,
    )?;
    write_string(output, "rustc_verbose", &hardware.rustc_verbose)
}

fn write_string(output: &mut String, key: &str, value: &str) -> Result<(), String> {
    writeln!(
        output,
        "  \"{}\": \"{}\",",
        json_escape(key),
        json_escape(value)
    )
    .map_err(fmt_error)
}

fn write_settings(output: &mut String, settings: Settings) -> Result<(), String> {
    writeln!(output, "  \"settings\": {{").map_err(fmt_error)?;
    writeln!(
        output,
        "    \"warmup_samples\": {},",
        settings.warmup_samples
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"measured_samples\": {},",
        settings.measured_samples
    )
    .map_err(fmt_error)?;
    writeln!(output, "    \"random_reads\": {},", settings.random_reads).map_err(fmt_error)?;
    writeln!(output, "    \"full_scans\": {},", settings.full_scans).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"volume_queries\": {},",
        settings.volume_queries
    )
    .map_err(fmt_error)?;
    writeln!(output, "    \"mutations\": {},", settings.mutations).map_err(fmt_error)?;
    writeln!(
        output,
        "    \"contains_queries\": {},",
        settings.contains_queries
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"promotion_samples\": {},",
        settings.promotion_samples
    )
    .map_err(fmt_error)?;
    writeln!(
        output,
        "    \"lifetime_mutations\": {}",
        settings.lifetime_mutations
    )
    .map_err(fmt_error)?;
    writeln!(output, "  }},").map_err(fmt_error)
}

fn write_memory(output: &mut String, benchmark: &BenchmarkOutput) -> Result<(), String> {
    writeln!(output, "  \"memory\": [").map_err(fmt_error)?;
    for (index, record) in benchmark.memory.iter().enumerate() {
        let suffix = comma_suffix(index, benchmark.memory.len());
        writeln!(
            output,
            "    {{\"candidate\":\"{}\",\"production_candidate\":{},\"pattern\":\"{}\",\"pool_cardinality\":{},\"actual_cardinality\":{},\"representation\":\"{}\",\"owned_bytes\":{},\"construction_logical_allocations\":{},\"construction_transitions\":{}}}{suffix}",
            json_escape(record.candidate),
            record.production_candidate,
            json_escape(record.pattern),
            record.pool_cardinality,
            record.actual_cardinality,
            json_escape(&record.representation),
            record.owned_bytes,
            record.construction_logical_allocations,
            record.construction_transitions,
        )
        .map_err(fmt_error)?;
    }
    writeln!(output, "  ],").map_err(fmt_error)
}

fn write_lifetimes(output: &mut String, benchmark: &BenchmarkOutput) -> Result<(), String> {
    writeln!(output, "  \"lifetimes\": [").map_err(fmt_error)?;
    for (index, record) in benchmark.lifetimes.iter().enumerate() {
        let suffix = comma_suffix(index, benchmark.lifetimes.len());
        writeln!(
            output,
            "    {{\"candidate\":\"{}\",\"pattern\":\"{}\",\"pool_cardinality\":{},\"mutation_count\":{},\"representation_transitions\":{},\"logical_backing_allocations\":{},\"peak_owned_bytes\":{},\"final_owned_bytes\":{},\"final_representation\":\"{}\"}}{suffix}",
            json_escape(record.candidate),
            json_escape(record.pattern),
            record.pool_cardinality,
            record.mutation_count,
            record.representation_transitions,
            record.logical_backing_allocations,
            record.peak_owned_bytes,
            record.final_owned_bytes,
            json_escape(&record.final_representation),
        )
        .map_err(fmt_error)?;
    }
    writeln!(output, "  ],").map_err(fmt_error)
}

fn write_timings(output: &mut String, benchmark: &BenchmarkOutput) -> Result<(), String> {
    writeln!(output, "  \"timings\": [").map_err(fmt_error)?;
    for (index, record) in benchmark.timings.iter().enumerate() {
        let suffix = comma_suffix(index, benchmark.timings.len());
        write!(
            output,
            "    {{\"candidate\":\"{}\",\"production_candidate\":{},\"workload\":\"{}\",\"pattern\":\"{}\",\"pool_cardinality\":{},\"actual_cardinality\":{},\"representation\":\"{}\",\"unit\":\"{}\",\"operations_per_sample\":{},\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{},\"p50_ps_per_op\":{},\"samples_ns\":[",
            json_escape(record.candidate),
            record.production_candidate,
            json_escape(&record.workload),
            json_escape(record.pattern),
            record.pool_cardinality,
            record.actual_cardinality,
            json_escape(&record.representation),
            json_escape(record.unit),
            record.timing.operations_per_sample,
            record.timing.p50_ns,
            record.timing.p95_ns,
            record.timing.p99_ns,
            record.timing.max_ns,
            record.timing.p50_ps_per_op(),
        )
        .map_err(fmt_error)?;
        for (sample_index, sample) in record.timing.samples_ns.iter().enumerate() {
            if sample_index != 0 {
                output.push(',');
            }
            write!(output, "{sample}").map_err(fmt_error)?;
        }
        writeln!(output, "]}}{suffix}").map_err(fmt_error)?;
    }
    writeln!(output, "  ]").map_err(fmt_error)
}

fn comma_suffix(index: usize, len: usize) -> &'static str {
    if index + 1 == len { "" } else { "," }
}

fn json_escape(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(result, "\\u{:04x}", u32::from(character));
            }
            character => result.push(character),
        }
    }
    result
}

fn fmt_error(_: fmt::Error) -> String {
    "formatting benchmark report unexpectedly failed".to_owned()
}

#[cfg(test)]
mod tests {
    use super::json_escape;

    #[test]
    fn json_escape_handles_control_and_quote_characters() {
        assert_eq!(json_escape("a\n\"b\\c"), "a\\n\\\"b\\\\c");
    }
}
