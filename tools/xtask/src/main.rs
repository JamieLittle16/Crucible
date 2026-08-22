//! Repository automation entry point.

#![forbid(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crucible_section_qualification::{Candidate, QualificationMode};

const PINNED_RUST: &str = "1.97.1";

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(OsStr::to_str) {
        Some("guard") => guard(),
        Some("vanilla") => {
            let remaining_args = args.collect::<Vec<_>>();
            vanilla(&remaining_args)
        }
        Some("qualify") => {
            let remaining_args = args.collect::<Vec<_>>();
            qualify(&remaining_args)
        }
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
            failures.push(format!(
                "vanilla pin/audit consistency marker missing: {expected}"
            ));
        }
    }
}

fn vanilla(args: &[OsString]) -> ExitCode {
    let root = workspace_root();
    if args.first().and_then(|arg| arg.to_str()) == Some("state-data") {
        return vanilla_state_data(&root, &args[1..]);
    }

    let script = root.join("tools/vanilla_atlas.py");
    if !script.is_file() {
        return failure("tools/vanilla_atlas.py is missing");
    }
    run_python(&root, &script, args)
}

fn vanilla_state_data(root: &Path, args: &[OsString]) -> ExitCode {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return failure("usage: cargo xtask vanilla state-data <inspect|generate|verify|diff> ...");
    };

    match command {
        "inspect" if args.len() == 2 => {
            let script = root.join("tools/state_data.py");
            run_python(root, &script, &[OsString::from("inspect"), args[1].clone()])
        }
        "generate" if args.len() == 2 => {
            let script = root.join("tools/state_data.py");
            run_python(
                root,
                &script,
                &[
                    OsString::from("generate"),
                    args[1].clone(),
                    OsString::from("--output"),
                    OsString::from("crates/data/crucible-generated/src/lib.rs"),
                    OsString::from("--manifest"),
                    OsString::from("vanilla/state-data/26.2-state-data-manifest.json"),
                ],
            )
        }
        "verify" if args.len() == 1 => {
            let script = root.join("tools/finalize_state_data.py");
            run_python(root, &script, &[OsString::from("--verify")])
        }
        "diff" if args.len() == 3 => {
            let script = root.join("tools/state_data.py");
            run_python(
                root,
                &script,
                &[OsString::from("diff"), args[1].clone(), args[2].clone()],
            )
        }
        _ => failure(
            "usage: cargo xtask vanilla state-data inspect <qualified-input> | generate \
             <qualified-input> | verify | diff <old-manifest> <new-manifest>",
        ),
    }
}

fn qualify(args: &[OsString]) -> ExitCode {
    if args.first().and_then(|arg| arg.to_str()) != Some("section") {
        return failure(
            "usage: cargo xtask qualify section [--quick|--full] [--candidate <direct|adaptive|fast-local|packed-local>] | --vanilla <fixture> [--runtime-data <RAW.json>]",
        );
    }

    let mut mode = QualificationMode::Quick;
    let mut candidate = None;
    let mut vanilla_fixture = None;
    let mut runtime_data = None;
    let mut index = 1_usize;
    while index < args.len() {
        match args[index].to_str() {
            Some("--quick") => mode = QualificationMode::Quick,
            Some("--full") => mode = QualificationMode::Full,
            Some("--candidate") => {
                let Some(value) = args.get(index + 1).and_then(|arg| arg.to_str()) else {
                    return failure("--candidate requires a candidate name");
                };
                let Some(parsed) = Candidate::from_name(value) else {
                    return failure(
                        "unknown section candidate; expected direct, adaptive, fast-local, or packed-local",
                    );
                };
                candidate = Some(parsed);
                index += 1;
            }
            Some("--vanilla") => {
                let Some(value) = args.get(index + 1) else {
                    return failure("--vanilla requires a semantic fixture path");
                };
                vanilla_fixture = Some(value.clone());
                index += 1;
            }
            Some("--runtime-data") => {
                let Some(value) = args.get(index + 1) else {
                    return failure("--runtime-data requires an official runtime JSON path");
                };
                runtime_data = Some(value.clone());
                index += 1;
            }
            Some(other) => {
                return failure(&format!("unknown section qualification option: {other}"));
            }
            None => return failure("section qualification arguments must be valid UTF-8"),
        }
        index += 1;
    }

    if let Some(fixture) = vanilla_fixture {
        if candidate.is_some() || mode == QualificationMode::Full {
            return failure("--vanilla cannot be combined with --candidate or --full");
        }
        return qualify_section_fixture(&fixture, runtime_data.as_ref());
    }
    if runtime_data.is_some() {
        return failure("--runtime-data is only valid together with --vanilla");
    }

    let report = match crucible_section_qualification::qualify(mode, candidate) {
        Ok(report) => report,
        Err(error) => return failure(&format!("section qualification failed: {error}")),
    };

    let root = workspace_root();
    let commit_sha = match git_head_sha(&root) {
        Ok(sha) => sha,
        Err(error) => return failure(&error),
    };
    let output_dir = root.join("target/crucible-qualification/section");
    if let Err(error) = fs::create_dir_all(&output_dir) {
        return failure(&format!(
            "could not create qualification output directory: {error}"
        ));
    }
    let output_path = output_dir.join(format!("{}.json", mode.as_str()));
    if let Err(error) = fs::write(&output_path, report.to_json(&commit_sha)) {
        return failure(&format!(
            "could not write section qualification evidence: {error}"
        ));
    }

    for record in report.records() {
        println!(
            "section qualification: {} {} operations={} PASS",
            record.id(),
            record.candidate().as_str(),
            record.trace_operations()
        );
    }
    println!("section qualification evidence: {}", output_path.display());
    ExitCode::SUCCESS
}

