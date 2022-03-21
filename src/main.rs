#![feature(test)]
extern crate test;
use crate::common::OpCode;

mod chunk;
mod common;
mod value;

pub fn execute() {
    let mut empty_chunk = chunk::Chunk::init();
    let constant_index = empty_chunk.add_constant(&1.0);

    empty_chunk.write_chunk(OpCode::Constant as u8, 1);
    empty_chunk.write_chunk(constant_index as u8, 1);

    empty_chunk.write_chunk(OpCode::Return as u8, 2);

    println!("{:?}", empty_chunk);
    empty_chunk.disassemble_chunk("debug");
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
        b.iter(|| execute());
    }
}
