use crate::common::{OpCode, Value};
use crate::value::{self, ValueArray};
extern crate num;

#[derive(Debug)]
pub(crate) struct Chunk<'a> {
    code: Vec<u8>,
    constants: value::ValueArray<'a>,
    lines: Vec<u32>,
}

impl<'a> Chunk<'a> {
    pub(crate) fn init() -> Chunk<'a> {
        Chunk {
            code: vec![],
            constants: ValueArray::init(),
            lines: vec![],
        }
    }

    pub(crate) fn write_chunk(&mut self, byte: u8, line: u32) {
        self.code.push(byte);
        self.lines.push(line);
    }

    pub(crate) fn add_constant(&mut self, value: &'a Value) -> usize {
        self.constants.append(value);
        self.constants.count()
    }

    pub(crate) fn disassemble_chunk(&self, name: &str) {
        println!("=== {} === ", name);
        let mut offset: usize = 0;
        while offset < self.code.len() {
            println!("{:04}", offset);
            // if its on same line
            if offset > 0 && self.lines.get(offset) == self.lines.get(offset - 1) {
                println!(" | ");
            } else {
                println!("Line: {}", self.lines.get(offset).unwrap());
            }
            let instruction = self.code.get(offset).unwrap();
            offset = self.handle_instruction(instruction, offset);
        }
    }

    fn handle_instruction(&self, instruction: &u8, offset: usize) -> usize {
        let opcode = num::FromPrimitive::from_u8(*instruction);
        match opcode {
            Some(OpCode::Return) => {
                println!("opcode: {:?}", OpCode::Return);
            }
            Some(OpCode::Constant) => {
                println!("opcode: {:?}", OpCode::Constant);
                let constant = self.code.get(offset + 1).unwrap();
                println!("constant index: {}", constant);
                //TODO: I am not sure if converting u8 to size here is fine or not
                println!(
                    "constant value: {:?}",
                    self.constants.get(*constant as usize)
                );
                return offset + 2;
            }
            _ => {
                println!("Unknown instruction");
            }
        }
        offset + 1
    }
}
