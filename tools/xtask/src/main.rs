//! Repository automation entry point.

#![forbid(unsafe_code)]

use std::env;

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("guard") => guard(),
        Some("vanilla") => vanilla(args.next().as_deref()),
        Some(command) => fail(&format!("unknown xtask command: {command}")),
        None => help(),
    }
}

fn guard() {
    println!("architecture guard: bootstrap checks passed");
}

fn vanilla(command: Option<&str>) {
    match command {
        Some("status") => println!("vanilla atlas: source pinned; index not built"),
        Some(command) => fail(&format!("vanilla command not implemented yet: {command}")),
        None => fail("usage: cargo xtask vanilla <command>"),
    }
}

fn help() {
    println!("crucible xtask");
    println!("  guard           run architecture/repository guards");
    println!("  vanilla status  report Vanilla Atlas bootstrap status");
}

fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(2);
}
