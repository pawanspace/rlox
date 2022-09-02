#![feature(test)]
extern crate test;

use crate::common::OpCode;

mod chunk;
mod common;
mod debug;
mod value;
mod vm;

pub fn execute() {
    let mut empty_chunk = chunk::Chunk::init();
    for i in 1..=257 {
        if i > 256 {
            empty_chunk.write_constant(&2566.0, i);
        } else {
            empty_chunk.write_constant(&1.0, i);
        }
    }
    let mut vm = vm::VM::init();
    empty_chunk.write_chunk(OpCode::Negate as u8, 258);
    empty_chunk.write_constant(&1.2, 259);
    empty_chunk.write_constant(&3.4, 259);
    empty_chunk.write_chunk(OpCode::Add as u8, 259);
    empty_chunk.write_constant(&5.6, 260);
    empty_chunk.write_chunk(OpCode::Divide as u8, 261);
    empty_chunk.write_chunk(OpCode::Return as u8, 262);
    //    vm.interpret(&empty_chunk);
    empty_chunk.disassemble_chunk("debug");

    // println!("{:?}", empty_chunk);
}

fn main() {
    execute();
}

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;
    #[bench]
    fn bench_create_chunks(b: &mut Bencher) {
        b.iter(execute);
    }
}
