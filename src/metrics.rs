//! # Metrics — a tiny wall-clock timing helper
//!
//! This module measures how long chunks of work take (e.g. "Compiler time",
//! "VM run time", "Total time") and stashes the results in a global table so
//! they can be printed at the end. It's a debugging/profiling aid, not part of
//! the language itself.
//!
//! ## Core idea: timing with `Instant`
//! `std::time::Instant::now()` grabs a monotonic clock reading. Subtracting two
//! readings (via `.elapsed()`) yields a `Duration`. "Monotonic" means it only
//! moves forward and is immune to wall-clock adjustments (NTP, DST), which is
//! exactly what you want for measuring elapsed time.
//!
//! ## Why a global, and why that's a problem here
//! The results live in a single process-wide table so any code can record into
//! it without threading a handle through every function. This module reaches
//! for `static mut` to do that — see the BUG note below for why that's unsound
//! in Rust.

use crate::common::random_color;
use colored::Colorize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Process-wide table mapping an event name -> how long it took.
///
/// `Option<..>` so it can start as `None` and be lazily created on first use
/// (`static` initializers must be const-evaluable, and `HashMap::new()` is not
/// usable as a `static` initializer in the way we want here).
///
// BUG/UNSOUND: `static mut` is undefined behavior to access if there is ANY
// possibility of overlapping/aliasing access (Rust 2024 lints this as
// `static_mut_refs`). Every read/write below needs `unsafe` precisely because
// the compiler cannot guarantee exclusive access. The correct idiomatic fix is
// `std::sync::OnceLock<Mutex<HashMap<..>>>` (or `thread_local!`), which gives
// safe, initialized-once, synchronized access without `unsafe`.
static mut EVENTS: Option<HashMap<String, Duration>> = None;

/// Lazily initialize the global `EVENTS` map the first time it's needed.
///
/// `matches!(EVENTS, None)` checks whether the map has been created yet; if
/// not, we allocate an empty `HashMap`. Wrapped in `unsafe` because touching a
/// `static mut` is unsafe (see the note on `EVENTS`).
fn init_events() {
    unsafe {
        if matches!(EVENTS, None) {
            EVENTS = Some(HashMap::new());
        }
    }
}

/// Time a closure and record the result under `name`, returning the closure's
/// own return value untouched.
///
/// This is the workhorse: wrap any expression as
/// `metrics::record("label", || do_work())` and it (1) runs `do_work`,
/// (2) measures how long it took, (3) stores that under "label", and
/// (4) hands you back whatever `do_work` returned. The generic `R` is that
/// return type, so `record` is transparent to callers.
///
/// `impl FnMut() -> R` accepts any closure/function that can be called to
/// produce an `R`.
pub(crate) fn record<R>(name: String, mut func: impl FnMut() -> R) -> R {
    init_events();
    let start = Instant::now();
    // Run the actual work being measured.
    let result = func();
    // `elapsed()` = now - start, as a `Duration`.
    let total_time = start.elapsed();
    unsafe {
        EVENTS.as_mut().unwrap().insert(name, total_time);
    }
    result
}

/// Print every recorded timing, one per line, in a random color.
///
/// Iteration order is unspecified because `HashMap` has no ordering guarantee.
/// The leading blank lines just visually separate this summary from the noisy
/// interpreter output above it.
pub(crate) fn display() {
    println!("\n\n\n");
    unsafe {
        EVENTS.as_ref().unwrap().iter().for_each(|(key, value)| {
            println!(
                "{}",
                format!("***** {:?}: {:?} *****", key, value).color(random_color())
            );
        });
    }
}
