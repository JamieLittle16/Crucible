use std::env;
use std::fs;
use std::process::{Command, id};

#[derive(Clone, Debug)]
pub(crate) struct HardwareMetadata {
    pub(crate) commit_sha: String,
    pub(crate) rustc_verbose: String,
    pub(crate) target_triple: String,
    pub(crate) cpu_model: String,
    pub(crate) kernel: String,
    pub(crate) cpu_governor: String,
    pub(crate) cpu_current_khz: String,
    pub(crate) cpu_min_khz: String,
    pub(crate) cpu_max_khz: String,
    pub(crate) cpus_allowed_list: String,
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
        cpu_model: cpu_model(),
        kernel: command_output("uname", &["-srmo"])
            .unwrap_or_else(|_| "unknown".to_owned())
            .trim()
            .to_owned(),
        cpu_governor: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        cpu_current_khz: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"),
        cpu_min_khz: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq"),
        cpu_max_khz: read_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq"),
        cpus_allowed_list: cpus_allowed_list(),
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
    fs::read_to_string(path)
        .map_or_else(|_| "unknown".to_owned(), |value| value.trim().to_owned())
}

fn cpu_model() -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("model name\t:")
                    .map(|value| value.trim().to_owned())
            })
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn cpus_allowed_list() -> String {
    let from_status = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("Cpus_allowed_list:")
                    .map(|value| value.trim().to_owned())
            })
        });
    if let Some(value) = from_status {
        return value;
    }

    command_output("taskset", &["-pc", &id().to_string()])
        .unwrap_or_else(|_| "unknown".to_owned())
        .trim()
        .to_owned()
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
        assert!(!metadata.cpus_allowed_list.is_empty());
    }
}
