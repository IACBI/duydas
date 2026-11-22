//! Runs the virtual-list spike (ADR 0018) on each renderer at 100% and 200% scale.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::perf::{Guard, build_release, drain_stderr, join_tail};

const MARKER: &str = "duydas-vlist ";
const TIMEOUT: Duration = Duration::from_secs(120);

struct Config {
    label: &'static str,
    backend: &'static str,
    /// `ICED_PRESENT_MODE`; `no_vsync` turns vsync off where the surface allows it (falling back to vsync), so intervals show frame cost.
    present_mode: &'static str,
    scale: &'static str,
}

/// wgpu always runs on the low-power adapter (ADR 0017). tiny-skia presents without vsync, so
/// its intervals already show cost.
const CONFIGS: &[Config] = &[
    Config {
        label: "wgpu low-power, vsync",
        backend: "wgpu",
        present_mode: "vsync",
        scale: "1",
    },
    Config {
        label: "wgpu low-power, vsync",
        backend: "wgpu",
        present_mode: "vsync",
        scale: "2",
    },
    Config {
        label: "wgpu low-power, no vsync",
        backend: "wgpu",
        present_mode: "no_vsync",
        scale: "1",
    },
    Config {
        label: "wgpu low-power, no vsync",
        backend: "wgpu",
        present_mode: "no_vsync",
        scale: "2",
    },
    Config {
        label: "tiny-skia",
        backend: "tiny-skia",
        present_mode: "vsync",
        scale: "1",
    },
    Config {
        label: "tiny-skia",
        backend: "tiny-skia",
        present_mode: "vsync",
        scale: "2",
    },
];

pub(crate) fn run(root: &Path) -> Result<(), String> {
    let exe = build_release(root, "duydas-spike-vlist")?;
    let mut invalid = Vec::new();
    for config in CONFIGS {
        let name = format!("{} @{}x", config.label, config.scale);
        let mut guard = Guard(
            Command::new(&exe)
                .env("ICED_BACKEND", config.backend)
                .env("ICED_PRESENT_MODE", config.present_mode)
                .env("WGPU_POWER_PREF", "low")
                .env("DUYDAS_SCALE", config.scale)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("spawn {}: {e}", exe.display()))?,
        );
        let stdout = guard.0.stdout.take().ok_or("no stdout")?;
        let stderr = drain_stderr(&mut guard.0);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(result) = line.strip_prefix(MARKER) {
                    let _ = tx.send(result.to_owned());
                    break;
                }
            }
        });
        let Ok(result) = rx.recv_timeout(TIMEOUT) else {
            let _ = guard.0.kill();
            return Err(format!(
                "{name}: no result within {TIMEOUT:?}; stderr:\n{}",
                join_tail(stderr)
            ));
        };
        if !result.starts_with("ok=true") {
            invalid.push(name.clone());
        }
        println!("{name}: {result}");
    }
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "invalid runs (missing samples, resizes or prepends, or a prepend moved the view): {}",
            invalid.join(", ")
        ))
    }
}
