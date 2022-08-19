use clap::Parser;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
mod debug;

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

    println!("File contents: {:?}", contents);
    if contents.len() < file.metadata().unwrap().len().try_into().unwrap() {
        eprintln!("Could not read file: {:?}", path);
        std::process::exit(74);
    }
}

fn prompt(name: &str) -> String {
    let mut line = String::new();
    print!("{}", name);
    std::io::stdout().flush().unwrap();
    std::io::stdin()
        .read_line(&mut line)
        .expect("Error: could not read input");

    return line.trim().to_string();
}

fn repl() {
    loop {
        let input = prompt("> ");
    }
}

fn main() {
    let args = Cli::parse();
    if args.path.as_os_str().is_empty() {
        repl();
    } else {
        run_file(args.path);
    }
}
