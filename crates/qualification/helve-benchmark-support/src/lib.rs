//! Shared, dependency-free support for Crucible performance qualification harnesses.
//!
//! This crate is qualification infrastructure only. Production engine code must not depend on it.

#![forbid(unsafe_code)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, id};

/// Machine/toolchain state recorded beside every Crucible microarchitectural benchmark artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardwareMetadata {
    pub commit_sha: String,
    pub rustc_verbose: String,
    pub target_triple: String,
    pub cpu_model: String,
    pub cpu_vendor: String,
    pub cpu_family: String,
    pub cpu_model_id: String,
    pub cpu_stepping: String,
    pub cpu_microcode: String,
    pub kernel: String,
    pub cpu_governor: String,
    pub cpu_current_khz: String,
    pub cpu_min_khz: String,
    pub cpu_max_khz: String,
    pub cpus_allowed_list: String,
    pub mems_allowed_list: String,
    pub online_cpus: String,
    pub smt_active: String,
    pub cache_topology: String,
    pub perf_event_paranoid: String,
    pub transparent_hugepage: String,
    pub memory_total_kib: String,
    pub load_average: String,
    pub no_turbo: String,
    pub rustflags: String,
    pub cargo_encoded_rustflags: String,
}

impl HardwareMetadata {
    /// Returns the sole allowed logical CPU when the process affinity is exactly one CPU.
    #[must_use]
    pub fn single_allowed_cpu(&self) -> Option<u32> {
        self.cpus_allowed_list.parse().ok()
    }

    /// Renders the metadata as one deterministic JSON object.
    #[must_use]
    pub fn to_json(&self) -> String {
        let fields = [
            ("commit_sha", self.commit_sha.as_str()),
            ("rustc_verbose", self.rustc_verbose.as_str()),
            ("target_triple", self.target_triple.as_str()),
            ("cpu_model", self.cpu_model.as_str()),
            ("cpu_vendor", self.cpu_vendor.as_str()),
            ("cpu_family", self.cpu_family.as_str()),
            ("cpu_model_id", self.cpu_model_id.as_str()),
            ("cpu_stepping", self.cpu_stepping.as_str()),
            ("cpu_microcode", self.cpu_microcode.as_str()),
            ("kernel", self.kernel.as_str()),
            ("cpu_governor", self.cpu_governor.as_str()),
            ("cpu_current_khz", self.cpu_current_khz.as_str()),
            ("cpu_min_khz", self.cpu_min_khz.as_str()),
            ("cpu_max_khz", self.cpu_max_khz.as_str()),
            ("cpus_allowed_list", self.cpus_allowed_list.as_str()),
            ("mems_allowed_list", self.mems_allowed_list.as_str()),
            ("online_cpus", self.online_cpus.as_str()),
            ("smt_active", self.smt_active.as_str()),
            ("cache_topology", self.cache_topology.as_str()),
            ("perf_event_paranoid", self.perf_event_paranoid.as_str()),
            ("transparent_hugepage", self.transparent_hugepage.as_str()),
            ("memory_total_kib", self.memory_total_kib.as_str()),
            ("load_average", self.load_average.as_str()),
            ("no_turbo", self.no_turbo.as_str()),
            ("rustflags", self.rustflags.as_str()),
            (
                "cargo_encoded_rustflags",
                self.cargo_encoded_rustflags.as_str(),
            ),
        ];

        let mut output = String::from("{");
        for (index, (name, value)) in fields.iter().enumerate() {
            if index != 0 {
                output.push(',');
            }
            push_json_string(&mut output, name);
            output.push(':');
            push_json_string(&mut output, value);
        }
        output.push('}');
        output
    }
}

