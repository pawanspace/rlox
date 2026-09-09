//! # Debug / trace output helpers
//!
//! Small logging layer used throughout the VM and compiler. There is no logging
//! crate here; instead three compile-time `bool` flags act as switches. Because
//! they are `static` constants, the compiler can often eliminate the disabled
//! branches entirely, so leaving trace calls in the code costs little when the
//! flags are off.
//!
//! This mirrors clox's `DEBUG_TRACE_EXECUTION` / `DEBUG_PRINT_CODE` compile-time
//! `#define` switches — the same idea of "instrumentation you toggle at build
//! time rather than run time".

use crate::common::{Obj, Value};
use crate::memory;

/// Master switch for `info` messages (general tracing).
static INFO: bool = true;
/// Master switch for `debug` messages (disassembly / value dumps).
static DEBUG: bool = true;
/// When true, `print_debug_info` in the VM dumps the whole value stack each
/// instruction. Very noisy; off by default.
pub(crate) static PRINT_STACK: bool = false;

/// Print a `[DEBUG]` line (or, when `new_line` is false, print without a
/// trailing newline so callers can build up a line piece by piece). No-op
/// unless the `DEBUG` flag is set.
pub fn debug(message: String, new_line: bool) {
    if DEBUG && new_line {
        println!("[DEBUG] {}", message)
    } else if DEBUG && !new_line {
        print!("{}", message)
    }
}

/// Print an `[INFO]` line. No-op unless the `INFO` flag is set.
pub fn info(message: String) {
    if INFO {
        println!("[INFO] {:?}", message);
    }
}

/// Pretty-print a runtime `Value` for tracing / the `print` statement.
///
/// Strings are special-cased: a `Value::Obj(Obj::Str)` holds a `FatPointer`
/// into manually-managed memory, so we must `read_string` the raw bytes back
/// out rather than relying on a `Debug` impl. All other values fall back to
/// their `{:?}` formatting.
pub(crate) fn print_value(value: &Value, new_line: bool) {
    match value {
        Value::Obj(obj) => match obj {
            Obj::Str(fat_ptr) => unsafe {
                let str = memory::read_string(fat_ptr.ptr, fat_ptr.size);
                debug(format!("constant value: {:?}", str), new_line);
            },
            _ => debug(format!("constant value: {:?}", obj), new_line),
        },
        _ => debug(format!("constant value: {:?}", value), new_line),
    }
}
