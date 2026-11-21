//! Startup and memory measurement for the desktop client (see `docs/performance.md`).
//!
//! Measures wall time from spawn to the app's `first_redraw` marker, then samples memory after the
//! app has idled. Memory metrics differ per OS and are reported under their native names.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const IDLE_BEFORE_SAMPLE: Duration = Duration::from_millis(2500);
const MARKER_TIMEOUT: Duration = Duration::from_secs(30);
const MARKER: &str = "duydas-perf first_redraw_ms=";

/// Kills the child on every exit path, including early returns and panics.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let runs = match args.iter().position(|a| a == "--runs") {
        Some(i) => args
            .get(i + 1)
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .ok_or("--runs needs a positive integer")?,
        None => 5,
    };

    let exe = build_release(root)?;
    let mut redraws = Vec::with_capacity(runs);
    for run in 1..=runs {
        let (first_redraw, mem) = measure_once(&exe)?;
        println!("run {run}: spawn→first_redraw {first_redraw:.1} ms, {mem}");
        redraws.push(first_redraw);
    }
    redraws.sort_by(f64::total_cmp);
    let p50 = redraws[redraws.len() / 2];
    let max = redraws[redraws.len() - 1];
    println!("spawn→first_redraw: p50 {p50:.1} ms, max {max:.1} ms over {runs} runs");
    println!("binary size: {:.1} MB", file_mb(&exe));
    Ok(())
}

/// Builds the desktop app and returns the executable path Cargo reports, which honors
/// `CARGO_TARGET_DIR`, `build.target-dir` and target-triple configuration.
fn build_release(root: &Path) -> Result<PathBuf, String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "duydas-desktop",
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("release build failed".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("\"reason\":\"compiler-artifact\""))
        .filter(|line| line.contains("\"name\":\"duydas-desktop\""))
        .find_map(|line| json_string_field(line, "executable"))
        .map(PathBuf::from)
        .ok_or_else(|| "cargo did not report the duydas-desktop executable".into())
}

/// Extracts a string field from one line of Cargo's JSON output. Cargo escapes only `\\` and
/// `"` in paths, which is all this needs to undo.
fn json_string_field(line: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\":\"");
    let start = line.find(&key)? + key.len();
    let mut out = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => out.push(chars.next()?),
            other => out.push(other),
        }
    }
    None
}

fn measure_once(exe: &Path) -> Result<(f64, String), String> {
    let spawned = Instant::now();
    let mut guard = Guard(
        Command::new(exe)
            .env("DUYDAS_PERF", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", exe.display()))?,
    );
    let stdout = guard.0.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.starts_with(MARKER) {
                let _ = tx.send(spawned.elapsed());
                break;
            }
        }
    });
    let Ok(elapsed) = rx.recv_timeout(MARKER_TIMEOUT) else {
        let _ = guard.0.kill();
        let mut stderr = String::new();
        if let Some(mut pipe) = guard.0.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        return Err(format!(
            "no first_redraw marker within {MARKER_TIMEOUT:?}; app stderr:\n{stderr}"
        ));
    };
    std::thread::sleep(IDLE_BEFORE_SAMPLE);
    let mem = sample_memory(guard.0.id());
    Ok((elapsed.as_secs_f64() * 1000.0, mem))
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
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|kb| kb.parse::<f64>().ok())
            })
            .map_or_else(
                || "memory unavailable".into(),
                |kb| format!("{:.1} MB PSS", kb / 1024.0),
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

#[cfg(test)]
mod tests {
    use super::json_string_field;

    #[test]
    fn extracts_escaped_executable_path() {
        let line = r#"{"reason":"compiler-artifact","executable":"C:\\t\\duydas-desktop.exe","fresh":true}"#;
        assert_eq!(
            json_string_field(line, "executable").as_deref(),
            Some(r"C:\t\duydas-desktop.exe")
        );
        assert_eq!(
            json_string_field(r#"{"executable":null}"#, "executable"),
            None
        );
    }
}
