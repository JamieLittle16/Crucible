//! Reproducible section-representation performance laboratory for M0.3D.
//!
//! This binary is intentionally dependency-light and is not linked into the server. Correctness is
//! qualified elsewhere; this harness measures CPU/tail behaviour and deterministic memory/lifetime
//! diagnostics for already-qualified candidates.

#![forbid(unsafe_code)]

mod corpus;
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

#[derive(Debug)]
enum Invocation {
    Synthetic {
        mode: Mode,
        output: Option<PathBuf>,
    },
    Corpus {
        path: PathBuf,
        decision_requested: bool,
        output: Option<PathBuf>,
    },
}

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
    let Some(invocation) = parse_args()? else {
        print_help();
        return Ok(());
    };
    if cfg!(debug_assertions) {
        return Err("section benchmarks must be built with --release".to_owned());
    }

    match invocation {
        Invocation::Synthetic { mode, output } => run_synthetic(mode, output),
        Invocation::Corpus {
            path,
            decision_requested,
            output,
        } => run_corpus(path, decision_requested, output),
    }
}

fn run_synthetic(mode: Mode, output: Option<PathBuf>) -> Result<(), String> {
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

fn run_corpus(
    path: PathBuf,
    decision_requested: bool,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let checked = corpus::check_corpus(&path, decision_requested)?;
    let artifact = checked.to_json(decision_requested);
    write_artifact(output, &artifact)?;
    println!(
        "section corpus import: purpose={} decision_eligible={} sections={} cells={} states={} PASS",
        checked.header.purpose.as_str(),
        checked.header.decision_eligible(),
        checked.section_count,
        checked.total_cells,
        checked.distinct_state_ids,
    );
    Ok(())
}

fn parse_args() -> Result<Option<Invocation>, String> {
    let mut mode = Mode::Qualification;
    let mut mode_explicit = false;
    let mut corpus: Option<(PathBuf, bool)> = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--smoke") => {
                if mode_explicit {
                    return Err("benchmark mode may be specified only once".to_owned());
                }
                mode = Mode::Smoke;
                mode_explicit = true;
            }
            Some("--qualification") => {
                if mode_explicit {
                    return Err("benchmark mode may be specified only once".to_owned());
                }
                mode = Mode::Qualification;
                mode_explicit = true;
            }
            Some("--corpus-check" | "--corpus-decision-check") => {
                if corpus.is_some() {
                    return Err("corpus mode may be specified only once".to_owned());
                }
                let decision_requested = arg.to_str() == Some("--corpus-decision-check");
                let path = args
                    .next()
                    .ok_or_else(|| "corpus mode requires a corpus path".to_owned())?;
                corpus = Some((PathBuf::from(path), decision_requested));
            }
            Some("--output") => {
                if output.is_some() {
                    return Err("--output may be specified only once".to_owned());
                }
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

    if let Some((path, decision_requested)) = corpus {
        if mode_explicit {
            return Err(
                "--corpus-check/--corpus-decision-check cannot be combined with synthetic modes"
                    .to_owned(),
            );
        }
        return Ok(Some(Invocation::Corpus {
            path,
            decision_requested,
            output,
        }));
    }

    Ok(Some(Invocation::Synthetic { mode, output }))
}

fn print_help() {
    println!(
        "usage: section_bench [--smoke|--qualification] [--output PATH]\n       section_bench --corpus-check CORPUS [--output PATH]\n       section_bench --corpus-decision-check CORPUS [--output PATH]"
    );
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
