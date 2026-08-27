//! Shared, dependency-free support for Crucible performance qualification harnesses.
//!
//! This crate is qualification infrastructure only. Production engine code must not depend on it.

#![forbid(unsafe_code)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, id};

/// Parts-per-million scale used for benchmark ratios and rates.
pub const RATIO_SCALE_PPM: u128 = 1_000_000;

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

/// Robust summary of one non-empty benchmark latency/sample distribution.
///
/// Percentiles use the nearest-rank definition. `top_1pct_mean` and `top_0_1pct_mean` average the
/// slowest tails rather than trusting one maximum sample. Ratio fields are expressed in ppm against
/// `p50`, making tail amplification directly comparable across workloads with different scales.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistributionStats {
    pub count: usize,
    pub min: u128,
    pub p50: u128,
    pub p90: u128,
    pub p95: u128,
    pub p99: u128,
    pub p999: u128,
    pub max: u128,
    pub mean: u128,
    pub mad: u128,
    pub iqr: u128,
    pub top_1pct_mean: u128,
    pub top_0_1pct_mean: u128,
    pub relative_mad_ppm: u128,
    pub p99_to_p50_ppm: u128,
    pub p999_to_p50_ppm: u128,
    pub max_to_p50_ppm: u128,
}

impl DistributionStats {
    /// Summarizes a non-empty set of unsigned benchmark samples.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty sample set or if checked aggregate/ratio arithmetic overflows.
    pub fn from_samples(values: &[u128]) -> Result<Self, String> {
        if values.is_empty() {
            return Err("cannot summarize an empty sample set".to_owned());
        }

        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let p25 = quantile_permille_sorted(&sorted, 250)?;
        let p50 = quantile_permille_sorted(&sorted, 500)?;
        let p75 = quantile_permille_sorted(&sorted, 750)?;
        let p90 = quantile_permille_sorted(&sorted, 900)?;
        let p95 = quantile_permille_sorted(&sorted, 950)?;
        let p99 = quantile_permille_sorted(&sorted, 990)?;
        let p999 = quantile_permille_sorted(&sorted, 999)?;
        let sum = checked_sum(&sorted)?;
        let count = u128::try_from(sorted.len())
            .map_err(|_| "sample count does not fit u128".to_owned())?;
        let mean = sum
            .checked_div(count)
            .ok_or_else(|| "sample count must be positive".to_owned())?;

        let mut deviations = Vec::with_capacity(sorted.len());
        deviations.extend(sorted.iter().map(|value| value.abs_diff(p50)));
        deviations.sort_unstable();
        let mad = quantile_permille_sorted(&deviations, 500)?;
        let max = sorted[sorted.len() - 1];

        Ok(Self {
            count: sorted.len(),
            min: sorted[0],
            p50,
            p90,
            p95,
            p99,
            p999,
            max,
            mean,
            mad,
            iqr: p75.saturating_sub(p25),
            top_1pct_mean: upper_tail_mean(&sorted, 10)?,
            top_0_1pct_mean: upper_tail_mean(&sorted, 1)?,
            relative_mad_ppm: ratio_ppm(mad, p50)?,
            p99_to_p50_ppm: ratio_ppm(p99, p50)?,
            p999_to_p50_ppm: ratio_ppm(p999, p50)?,
            max_to_p50_ppm: ratio_ppm(max, p50)?,
        })
    }
}

/// Computes `numerator / denominator` in parts per million with checked arithmetic.
///
/// # Errors
///
/// Returns an error when multiplication overflows or `denominator` is zero.
pub fn ratio_ppm(numerator: u128, denominator: u128) -> Result<u128, String> {
    numerator
        .checked_mul(RATIO_SCALE_PPM)
        .ok_or_else(|| "ratio numerator overflow".to_owned())?
        .checked_div(denominator)
        .ok_or_else(|| "ratio denominator must be positive".to_owned())
}

