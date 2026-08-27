//! Cold-path executable for source-backed section semantic fixtures.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

#[path = "../fixtures.rs"]
pub mod fixtures;

const SOURCE_ARCHIVE_SHA256: &str =
    "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750";
const SOURCE_QUALIFICATION_SHA256: &str =
    "5d312d6025fa6556feaf5fa26c80577dcb024e7e5be5cd1bda98d101367600c8";
const RUNTIME_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";

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
    let mut fixture = None;
    let mut output = None;
    let mut commit_sha = None;
    let mut args = env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--fixture") => {
                fixture = Some(
                    args.next()
                        .ok_or_else(|| "--fixture requires a path".to_owned())?,
                );
            }
            Some("--output") => {
                output = Some(
                    args.next()
                        .ok_or_else(|| "--output requires a path".to_owned())?,
                );
            }
            Some("--commit") => {
                commit_sha = Some(
                    args.next()
                        .and_then(|value| value.into_string().ok())
                        .ok_or_else(|| "--commit requires a UTF-8 SHA".to_owned())?,
                );
            }
            Some(other) => return Err(format!("unknown section fixture option: {other}")),
            None => return Err("section fixture arguments must be valid UTF-8".to_owned()),
        }
    }

    let fixture = PathBuf::from(fixture.ok_or_else(|| "--fixture is required".to_owned())?);
    let output = PathBuf::from(output.ok_or_else(|| "--output is required".to_owned())?);
    let commit_sha = commit_sha.ok_or_else(|| "--commit is required".to_owned())?;
    if commit_sha.len() != 40 || !commit_sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("--commit must be a 40-character hexadecimal Git SHA".to_owned());
    }

    let fixture_text = fs::read_to_string(&fixture)
        .map_err(|error| format!("could not read fixture {}: {error}", fixture.display()))?;
    let evidence = fixtures::qualify_fixture(&fixture_text)
        .map_err(|error| format!("section semantic fixture failed: {error}"))?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "could not create fixture evidence directory {}: {error}",
                parent.display()
            )
        })?;
    }
    fs::write(&output, render_json(evidence, &commit_sha)).map_err(|error| {
        format!(
            "could not write section fixture evidence {}: {error}",
            output.display()
        )
    })?;

    println!(
        "section vanilla fixture: cases={} block-candidate-checks={} biome-checks={} fingerprint={:016x} PASS",
        evidence.cases(),
        evidence.block_candidate_checks(),
        evidence.biome_checks(),
        evidence.fingerprint()
    );
    println!("section vanilla fixture evidence: {}", output.display());
    Ok(())
}

fn render_json(evidence: fixtures::FixtureEvidence, commit_sha: &str) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"schema\": 1,\n",
            "  \"id\": \"EQUIV-WORLD-SECTION-VANILLA-FIXTURE\",\n",
            "  \"qualification\": \"source-backed-semantic-fixture\",\n",
            "  \"minecraft_version\": \"26.2\",\n",
            "  \"protocol_version\": 776,\n",
            "  \"data_version\": 4903,\n",
            "  \"commit_sha\": \"{}\",\n",
            "  \"source_archive_sha256\": \"{}\",\n",
            "  \"source_qualification_sha256\": \"{}\",\n",
            "  \"runtime_server_sha256\": \"{}\",\n",
            "  \"state_data_generation_sha256\": \"{}\",\n",
            "  \"sem_ids\": [\"SEM-WORLD-SECTION-003\", \"SEM-WORLD-SECTION-004\", \"SEM-WORLD-SECTION-005\", \"SEM-WORLD-SECTION-006\", \"SEM-WORLD-SECTION-007\", \"SEM-WORLD-SECTION-008\", \"SEM-WORLD-SECTION-009\", \"SEM-WORLD-SECTION-010\", \"SEM-WORLD-SECTION-012\", \"SEM-WORLD-SECTION-015\", \"SEM-WORLD-SECTION-016\"],\n",
            "  \"fixture_cases\": {},\n",
            "  \"block_candidate_checks\": {},\n",
            "  \"biome_checks\": {},\n",
            "  \"fixture_fingerprint_fnv1a64\": \"{:016x}\"\n",
            "}}\n"
        ),
        commit_sha,
        SOURCE_ARCHIVE_SHA256,
        SOURCE_QUALIFICATION_SHA256,
        RUNTIME_SERVER_SHA256,
        crucible_generated::STATE_DATA_GENERATION_SHA256,
        evidence.cases(),
        evidence.block_candidate_checks(),
        evidence.biome_checks(),
        evidence.fingerprint()
    )
}
