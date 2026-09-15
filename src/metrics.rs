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
//! ## Why a global, and how it stays safe
//! The results live in a single process-wide table so any code can record into
//! it without threading a handle through every function. A naive global would
//! use `static mut`, but that is unsound in Rust (any overlapping/aliased access
//! is undefined behavior, and it offers no thread synchronization). Instead the
//! table is a `static OnceLock<Mutex<..>>`: an *immutable* static whose contents
//! are mutated through interior mutability. `OnceLock` guarantees it is
//! initialized exactly once; `Mutex` guarantees exclusive access at runtime — so
//! no `unsafe` is needed and there is no data race.

use crate::common::random_color;
use colored::Colorize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Process-wide table mapping an event name -> how long it took.
///
/// Declared as a plain (immutable) `static`, not `static mut`. `OnceLock::new()`
/// is a `const fn`, so it can initialize a static directly, and the map inside
/// is created lazily on first use via `get_or_init`. Mutation happens through
/// the `Mutex` (interior mutability), so accessing this needs no `unsafe`:
/// `OnceLock` makes init race-free and `Mutex` makes each access exclusive.
static EVENTS: OnceLock<Mutex<HashMap<String, Duration>>> = OnceLock::new();

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
    let start = Instant::now();
    // Run the actual work being measured.
    let result = func();
    // `elapsed()` = now - start, as a `Duration`.
    let total_time = start.elapsed();
    EVENTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(name, total_time);
    result
}

/// Print every recorded timing, one per line, in a random color.
///
/// Iteration order is unspecified because `HashMap` has no ordering guarantee.
/// The leading blank lines just visually separate this summary from the noisy
/// interpreter output above it.
pub(crate) fn display() {
    println!("\n\n\n");

    EVENTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .iter()
        .for_each(|(key, value)| {
            println!(
                "{}",
                format!("***** {:?}: {:?} *****", key, value).color(random_color())
            );
        });
}
