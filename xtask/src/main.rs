//! Developer tasks: `cargo xtask <command>`.
//!
//! - `i18n-check [--strict]`: validate every locale against en-US and lint UI code for hardcoded
//!   strings. `--strict` also fails on unused keys and missing variables.
//! - `perf [--runs N]`: launch the release desktop build and report startup and memory markers.

#![allow(
    clippy::print_stdout,
    reason = "xtask is a CLI; stdout is its interface"
)]

mod i18n;
mod perf;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("i18n-check") => i18n::check(&workspace_root(), args.iter().any(|a| a == "--strict")),
        Some("perf") => perf::run(&workspace_root(), &args[1..]),
        _ => {
            eprintln!("usage: cargo xtask <i18n-check [--strict] | perf [--runs N]>");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default()
}
