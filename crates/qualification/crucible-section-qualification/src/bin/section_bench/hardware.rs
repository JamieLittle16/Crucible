use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, id};

#[derive(Clone, Debug)]
pub(crate) struct HardwareMetadata {
    pub(crate) commit_sha: String,
    pub(crate) rustc_verbose: String,
    pub(crate) target_triple: String,
    pub(crate) cpu_model: String,
    pub(crate) cpu_vendor: String,
    pub(crate) cpu_family: String,
    pub(crate) cpu_model_id: String,
    pub(crate) cpu_stepping: String,
    pub(crate) cpu_microcode: String,
    pub(crate) kernel: String,
    pub(crate) cpu_governor: String,
    pub(crate) cpu_current_khz: String,
    pub(crate) cpu_min_khz: String,
    pub(crate) cpu_max_khz: String,
    pub(crate) cpus_allowed_list: String,
    pub(crate) mems_allowed_list: String,
    pub(crate) online_cpus: String,
    pub(crate) smt_active: String,
    pub(crate) cache_topology: String,
    pub(crate) perf_event_paranoid: String,
    pub(crate) transparent_hugepage: String,
    pub(crate) memory_total_kib: String,
    pub(crate) load_average: String,
    pub(crate) no_turbo: String,
    pub(crate) rustflags: String,
    pub(crate) cargo_encoded_rustflags: String,
}

pub(crate) fn collect() -> Result<HardwareMetadata, String> {
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
    use super::collect;

    #[test]
    fn hardware_metadata_has_nonempty_identity_fields() {
        let metadata = collect().expect("metadata collection should work in repository tests");
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
}
