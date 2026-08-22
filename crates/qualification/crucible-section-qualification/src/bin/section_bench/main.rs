//! Reproducible section-representation performance laboratory for M0.3D.
//!
//! This binary is intentionally dependency-light and is not linked into the server. Correctness is
//! qualified elsewhere; this harness measures CPU/tail behaviour and deterministic memory/lifetime
//! diagnostics for already-qualified candidates.

#![forbid(unsafe_code)]

mod hardware;
mod measure;
mod model;
mod report;
mod workloads;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use crucible_generated::BLOCK_STATE_COUNT;

use crate::model::Mode;

const HIGHEST_SYNTHETIC_STATE: usize = 5_511;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Some((mode, output)) = parse_args()? else {
        println!("usage: section_bench [--smoke|--qualification] [--output PATH]");
        return Ok(());
    };
    if cfg!(debug_assertions) {
        return Err("section benchmarks must be built with --release".to_owned());
    }
    if BLOCK_STATE_COUNT <= HIGHEST_SYNTHETIC_STATE {
        return Err(
            "target state universe is too small for synthetic benchmark streams".to_owned(),
        );
    }

    let settings = mode.settings();
    let cases = workloads::cases_for(mode);
    let benchmark = measure::run_all(&cases, settings);
    let hardware = hardware::collect()?;
    let artifact = report::render_report(mode, settings, &benchmark, &hardware)?;
    write_artifact(output, &artifact)?;

    println!(
        "section benchmark: mode={} timing_records={} memory_records={} lifetime_records={} complete",
        mode.as_str(),
        benchmark.timings.len(),
        benchmark.memory.len(),
        benchmark.lifetimes.len()
    );
    Ok(())
}

fn parse_args() -> Result<Option<(Mode, Option<PathBuf>)>, String> {
    let mut mode = Mode::Qualification;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--smoke") => mode = Mode::Smoke,
            Some("--qualification") => mode = Mode::Qualification,
            Some("--output") => {
                let path = args
                    .next()
                    .ok_or_else(|| "--output requires a path".to_owned())?;
                output = Some(PathBuf::from(path));
            }
            Some("--help" | "-h") => return Ok(None),
            Some(other) => return Err(format!("unknown benchmark option: {other}")),
            None => return Err("benchmark arguments must be valid UTF-8".to_owned()),
        }
    }
    Ok(Some((mode, output)))
}

fn write_artifact(path: Option<PathBuf>, artifact: &str) -> Result<(), String> {
    let Some(path) = path else {
        print!("{artifact}");
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    fs::write(&path, artifact)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    println!("section benchmark artifact: {}", path.display());
    Ok(())
}
