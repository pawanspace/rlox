use crate::common::{OpCode, Value};
use crate::debug;
use crate::value::{self, ValueArray};
extern crate num;
#[derive(Debug, Clone)]
pub(crate) struct Chunk {
    pub code: Vec<u8>,
    pub constants: value::ValueArray,
    pub lines: Vec<u32>,
}

impl<'a> Chunk {
    pub(crate) fn init() -> Chunk {
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

    pub(crate) fn add_constant(&mut self, value: Value) -> usize {
        self.constants.append(value);
        self.constants.count()
    }

    // version of write_chunk
    pub(crate) fn write_constant(&mut self, value: Value, line: u32) -> usize {
        let index = self.add_constant(value);
        // for any index constant that doesn't fit in u8, we store all bytes
        if index <= 255 {
            self.write_chunk(OpCode::Constant as u8, line);
        } else {
            self.write_chunk(OpCode::ConstantLong as u8, line);
        }
        self.write_index(index, line);
        index
    }

    pub(crate) fn write_index(&mut self, index: usize, line: u32) {
        if index <= 255 {
            self.write_chunk(index as u8, line);
        } else {
            let bytes = index.to_ne_bytes();
            for byte in bytes.iter() {
                self.write_chunk(*byte, line);
            }
        }
    }

    pub(crate) fn disassemble_chunk(&self, name: &str) {
        debug::info(format!("=== {} === ", name));
        let mut offset: usize = 0;
        while offset < self.code.len() {
            debug::info(format!("{:04}", offset));
            // if its on same line
            if offset > 0 && self.lines.get(offset) == self.lines.get(offset - 1) {
                debug::info(" | ".to_string());
            } else {
                debug::info(format!("Line: {}", self.lines.get(offset).unwrap()));
            }
            let instruction = self.code.get(offset).unwrap();
            offset = self.handle_instruction(instruction, offset);
        }
    }

    pub fn handle_instruction(&self, instruction: &u8, offset: usize) -> usize {
        let opcode = num::FromPrimitive::from_u8(*instruction);
        match opcode {
            Some(OpCode::Return)
            | Some(OpCode::Negate)
            | Some(OpCode::Add)
            | Some(OpCode::Subtract)
            | Some(OpCode::Multiply)
            | Some(OpCode::False)
            | Some(OpCode::True)
            | Some(OpCode::Nil)
            | Some(OpCode::Not)
            | Some(OpCode::Greater)
            | Some(OpCode::Less)
            | Some(OpCode::Equal)
            | Some(OpCode::Print)
            | Some(OpCode::DefineGlobalVariable)
            | Some(OpCode::GetGlobalVariable)
            | Some(OpCode::SetGlobalVariable)
            | Some(OpCode::GetLocalVariable)
            | Some(OpCode::SetLocalVariable)
            | Some(OpCode::Pop)
            | Some(OpCode::Call)
            | Some(OpCode::Closure)
            | Some(OpCode::Divide) => {
                debug::debug(format!("opcode: {:?}", opcode.unwrap()), true);
            }
            Some(OpCode::Jump) | Some(OpCode::JumpIfFalse) | Some(OpCode::Loop) => {
                self.jump_instruction(opcode.unwrap(), offset);
                return offset + 3; // 1 byte for opcode 2 for the jump offset
            }
            Some(OpCode::Constant) => {
                let constant_index = self.code.get(offset + 1).unwrap();
                self.print_debug_info(OpCode::Constant, *constant_index as usize);
                // return 1 byte of constant_index + 1 byte of opcode
                return offset + 2;
            }
            Some(OpCode::ConstantLong) => {
                let mut constant_index_bytes = [0, 0, 0, 0, 0, 0, 0, 0];
                // our long constant index is usize which is 8 bytes
                for i in 1..=8 {
                    constant_index_bytes[i - 1] = *self.code.get(i + offset).unwrap();
                }
                let constant_index = usize::from_ne_bytes(constant_index_bytes);
                self.print_debug_info(OpCode::ConstantLong, constant_index);
                // return 8 bytes of constant_index + 1 byte of opcode
                return offset + 9;
            }
            _ => {
                debug::info(format!("Unknown instruction: {:?}", opcode));
            }
        }
        offset + 1
    }

    fn jump_instruction(&self, instruction: OpCode, offset: usize) {
        debug::info(format!("opcode: {:?}", instruction));
        debug::info(format!("with jump: {:?}", self.get_offset(offset)));
    }

    fn get_offset(&self, offset: usize) -> u16 {
        let offset_bytes: [u8; 2] = [
            self.code[(offset + 2) as usize],
            self.code[(offset + 1) as usize],
        ];
        println!("offset bytes: {:?}", offset_bytes);
        // adding 2 because we read offset bytes
        u16::from_ne_bytes(offset_bytes)
    }

    fn print_debug_info(&self, opcode: OpCode, constant_index: usize) {
        debug::info(format!("opcode: {:?}", opcode));
        debug::info(format!("constant index: {}", constant_index));
        //TODO: I am not sure if converting u8 to size here is fine or not
        let value = self.constants.get(constant_index as usize);
        debug::print_value(&value, true);
    }
}