/// Computes a success fraction in parts per million with checked conversion/arithmetic.
///
/// # Errors
///
/// Returns an error when counts do not fit the arithmetic domain or `total` is zero.
pub fn rate_ppm(successes: usize, total: usize) -> Result<u128, String> {
    let successes =
        u128::try_from(successes).map_err(|_| "success count does not fit u128".to_owned())?;
    let total = u128::try_from(total).map_err(|_| "total count does not fit u128".to_owned())?;
    ratio_ppm(successes, total)
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

fn checked_sum(values: &[u128]) -> Result<u128, String> {
    values.iter().try_fold(0_u128, |sum, value| {
        sum.checked_add(*value)
            .ok_or_else(|| "sample sum overflow".to_owned())
    })
}

fn quantile_permille_sorted(sorted: &[u128], permille: usize) -> Result<u128, String> {
    if sorted.is_empty() {
        return Err("cannot compute a quantile of an empty sample set".to_owned());
    }
    if permille == 0 || permille > 1_000 {
        return Err("quantile permille must be in 1..=1000".to_owned());
    }
    let numerator = sorted
        .len()
        .checked_mul(permille)
        .ok_or_else(|| "quantile rank overflow".to_owned())?;
    let rank = numerator
        .checked_add(999)
        .ok_or_else(|| "quantile rank overflow".to_owned())?
        / 1_000;
    Ok(sorted[rank.saturating_sub(1).min(sorted.len() - 1)])
}

fn upper_tail_mean(sorted: &[u128], tail_permille: usize) -> Result<u128, String> {
    if sorted.is_empty() {
        return Err("cannot summarize an empty tail".to_owned());
    }
    if tail_permille == 0 || tail_permille > 1_000 {
        return Err("tail permille must be in 1..=1000".to_owned());
    }
    let numerator = sorted
        .len()
        .checked_mul(tail_permille)
        .ok_or_else(|| "tail rank overflow".to_owned())?;
    let count = numerator
        .checked_add(999)
        .ok_or_else(|| "tail rank overflow".to_owned())?
        / 1_000;
    let tail = &sorted[sorted.len() - count.max(1)..];
    let divisor =
        u128::try_from(tail.len()).map_err(|_| "tail sample count does not fit u128".to_owned())?;
    checked_sum(tail)?
        .checked_div(divisor)
        .ok_or_else(|| "tail sample count must be positive".to_owned())
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
    use super::{DistributionStats, RATIO_SCALE_PPM, collect_hardware_metadata, push_json_string};

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

    #[test]
    fn latency_distribution_uses_nearest_rank_and_robust_spread() {
        let samples = (1_u128..=1_000).collect::<Vec<_>>();
        let stats = DistributionStats::from_samples(&samples).expect("valid distribution");
        assert_eq!(stats.count, 1_000);
        assert_eq!(stats.min, 1);
        assert_eq!(stats.p50, 500);
        assert_eq!(stats.p90, 900);
        assert_eq!(stats.p95, 950);
        assert_eq!(stats.p99, 990);
        assert_eq!(stats.p999, 999);
        assert_eq!(stats.max, 1_000);
        assert_eq!(stats.mean, 500);
        assert_eq!(stats.mad, 250);
        assert_eq!(stats.iqr, 500);
        assert_eq!(stats.top_1pct_mean, 995);
        assert_eq!(stats.top_0_1pct_mean, 1_000);
        assert_eq!(stats.relative_mad_ppm, 500_000);
        assert_eq!(stats.p99_to_p50_ppm, 1_980_000);
        assert_eq!(stats.p999_to_p50_ppm, 1_998_000);
        assert_eq!(stats.max_to_p50_ppm, 2 * RATIO_SCALE_PPM);
    }

    #[test]
    fn latency_distribution_rejects_empty_samples() {
        assert!(DistributionStats::from_samples(&[]).is_err());
    }
}