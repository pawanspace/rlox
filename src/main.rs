#![feature(test)]
extern crate test;

use crate::common::OpCode;

mod chunk;
mod common;
mod debug;
mod value;

pub fn execute() {
    let mut empty_chunk = chunk::Chunk::init();
    for i in 1..=257 {
        if i > 256 {
            empty_chunk.write_constant(&2566.0, i);
        } else {
            empty_chunk.write_constant(&1.0, i);
        }
    }

    empty_chunk.write_chunk(OpCode::Return as u8, 258);
    empty_chunk.disassemble_chunk("debug");

    println!("{:?}", empty_chunk);
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
