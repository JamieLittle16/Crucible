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
mod population;
mod report;
mod workloads;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crucible_generated::BLOCK_STATE_COUNT;

use crate::model::Mode;
use crate::population::PopulationMode;

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
    Population {
        pack: PathBuf,
        candidate: String,
        mode: PopulationMode,
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
        } => run_corpus(&path, decision_requested, output),
        Invocation::Population {
            pack,
            candidate,
            mode,
            output,
        } => run_population(&pack, &candidate, mode, output),
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
    path: &Path,
    decision_requested: bool,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let checked = corpus::check_corpus(path, decision_requested)?;
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

fn run_population(
    pack: &Path,
    candidate: &str,
    mode: PopulationMode,
    output: Option<PathBuf>,
) -> Result<(), String> {
    let artifact = population::run(pack, candidate, mode)?;
    write_artifact(output, &artifact)?;
    println!(
        "section population benchmark: mode={} candidate={} pack={} complete",
        mode.as_str(),
        candidate,
        pack.display()
    );
    Ok(())
}

fn parse_args() -> Result<Option<Invocation>, String> {
    let mut synthetic_mode = Mode::Qualification;
    let mut synthetic_mode_explicit = false;
    let mut corpus: Option<(PathBuf, bool)> = None;
    let mut population_pack: Option<PathBuf> = None;
    let mut population_candidate: Option<String> = None;
    let mut population_mode: Option<PopulationMode> = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--smoke") => {
                if synthetic_mode_explicit {
                    return Err("synthetic benchmark mode may be specified only once".to_owned());
                }
                synthetic_mode = Mode::Smoke;
                synthetic_mode_explicit = true;
            }
            Some("--qualification") => {
                if synthetic_mode_explicit {
                    return Err("synthetic benchmark mode may be specified only once".to_owned());
                }
                synthetic_mode = Mode::Qualification;
                synthetic_mode_explicit = true;
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
            Some("--population-pack") => {
                if population_pack.is_some() {
                    return Err("--population-pack may be specified only once".to_owned());
                }
                population_pack = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--population-pack requires a path".to_owned())?,
                ));
            }
            Some("--candidate") => {
                if population_candidate.is_some() {
                    return Err("--candidate may be specified only once".to_owned());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "--candidate requires a candidate name".to_owned())?;
                let candidate = value
                    .into_string()
                    .map_err(|_| "candidate name must be valid UTF-8".to_owned())?;
                population_candidate = Some(candidate);
            }
            Some("--population-smoke") => {
                if population_mode.is_some() {
                    return Err("population benchmark mode may be specified only once".to_owned());
                }
                population_mode = Some(PopulationMode::Smoke);
            }
            Some("--population-qualification") => {
                if population_mode.is_some() {
                    return Err("population benchmark mode may be specified only once".to_owned());
                }
                population_mode = Some(PopulationMode::Qualification);
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

    let population_requested =
        population_pack.is_some() || population_candidate.is_some() || population_mode.is_some();
    if population_requested {
        if synthetic_mode_explicit || corpus.is_some() {
            return Err(
                "population benchmark options cannot be combined with synthetic/corpus modes"
                    .to_owned(),
            );
        }
        let pack = population_pack
            .ok_or_else(|| "population benchmark requires --population-pack PATH".to_owned())?;
        let candidate = population_candidate
            .ok_or_else(|| "population benchmark requires --candidate NAME".to_owned())?;
        let mode = population_mode.ok_or_else(|| {
            "population benchmark requires --population-smoke or --population-qualification"
                .to_owned()
        })?;
        return Ok(Some(Invocation::Population {
            pack,
            candidate,
            mode,
            output,
        }));
    }

    if let Some((path, decision_requested)) = corpus {
        if synthetic_mode_explicit {
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

    Ok(Some(Invocation::Synthetic {
        mode: synthetic_mode,
        output,
    }))
}

fn print_help() {
    println!(
        "usage: section_bench [--smoke|--qualification] [--output PATH]\n       section_bench --corpus-check CORPUS [--output PATH]\n       section_bench --corpus-decision-check CORPUS [--output PATH]\n       section_bench --population-pack PACK --candidate NAME (--population-smoke|--population-qualification) [--output PATH]"
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
