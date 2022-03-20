use crate::common::OpCode;

mod chunk;
mod common;
mod value;

fn main() {
    let mut empty_chunk = chunk::Chunk::init();
    let constant_index = empty_chunk.add_constant(&1.0);

    empty_chunk.write_chunk(OpCode::Constant as u8, 1);
    empty_chunk.write_chunk(constant_index as u8, 1);

    
    empty_chunk.write_chunk(OpCode::Return as u8, 2
    );


    println!("{:?}", empty_chunk);
    empty_chunk.disassemble_chunk("debug");
}
