//! Repository automation entry point.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const PINNED_RUST: &str = "1.97.1";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("guard") => guard(),
        Some("vanilla") => vanilla(args.next().as_deref()),
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
    ] {
        if !root.join(required).is_file() {
            failures.push(format!("required repository file is missing: {required}"));
        }
    }

    check_toolchain_pin(&root, &mut failures);
    check_forbidden_tracked_files(&root, &mut failures);

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
            || path.starts_with("vanilla/source/")
            || path.starts_with("vanilla/artifacts/")
            || path.starts_with("vanilla/private/");

        if forbidden {
            failures.push(format!("forbidden tracked source/artifact: {path}"));
        }
    }
}

fn vanilla(command: Option<&str>) -> ExitCode {
    match command {
        Some("status") => {
            println!("vanilla atlas: source pinned; index not built");
            ExitCode::SUCCESS
        }
        Some(command) => failure(&format!("vanilla command not implemented yet: {command}")),
        None => failure("usage: cargo xtask vanilla <command>"),
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("xtask must live under tools/xtask in the Crucible workspace")
}

fn help() {
    println!("crucible xtask");
    println!("  guard           run architecture/repository guards");
    println!("  vanilla status  report Vanilla Atlas bootstrap status");
}

fn failure(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}
