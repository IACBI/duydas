//! Runs the CPU renderer's scroll benchmark (ADR 0024) headless at 100% and 200% scale, with
//! damage tracking and with full redraws, and checks the shipping configuration (damage
//! tracking) against the ADR 0018 budget. Full redraws are printed for comparison only.

use std::path::Path;
use std::process::Command;

use crate::perf::build_release_target;

const MARKER: &str = "duydas-render-scroll ";

/// Arguments, and whether the run must meet the budget.
const CONFIGS: &[(&[&str], bool)] = &[
    (&["--scale", "1"], true),
    (&["--scale", "2"], true),
    (&["--scale", "1", "--full"], false),
    (&["--scale", "2", "--full"], false),
];

pub(crate) fn run(root: &Path) -> Result<(), String> {
    let exe = build_release_target(
        root,
        &["-p", "duydas-render", "--example", "scroll"],
        "scroll",
    )?;
    let mut failures = Vec::new();
    for &(args, gated) in CONFIGS {
        let output = Command::new(&exe)
            .args(args)
            .output()
            .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some(result) = stdout.lines().find_map(|l| l.strip_prefix(MARKER)) else {
            return Err(format!(
                "{}: no result line; output:\n{stdout}",
                args.join(" ")
            ));
        };
        println!("{result}");
        let field = |name: &str| {
            result
                .split_whitespace()
                .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
                .map(str::to_owned)
        };
        // The benchmark judges the budget on unrounded samples, stalls included.
        let passed = field("ok").as_deref() == Some("true")
            && (!gated || field("budget").as_deref() == Some("pass"));
        if !passed {
            failures.push(args.join(" "));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "over budget (p99 above 16.7 ms or a frame over 50 ms) or invalid: {}",
            failures.join(", ")
        ))
    }
}