fn qualify_section_fixture(fixture: &OsString, runtime_data: Option<&OsString>) -> ExitCode {
    let root = workspace_root();
    let commit_sha = match git_head_sha(&root) {
        Ok(sha) => sha,
        Err(error) => return failure(&error),
    };
    let output_dir = root.join("target/crucible-qualification/section");
    if let Err(error) = fs::create_dir_all(&output_dir) {
        return failure(&format!(
            "could not create qualification output directory: {error}"
        ));
    }

    let source_evidence = output_dir.join("vanilla-fixture.json");
    let status = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--locked",
            "-p",
            "crucible-section-qualification",
            "--bin",
            "section_fixture",
            "--",
            "--fixture",
        ])
        .arg(fixture)
        .arg("--output")
        .arg(&source_evidence)
        .arg("--commit")
        .arg(&commit_sha)
        .current_dir(&root)
        .status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => return exit_from_status(status.code()),
        Err(error) => return failure(&format!("could not launch section fixture qualifier: {error}")),
    }

    if let Some(runtime_data) = runtime_data {
        let script = root.join("tools/section_runtime_fixture.py");
        let runtime_evidence = output_dir.join("runtime-facts-fixture.json");
        let status = run_python(
            &root,
            &script,
            &[
                OsString::from("--runtime-data"),
                runtime_data.clone(),
                OsString::from("--fixture"),
                fixture.clone(),
                OsString::from("--output"),
                runtime_evidence.into_os_string(),
            ],
        );
        if status != ExitCode::SUCCESS {
            return status;
        }
    }

    ExitCode::SUCCESS
}

fn git_head_sha(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", root.to_string_lossy().as_ref(), "rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("could not execute git rev-parse: {error}"))?;
    if !output.status.success() {
        return Err(
            "git rev-parse HEAD failed; equivalence evidence requires a concrete commit".to_owned(),
        );
    }
    let sha = String::from_utf8(output.stdout)
        .map_err(|_| "git rev-parse HEAD returned non-UTF-8 output".to_owned())?;
    let sha = sha.trim();
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("git rev-parse HEAD did not return a 40-character hexadecimal SHA".to_owned());
    }
    Ok(sha.to_owned())
}

fn run_python(root: &Path, script: &Path, args: &[OsString]) -> ExitCode {
    if !script.is_file() {
        return failure(&format!("{} is missing", script.display()));
    }

    let Some(python) = find_python() else {
        return failure("Python 3 is required for Vanilla tooling");
    };

    match Command::new(python)
        .arg(script)
        .args(args)
        .current_dir(root)
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => exit_from_status(status.code()),
        Err(error) => failure(&format!("could not launch {}: {error}", script.display())),
    }
}

fn exit_from_status(code: Option<i32>) -> ExitCode {
    ExitCode::from(u8::try_from(code.unwrap_or(1).clamp(1, 255)).unwrap_or(1))
}

fn find_python() -> Option<OsString> {
    if let Some(explicit) = env::var_os("PYTHON")
        && python_works(&explicit)
    {
        return Some(explicit);
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
        .args([
            "-c",
            "import sys; raise SystemExit(sys.version_info < (3, 11))",
        ])
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
    println!("  guard                         run architecture/repository guards");
    println!("  vanilla <args..>              run Vanilla Atlas tooling");
    println!("  vanilla state-data inspect    inspect a qualified state-data input");
    println!("  vanilla state-data generate   generate committed Rust + manifest");
    println!("  vanilla state-data verify     replay the full pinned qualification chain");
    println!("  vanilla state-data diff       compare two generated manifests");
    println!("  qualify section --quick       run PR-sized section semantic qualification");
    println!("  qualify section --full        run extended multi-seed section qualification");
    println!("  qualify section --candidate   restrict qualification to one candidate");
    println!("  qualify section --vanilla     qualify a source-backed semantic fixture");
    println!("  qualify section --runtime-data additionally bind fixture to official runtime facts");
    println!();
    println!("examples:");
    println!("  cargo xtask vanilla verify-source /path/to/mc-src.zip");
    println!("  cargo xtask vanilla index /path/to/mc-src.zip");
    println!("  cargo xtask vanilla frontier m0-world-kernel");
    println!("  cargo xtask vanilla next m0-world-kernel");
    println!("  cargo xtask vanilla state-data verify");
    println!("  cargo xtask qualify section --quick");
    println!("  cargo xtask qualify section --full --candidate packed-local");
    println!("  cargo xtask qualify section --vanilla vanilla/fixtures/section/26.2-semantic-fixtures.txt");
}

fn failure(message: &str) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::FAILURE
}
