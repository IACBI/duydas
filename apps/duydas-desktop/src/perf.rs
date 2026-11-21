//! Startup markers for `cargo xtask perf`. Enabled with `DUYDAS_PERF=1`; otherwise inert.

use std::time::Instant;

/// Records process start and reports the first redraw request once. Iced emits it just before
/// presenting, so it marks the first frame attempt, not a confirmed present.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Probe {
    started: Instant,
    reported: bool,
}

impl Probe {
    pub(crate) fn from_env() -> Option<Self> {
        std::env::var_os("DUYDAS_PERF").map(|_| Self {
            started: Instant::now(),
            reported: false,
        })
    }

    pub(crate) fn pending(&self) -> bool {
        !self.reported
    }

    pub(crate) fn first_redraw(&mut self, at: Instant) {
        if self.reported {
            return;
        }
        self.reported = true;
        let ms = at.saturating_duration_since(self.started).as_secs_f64() * 1000.0;
        #[allow(
            clippy::print_stdout,
            reason = "machine-readable marker parsed by xtask perf"
        )]
        {
            println!("duydas-perf first_redraw_ms={ms:.1}");
        }
    }
}
