use clap::Parser;
use std::{env, fs};

use std::io::{Read, Write};
use std::path::PathBuf;
mod chunk;
#[macro_use]
mod common;
mod compiler;
mod debug;
mod hash_map;
mod hasher;
mod memory;
mod scanner;
mod value;
mod vm;
mod metrics;
#[derive(Parser)]
struct Cli {
    // source file path
    #[clap(parse(from_os_str), default_value = "")]
    path: PathBuf,
}

fn run_file(path: PathBuf) {
    let mut file = fs::File::open(&path).expect("Unable to read file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Something went wrong while reading the file.");

    if contents.len() < file.metadata().unwrap().len().try_into().unwrap() {
        eprintln!("Could not read file: {:?}", path);
        std::process::exit(74);
    }
    let mut vm = vm::VM::init();    
    vm.interpret(contents.to_string());
}

struct Repl<'a> {
    vm: &'a mut vm::VM,
}

impl<'a> Repl<'a> {
    fn init(vm: &'a mut vm::VM) -> Repl<'a> {
        Repl { vm }
    }

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

fn repl() {
    let mut vm = vm::VM::init();
    let mut repl = Repl::init(&mut vm);
    loop {
        repl.prompt("> ");
    }
}

fn main() {    
    // env::set_var("RUST_BACKTRACE", "full");
    // let args = Cli::parse();
    // if args.path.as_os_str().is_empty() {
    //     repl();
    // } else {
        //run_file(args.path);
        
    metrics::record("Total time".to_string(), || run_file(PathBuf::from("first.lox")));    
    metrics::display();
    //}
}

