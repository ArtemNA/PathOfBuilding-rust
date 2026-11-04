//! Start-up marks for `tools/sg-oracle/measure-startup.ps1`: with `POB_STARTUP_TRACE` set, [`mark`] prints the time since the first mark of the
//! process to stderr; without it, nothing happens. The memory of a stage comes from the script's samples of the process.

use std::sync::OnceLock;
use std::time::Instant;

fn start() -> Option<&'static Instant> {
    static START: OnceLock<Option<Instant>> = OnceLock::new();
    START
        .get_or_init(|| std::env::var_os("POB_STARTUP_TRACE").map(|_| Instant::now()))
        .as_ref()
}

/// One line `mark <ms> <label>` when tracing.
pub fn mark(label: &str) {
    if let Some(t) = start() {
        eprintln!("mark {:.1} {label}", t.elapsed().as_secs_f64() * 1e3);
    }
}