/// Collects best-effort machine and exact toolchain provenance for a benchmark run.
///
/// Core repository/toolchain identity is required. Optional Linux topology/state fields degrade to
/// `"unknown"` rather than making qualification tooling itself platform-dependent.
///
/// # Errors
///
/// Returns an error when the current Git commit or `rustc --version --verbose` identity cannot be
/// obtained.
pub fn collect_hardware_metadata() -> Result<HardwareMetadata, String> {
    let commit_sha = command_output("git", &["rev-parse", "HEAD"])?;
    let rustc_verbose = command_output("rustc", &["--version", "--verbose"])?;
    let target_triple = rustc_verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or("unknown")
        .to_owned();

    Ok(HardwareMetadata {
        commit_sha: commit_sha.trim().to_owned(),
        target_triple,
        cpu_model: cpuinfo_value("model name"),
        cpu_vendor: cpuinfo_value("vendor_id"),
        cpu_family: cpuinfo_value("cpu family"),
        cpu_model_id: cpuinfo_value("model"),
        cpu_stepping: cpuinfo_value("stepping"),
        cpu_microcode: cpuinfo_value("microcode"),
        kernel: command_output("uname", &["-srmo"])
            .unwrap_or_else(|_| "unknown".to_owned())
            .trim()
            .to_owned(),
        cpu_governor: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        cpu_current_khz: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"),
        cpu_min_khz: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq"),
        cpu_max_khz: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq"),
        cpus_allowed_list: status_allowed_list("Cpus_allowed_list:").unwrap_or_else(|| {
            command_output("taskset", &["-pc", &id().to_string()])
                .unwrap_or_else(|_| "unknown".to_owned())
                .trim()
                .to_owned()
        }),
        mems_allowed_list: status_allowed_list("Mems_allowed_list:")
            .unwrap_or_else(|| "unknown".to_owned()),
        online_cpus: read_trimmed("/sys/devices/system/cpu/online"),
        smt_active: read_trimmed("/sys/devices/system/cpu/smt/active"),
        cache_topology: cache_topology(),
        perf_event_paranoid: read_trimmed("/proc/sys/kernel/perf_event_paranoid"),
        transparent_hugepage: read_trimmed("/sys/kernel/mm/transparent_hugepage/enabled"),
        memory_total_kib: meminfo_value("MemTotal"),
        load_average: read_trimmed("/proc/loadavg"),
        no_turbo: read_trimmed("/sys/devices/system/cpu/intel_pstate/no_turbo"),
        rustflags: env::var("RUSTFLAGS").unwrap_or_default(),
        cargo_encoded_rustflags: env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default(),
        rustc_verbose,
    })
}

/// Appends one correctly escaped JSON string literal to `output`.
pub fn push_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", u32::from(character))
                    .expect("writing to String cannot fail");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("could not execute {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited unsuccessfully"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} output was not UTF-8"))
}

fn read_trimmed(path: &str) -> String {
    fs::read_to_string(path).map_or_else(|_| "unknown".to_owned(), |value| value.trim().to_owned())
}

fn cpuinfo_value(key: &str) -> String {
    key_value_from_file("/proc/cpuinfo", key)
}

fn meminfo_value(key: &str) -> String {
    key_value_from_file("/proc/meminfo", key)
}

fn key_value_from_file(path: &str, key: &str) -> String {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                (name.trim() == key).then(|| value.trim().to_owned())
            })
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn cache_topology() -> String {
    let root = "/sys/devices/system/cpu/cpu0/cache";
    let Ok(entries) = fs::read_dir(root) else {
        return "unknown".to_owned();
    };
    let mut indexes = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("index"))
        })
        .collect::<Vec<PathBuf>>();
    indexes.sort();

    let mut records = Vec::with_capacity(indexes.len());
    for path in indexes {
        let value = |file: &str| {
            fs::read_to_string(path.join(file))
                .map_or_else(|_| "unknown".to_owned(), |text| text.trim().to_owned())
        };
        records.push(format!(
            "L{}:{}:size={}:line={}:shared={}",
            value("level"),
            value("type"),
            value("size"),
            value("coherency_line_size"),
            value("shared_cpu_list")
        ));
    }
    if records.is_empty() {
        "unknown".to_owned()
    } else {
        records.join(";")
    }
}

fn status_allowed_list(prefix: &str) -> Option<String> {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix(prefix)
                    .map(|value| value.trim().to_owned())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{collect_hardware_metadata, push_json_string};

    #[test]
    fn hardware_metadata_has_nonempty_identity_fields() {
        let metadata = collect_hardware_metadata()
            .expect("metadata collection should work in repository tests");
        assert!(!metadata.commit_sha.is_empty());
        assert!(!metadata.target_triple.is_empty());
        assert!(!metadata.rustc_verbose.is_empty());
        assert!(!metadata.cpu_model.is_empty());
        assert!(!metadata.cpu_vendor.is_empty());
        assert!(!metadata.cpus_allowed_list.is_empty());
        assert!(!metadata.mems_allowed_list.is_empty());
        assert!(!metadata.online_cpus.is_empty());
        assert!(!metadata.cache_topology.is_empty());
        assert!(!metadata.perf_event_paranoid.is_empty());
    }

    #[test]
    fn json_string_escaping_is_deterministic() {
        let mut output = String::new();
        push_json_string(&mut output, "a\"b\\c\n\t");
        assert_eq!(output, "\"a\\\"b\\\\c\\n\\t\"");
    }
}
