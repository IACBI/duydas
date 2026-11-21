//! Startup and memory measurement for the desktop client (see `docs/performance.md`).
//!
//! Measures wall time from spawn to the app's `first_frame` marker, then samples memory after the
//! app has idled. Memory metrics differ per OS and are reported under their native names.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const IDLE_BEFORE_SAMPLE: Duration = Duration::from_millis(2500);

pub(crate) fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let runs: usize = args
        .windows(2)
        .find(|w| w[0] == "--runs")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(5);

    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "duydas-desktop"])
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("release build failed".into());
    }
    let exe = root
        .join("target/release")
        .join(format!("duydas-desktop{}", std::env::consts::EXE_SUFFIX));

    let mut first_frames = Vec::new();
    let mut memory = Vec::new();
    for run in 1..=runs {
        let (first_frame, mem) = measure_once(&exe)?;
        println!("run {run}: spawn→first_frame {first_frame:.1} ms, {mem}");
        first_frames.push(first_frame);
        memory.push(mem);
    }
    first_frames.sort_by(f64::total_cmp);
    let p50 = first_frames[first_frames.len() / 2];
    let max = first_frames.last().copied().unwrap_or_default();
    println!("spawn→first_frame: p50 {p50:.1} ms, max {max:.1} ms over {runs} warm runs");
    println!("binary size: {:.1} MB", file_mb(&exe));
    Ok(())
}

fn measure_once(exe: &Path) -> Result<(f64, String), String> {
    let spawned = Instant::now();
    let mut child = Command::new(exe)
        .env("DUYDAS_PERF", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let mut first_frame = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.starts_with("duydas-perf first_frame_ms=") {
            first_frame = Some(spawned.elapsed().as_secs_f64() * 1000.0);
            break;
        }
    }
    std::thread::sleep(IDLE_BEFORE_SAMPLE);
    let mem = sample_memory(child.id());
    let _ = child.kill();
    let _ = child.wait();
    let first_frame = first_frame.ok_or("app exited before presenting a frame")?;
    Ok((first_frame, mem))
}

fn sample_memory(pid: u32) -> String {
    if cfg!(windows) {
        let script = format!(
            "$p = Get-Process -Id {pid}; '{{0:N1}} MB private, {{1:N1}} MB working set' -f ($p.PrivateMemorySize64/1MB), ($p.WorkingSet64/1MB)"
        );
        command_output("powershell", &["-NoProfile", "-Command", &script])
    } else if cfg!(target_os = "linux") {
        std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("Pss:"))
                    .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_owned())
            })
            .map_or_else(
                || "memory unavailable".into(),
                |kb| format!("{:.1} MB PSS", kb.parse::<f64>().unwrap_or(0.0) / 1024.0),
            )
    } else {
        let rss = command_output("ps", &["-o", "rss=", "-p", &pid.to_string()]);
        format!(
            "{:.1} MB RSS (use `footprint` for phys_footprint)",
            rss.trim().parse::<f64>().unwrap_or(0.0) / 1024.0
        )
    }
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|e| format!("memory unavailable: {e}"))
}

fn file_mb(path: &Path) -> f64 {
    std::fs::metadata(path).map_or(0.0, |m| m.len() as f64 / 1_048_576.0)
}
