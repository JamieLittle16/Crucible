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
use std::ffi::OsString;
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

#[derive(Debug)]
struct ParsedArgs {
    synthetic_mode: Mode,
    synthetic_mode_explicit: bool,
    corpus: Option<(PathBuf, bool)>,
    population_pack: Option<PathBuf>,
    population_candidate: Option<String>,
    population_mode: Option<PopulationMode>,
    output: Option<PathBuf>,
}

impl ParsedArgs {
    const fn new() -> Self {
        Self {
            synthetic_mode: Mode::Qualification,
            synthetic_mode_explicit: false,
            corpus: None,
            population_pack: None,
            population_candidate: None,
            population_mode: None,
            output: None,
        }
    }

    fn finish(self) -> Result<Invocation, String> {
        let population_requested = self.population_pack.is_some()
            || self.population_candidate.is_some()
            || self.population_mode.is_some();
        if population_requested {
            if self.synthetic_mode_explicit || self.corpus.is_some() {
                return Err(
                    "population benchmark options cannot be combined with synthetic/corpus modes"
                        .to_owned(),
                );
            }
            let pack = self
                .population_pack
                .ok_or_else(|| "population benchmark requires --population-pack PATH".to_owned())?;
            let candidate = self
                .population_candidate
                .ok_or_else(|| "population benchmark requires --candidate NAME".to_owned())?;
            let mode = self.population_mode.ok_or_else(|| {
                "population benchmark requires --population-smoke or --population-qualification"
                    .to_owned()
            })?;
            return Ok(Invocation::Population {
                pack,
                candidate,
                mode,
                output: self.output,
            });
        }

        if let Some((path, decision_requested)) = self.corpus {
            if self.synthetic_mode_explicit {
                return Err(
                    "--corpus-check/--corpus-decision-check cannot be combined with synthetic modes"
                        .to_owned(),
                );
            }
            return Ok(Invocation::Corpus {
                path,
                decision_requested,
                output: self.output,
            });
        }

        Ok(Invocation::Synthetic {
            mode: self.synthetic_mode,
            output: self.output,
        })
    }
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
    let mut parsed = ParsedArgs::new();
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if parse_argument(&mut parsed, arg, &mut args)? {
            return Ok(None);
        }
    }
    parsed.finish().map(Some)
}

fn parse_argument<I>(
    parsed: &mut ParsedArgs,
    arg: OsString,
    args: &mut I,
) -> Result<bool, String>
where
    I: Iterator<Item = OsString>,
{
    match arg.to_str() {
        Some("--smoke") => {
            set_synthetic_mode(parsed, Mode::Smoke)?;
        }
        Some("--qualification") => {
            set_synthetic_mode(parsed, Mode::Qualification)?;
        }
        Some("--corpus-check" | "--corpus-decision-check") => {
            if parsed.corpus.is_some() {
                return Err("corpus mode may be specified only once".to_owned());
            }
            let decision_requested = arg.to_str() == Some("--corpus-decision-check");
            let path = next_value(args, "corpus mode requires a corpus path")?;
            parsed.corpus = Some((PathBuf::from(path), decision_requested));
        }
        Some("--population-pack") => {
            if parsed.population_pack.is_some() {
                return Err("--population-pack may be specified only once".to_owned());
            }
            parsed.population_pack = Some(PathBuf::from(next_value(
                args,
                "--population-pack requires a path",
            )?));
        }
        Some("--candidate") => {
            if parsed.population_candidate.is_some() {
                return Err("--candidate may be specified only once".to_owned());
            }
            let candidate = next_value(args, "--candidate requires a candidate name")?
                .into_string()
                .map_err(|_| "candidate name must be valid UTF-8".to_owned())?;
            parsed.population_candidate = Some(candidate);
        }
        Some("--population-smoke") => {
            set_population_mode(parsed, PopulationMode::Smoke)?;
        }
        Some("--population-qualification") => {
            set_population_mode(parsed, PopulationMode::Qualification)?;
        }
        Some("--output") => {
            if parsed.output.is_some() {
                return Err("--output may be specified only once".to_owned());
            }
            parsed.output = Some(PathBuf::from(next_value(
                args,
                "--output requires a path",
            )?));
        }
        Some("--help" | "-h") => return Ok(true),
        Some(other) => return Err(format!("unknown benchmark option: {other}")),
        None => return Err("benchmark arguments must be valid UTF-8".to_owned()),
    }
    Ok(false)
}

fn set_synthetic_mode(parsed: &mut ParsedArgs, mode: Mode) -> Result<(), String> {
    if parsed.synthetic_mode_explicit {
        return Err("synthetic benchmark mode may be specified only once".to_owned());
    }
    parsed.synthetic_mode = mode;
    parsed.synthetic_mode_explicit = true;
    Ok(())
}

fn set_population_mode(parsed: &mut ParsedArgs, mode: PopulationMode) -> Result<(), String> {
    if parsed.population_mode.is_some() {
        return Err("population benchmark mode may be specified only once".to_owned());
    }
    parsed.population_mode = Some(mode);
    Ok(())
}

fn next_value<I>(args: &mut I, error: &str) -> Result<OsString, String>
where
    I: Iterator<Item = OsString>,
{
    args.next().ok_or_else(|| error.to_owned())
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
