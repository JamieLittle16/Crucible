//! Repository automation entry point.

#![forbid(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PINNED_RUST: &str = "1.97.1";

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(OsStr::to_str) {
        Some("guard") => guard(),
        Some("vanilla") => vanilla(args.collect()),
        Some(command) => failure(&format!("unknown xtask command: {command}")),
        None => {
            help();
            ExitCode::SUCCESS
        }
    }
}

fn guard() -> ExitCode {
    let root = workspace_root();
    let mut failures = Vec::new();

    for required in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "docs/README.md",
        "vanilla/vanilla.lock.toml",
        "tools/vanilla_atlas.py",
        "vanilla/atlas/README.md",
        "vanilla/atlas/SCHEMA.md",
        "vanilla/frontiers/m0-world-kernel.json",
        "vanilla/reports/26.2-source-audit.json",
        "vanilla/reports/26.2-source-audit.md",
    ] {
        if !root.join(required).is_file() {
            failures.push(format!("required repository file is missing: {required}"));
        }
    }

    check_toolchain_pin(&root, &mut failures);
    check_forbidden_tracked_files(&root, &mut failures);
    check_vanilla_pin_consistency(&root, &mut failures);

    if failures.is_empty() {
        println!("architecture guard: all bootstrap checks passed");
        ExitCode::SUCCESS
    } else {
        for failure in failures {
            eprintln!("guard failure: {failure}");
        }
        ExitCode::FAILURE
    }
}

fn check_toolchain_pin(root: &Path, failures: &mut Vec<String>) {
    for file in ["Cargo.toml", "rust-toolchain.toml"] {
        match fs::read_to_string(root.join(file)) {
            Ok(contents) if contents.contains(PINNED_RUST) => {}
            Ok(_) => failures.push(format!("{file} does not pin Rust {PINNED_RUST}")),
            Err(error) => failures.push(format!("could not read {file}: {error}")),
        }
    }
}

fn check_forbidden_tracked_files(root: &Path, failures: &mut Vec<String>) {
    let output = match Command::new("git")
        .args(["-C", root.to_string_lossy().as_ref(), "ls-files", "-z"])
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            failures.push(format!("could not execute git ls-files: {error}"));
            return;
        }
    };

    if !output.status.success() {
        failures.push("git ls-files failed".to_owned());
        return;
    }

    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let path = String::from_utf8_lossy(raw);
        let forbidden = path == "mc-src.zip"
            || path.ends_with(".jar")
            || path.starts_with(".crucible/")
            || path.starts_with("vanilla/source/")
            || path.starts_with("vanilla/artifacts/")
            || path.starts_with("vanilla/private/");

        if forbidden {
            failures.push(format!("forbidden tracked source/artifact: {path}"));
        }
    }
}

fn check_vanilla_pin_consistency(root: &Path, failures: &mut Vec<String>) {
    let lock = match fs::read_to_string(root.join("vanilla/vanilla.lock.toml")) {
        Ok(contents) => contents,
        Err(error) => {
            failures.push(format!("could not read vanilla/vanilla.lock.toml: {error}"));
            return;
        }
    };
    let report = match fs::read_to_string(root.join("vanilla/reports/26.2-source-audit.json")) {
        Ok(contents) => contents,
        Err(error) => {
            failures.push(format!("could not read source audit report: {error}"));
            return;
        }
    };

    for expected in [
        "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
        "\"minecraft_version\": \"26.2\"",
        "\"protocol_version\": 776",
        "\"world_version\": 4903",
        "\"java_files\": 4849",
    ] {
        let present = if expected.starts_with('"') {
            report.contains(expected)
        } else {
            lock.contains(expected) && report.contains(expected)
        };
        if !present {
            failures.push(format!("vanilla pin/audit consistency marker missing: {expected}"));
        }
    }
}

fn vanilla(args: Vec<OsString>) -> ExitCode {
    let root = workspace_root();
    let script = root.join("tools/vanilla_atlas.py");
    if !script.is_file() {
        return failure("tools/vanilla_atlas.py is missing");
    }

    let python = match find_python() {
        Some(python) => python,
        None => return failure("Python 3 is required for Vanilla Atlas tooling"),
    };

    let status = Command::new(python)
        .arg(script)
        .args(args)
        .current_dir(root)
        .status();

    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1).clamp(1, 255)).unwrap_or(1)),
        Err(error) => failure(&format!("could not launch Vanilla Atlas: {error}")),
    }
}

fn find_python() -> Option<OsString> {
    if let Some(explicit) = env::var_os("PYTHON") {
        if python_works(&explicit) {
            return Some(explicit);
        }
    }
    for candidate in ["python3", "python"] {
        let candidate = OsString::from(candidate);
        if python_works(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn python_works(executable: &OsStr) -> bool {
    Command::new(executable)
        .args(["-c", "import sys; raise SystemExit(sys.version_info < (3, 11))"])
        .status()
        .is_ok_and(|status| status.success())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("xtask must live under tools/xtask in the Crucible workspace")
}

fn help() {
    println!("crucible xtask");
    println!("  guard             run architecture/repository guards");
    println!("  vanilla <args..>  run Vanilla Atlas tooling");
    println!();
    println!("examples:");
    println!("  cargo xtask vanilla verify-source /path/to/mc-src.zip");
    println!("  cargo xtask vanilla index /path/to/mc-src.zip");
    println!("  cargo xtask vanilla frontier m0-world-kernel");
    println!("  cargo xtask vanilla next m0-world-kernel");
}

fn failure(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}
