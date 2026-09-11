//! # `Chunk`: the unit of compiled code
//!
//! A *chunk* is a self-contained block of compiled bytecode plus everything the
//! VM needs to run and debug it. Each `Function` owns one chunk. It bundles
//! three parallel pieces:
//!   - `code`      — the flat byte array of instructions and their operands.
//!   - `constants` — the constant pool (see `value.rs`) that instruction
//!                   operands index into.
//!   - `lines`     — the source line each byte came from, kept only for error
//!                   messages and disassembly (it is not used to run the code).
//!
//! This is clox's `Chunk` structure. Keeping line numbers in a separate array
//! parallel to `code` means the runtime data path stays lean while we can still
//! report "error on line N" when something goes wrong.
use crate::common::{OpCode, Value};
use crate::debug;
use crate::value::{self, ValueArray};
extern crate num;

/// A compiled block of bytecode with its constants and source-line map.
#[derive(Debug, Clone)]
pub(crate) struct Chunk {
    /// The instruction stream: opcodes interleaved with their operand bytes.
    pub code: Vec<u8>,
    /// Literal values referenced by `Constant`/`ConstantLong` operands.
    pub constants: value::ValueArray,
    /// `lines[i]` is the source line number that produced `code[i]`. Parallel
    /// to `code`, used for diagnostics only.
    pub lines: Vec<u32>,
}

impl<'a> Chunk {
    /// Create an empty chunk.
    pub(crate) fn init() -> Chunk {
        Chunk {
            code: vec![],
            constants: ValueArray::init(),
            lines: vec![],
        }
    }

    /// Append one raw byte (an opcode or an operand) to the code stream, and
    /// record the source line it came from in the parallel `lines` array.
    pub(crate) fn write_chunk(&mut self, byte: u8, line: u32) {
        self.code.push(byte);
        self.lines.push(line);
    }

    /// Add a value to the constant pool and return its index.
    pub(crate) fn add_constant(&mut self, value: Value) -> usize {
        self.constants.append(value);
        self.constants.last_index()
    }

    /// Emit a "load this constant" instruction: store the value in the pool,
    /// then write the appropriate opcode followed by the index operand.
    // This is the operand-carrying sibling of `write_chunk`.
    pub(crate) fn write_constant(&mut self, value: Value, line: u32) -> usize {
        let index = self.add_constant(value);
        // Pick the opcode by how big the index is: if it fits in a single byte
        // use the compact `Constant`; otherwise use `ConstantLong`, which
        // carries a full 8-byte index. This keeps the common case (few
        // constants) small while still supporting very large pools.
        if index <= 255 {
            self.write_chunk(OpCode::Constant as u8, line);
        } else {
            self.write_chunk(OpCode::ConstantLong as u8, line);
        }
        self.write_index(index, line);
        index
    }

    /// Write an index operand into the code stream, matching the variable-width
    /// encoding that `write_constant` chose.
    pub(crate) fn write_index(&mut self, index: usize, line: u32) {
        if index <= 255 {
            // Small index: a single byte.
            self.write_chunk(index as u8, line);
        } else {
            // Large index: the full `usize` as raw bytes. `to_ne_bytes` uses
            // *native* endianness, so the reader must decode the same way (see
            // the VM's `READ_CONSTANT_LONG`). Native endianness is fine here
            // because these bytes are never shared across machines.
            let bytes = index.to_ne_bytes();
            for byte in bytes.iter() {
                self.write_chunk(*byte, line);
            }
        }
    }

    /// Human-readable dump of the whole chunk (a *disassembler*), used for
    /// debugging the compiler's output.
    ///
    /// It walks the code array instruction by instruction. Note the loop does
    /// not do `offset += 1`: instructions have different sizes (some carry
    /// operands), so `handle_instruction` returns the offset of the *next*
    /// instruction, and we continue from there.
    pub(crate) fn disassemble_chunk(&self, name: &str) {
        debug::info(format!("=== {} === ", name));
        let mut offset: usize = 0;
        while offset < self.code.len() {
            debug::info(format!("{:04}", offset));
            // Collapse the line column: if this byte shares a source line with
            // the previous one, print "|" instead of repeating the line number.
            if offset > 0 && self.lines.get(offset) == self.lines.get(offset - 1) {
                debug::info(" | ".to_string());
            } else {
                debug::info(format!("Line: {}", self.lines.get(offset).unwrap()));
            }
            let instruction = self.code.get(offset).unwrap();
            offset = self.handle_instruction(instruction, offset);
        }
    }

    /// Decode and print a single instruction, returning the offset of the next
    /// instruction.
    ///
    /// The return value encodes each opcode's total size: simple opcodes
    /// advance by 1, `Constant` by 2 (opcode + 1-byte index), `ConstantLong`
    /// by 9 (opcode + 8-byte index), and jumps by 3 (opcode + 2-byte offset).
    /// The VM's real decode loop relies on the same size rules.
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
                return offset + 3; // 1 byte opcode + 2-byte jump offset
            }
            Some(OpCode::Constant) => {
                // The 1-byte operand sits right after the opcode.
                let constant_index = self.code.get(offset + 1).unwrap();
                self.print_debug_info(OpCode::Constant, *constant_index as usize);
                return offset + 2; // opcode + 1-byte index
            }
            Some(OpCode::ConstantLong) => {
                // Reassemble the 8-byte index that follows the opcode. It was
                // written with native endianness (see `write_index`), so we
                // decode with `from_ne_bytes` to match.
                let mut constant_index_bytes = [0, 0, 0, 0, 0, 0, 0, 0];
                for i in 1..=8 {
                    constant_index_bytes[i - 1] = *self.code.get(i + offset).unwrap();
                }
                let constant_index = usize::from_ne_bytes(constant_index_bytes);
                self.print_debug_info(OpCode::ConstantLong, constant_index);
                return offset + 9; // opcode + 8-byte index
            }
            _ => {
                debug::info(format!("Unknown instruction: {:?}", opcode));
            }
        }
        offset + 1
    }

    /// Print a jump instruction along with its decoded 2-byte offset.
    fn jump_instruction(&self, instruction: OpCode, offset: usize) {
        debug::info(format!("opcode: {:?}", instruction));
        debug::info(format!("with jump: {:?}", self.get_offset(offset)));
    }

    /// Decode the 2-byte jump offset that follows a jump opcode at `offset`.
    fn get_offset(&self, offset: usize) -> u16 {
        // The two operand bytes are read in reverse order (high byte at
        // offset+2, low byte at offset+1) to match how the compiler wrote them;
        // the VM's `update_offset` decodes with the same byte ordering.
        let offset_bytes: [u8; 2] = [
            self.code[(offset + 2) as usize],
            self.code[(offset + 1) as usize],
        ];
        // NOTE: stray debug `println!` — prints regardless of the debug flags.
        println!("offset bytes: {:?}", offset_bytes);
        u16::from_ne_bytes(offset_bytes)
    }

    /// Print an opcode together with the constant its operand points at.
    fn print_debug_info(&self, opcode: OpCode, constant_index: usize) {
        debug::info(format!("opcode: {:?}", opcode));
        debug::info(format!("constant index: {}", constant_index));
        let value = self.constants.get(constant_index as usize);
        debug::print_value(&value, true);
    }
}
