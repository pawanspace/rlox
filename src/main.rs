//! # rlox — crate entry point
//!
//! This is a bytecode interpreter for the Lox language (from the book
//! "Crafting Interpreters"), written in Rust. Execution flows through a
//! pipeline of stages, each of which is a module declared below:
//!
//! ```text
//!   source text
//!      -> scanner   : text -> tokens
//!      -> compiler  : tokens -> bytecode (a `Chunk`)
//!      -> vm        : executes the bytecode
//! ```
//!
//! Supporting modules: `chunk` (the bytecode container), `common` (shared
//! types: `Value`, `Obj`, `OpCode`), `value` (constant pool), `hash_map` +
//! `hasher` (the interpreter's own hash table and string hashing), `memory`
//! (manual heap allocation for strings), `debug` (disassembly / tracing), and
//! `metrics` (timing).
//!
//! `run_file` reads a `.lox` file and asks a fresh `VM` to `interpret` it.
//! `repl`/`Repl` provide an interactive prompt. See `main` for how execution
//! is actually kicked off today.

use clap::Parser;
use std::{env, fs};

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::vm::InterpretResult;

// `mod NAME;` declarations pull each sibling file (e.g. `scanner.rs`) into the
// crate as a module. This is how Rust wires the codebase together — nothing in
// another file is visible until it's declared here (or under another module).
mod chunk;
// `#[macro_use]` makes `macro_rules!` macros defined in `common` visible to the
// modules declared *after* it in this file. Ordering matters for that reason.
#[macro_use]
mod common;
mod compiler;
mod debug;
mod hash_map;
mod hasher;
mod memory;
mod metrics;
mod scanner;
mod value;
mod vm;

/// Command-line argument definition, parsed by the `clap` crate.
///
/// Declares a single positional `path` argument (the `.lox` file to run) that
/// defaults to the empty string when omitted.
// NOTE: `Cli` is currently unused — `main` bypasses argument parsing entirely
// (see the note there). It's kept as scaffolding for when the CLI is re-enabled.
#[derive(Parser)]
struct Cli {
    // source file path
    #[clap(parse(from_os_str), default_value = "")]
    path: PathBuf,
}

/// Read a Lox source file from disk and execute it on a fresh VM.
///
/// Steps: open the file, read its entire contents into a `String`, then create
/// a `VM` and hand the source to `interpret` (which runs scanner -> compiler ->
/// VM internally).
fn run_file(path: PathBuf) {
    let mut file = fs::File::open(&path).expect("Unable to read file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Something went wrong while reading the file.");

    // NOTE: This is meant as a "did we read the whole file?" guard, but it's
    // unreliable: `contents.len()` counts UTF-8 *bytes* of the decoded string
    // while `metadata().len()` is the file's byte length on disk. For a valid
    // UTF-8 file these are equal, so the check normally never fires; and
    // `read_to_string` already errors on a failed/partial read above, making
    // this largely redundant. Exit code 74 mirrors clox's `EX_IOERR`.
    if contents.len() < file.metadata().unwrap().len().try_into().unwrap() {
        eprintln!("Could not read file: {:?}", path);
        std::process::exit(74);
    }
    let mut vm = vm::VM::init();
    let result = vm.interpret(contents.to_string());
    match  result.0 {
        InterpretResult::InterpretRuntimeError(err) => eprintln!("{}", err.message),
        _ => ()
    }
}

/// Interactive Read-Eval-Print Loop state.
///
/// Holds a mutable borrow of a `VM` (`&'a mut`) so that each prompted line is
/// interpreted against the *same* VM — meaning globals defined on one line
/// persist to the next. The `'a` lifetime ties this struct to the VM it
/// borrows: the `Repl` cannot outlive that VM.
struct Repl<'a> {
    vm: &'a mut vm::VM,
}

impl<'a> Repl<'a> {
    /// Wrap a borrowed VM in a `Repl`.
    fn init(vm: &'a mut vm::VM) -> Repl<'a> {
        Repl { vm }
    }

    /// Print the prompt, read one line from stdin, and interpret it.
    ///
    /// `stdout().flush()` is required because `print!` (unlike `println!`) does
    /// not append a newline, and stdout is line-buffered — without the flush
    /// the prompt could stay invisible until after the user has typed.
    fn prompt(&mut self, name: &str) {
        let mut line = String::new();
        print!("{}", name);
        std::io::stdout().flush().unwrap();
        std::io::stdin()
            .read_line(&mut line)
            .expect("Error: could not read input");
        self.vm.interpret(line.to_string());
    }
}

/// Run an interactive session: loop forever, interpreting each entered line.
// NOTE: currently unreachable — `main` never calls this (see below).
fn repl() {
    let mut vm = vm::VM::init();
    let mut repl = Repl::init(&mut vm);
    loop {
        repl.prompt("> ");
    }
}

/// Program entry point.
fn main() {
    // NOTE: Setting `RUST_BACKTRACE` from inside the program is a debugging
    // convenience; normally you'd set it in the environment before launching.
    env::set_var("RUST_BACKTRACE", "full");

    // NOTE: The real CLI is currently disabled. In normal operation the
    // commented-out code would parse args and either start the `repl()` (no
    // file given) or `run_file(args.path)`. Instead, execution is hardcoded to
    // run the fixed file "first.lox" — a development shortcut. The
    // `metrics::display()` summary at the end is likewise commented out.
    // let args = Cli::parse();
    // if args.path.as_os_str().is_empty() {
    //     repl();
    // } else {
    metrics::record("Total time".to_string(), || {
        run_file(PathBuf::from("first.lox"))
    });
    //       metrics::display();
    //}
}
