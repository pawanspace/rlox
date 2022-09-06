use crate::chunk::Chunk;
use crate::common::{OpCode, Value};
use crate::compiler;
use crate::debug;
use crate::scanner::Scanner;
extern crate num;

const STACK_MAX: usize = 512;

pub(crate) struct VM<'a> {
    chunk: Option<Box<Chunk<'a>>>,
    ip: i32,
    stack: [Value; STACK_MAX],
    stack_top: usize,
}

pub enum InterpretResult {
    InterpretOk,
    InterpretCompileError,
    InterpretRuntimeError,
}

macro_rules! READ_BYTE {
    ($self:ident) => {
        *{
            let c = $self.chunk.as_ref().unwrap().code.get($self.ip as usize);
            $self.ip += 1;
            c.unwrap()
        }
    };
}

macro_rules! READ_CONSTANT {
    ($self:ident) => {{
        $self
            .chunk
            .as_ref()
            .unwrap()
            .constants
            .values
            .get(READ_BYTE!($self) as usize)
    }};
}

macro_rules! BINARY_OP {
    ($self:ident, $op:tt) => {{
	let right = $self.pop();
	let left = $self.pop();
	$self.push(left $op right);
    }}
}

macro_rules! READ_CONSTANT_LONG {
    ($self:ident) => {{
        let mut constant_index_bytes = [0, 0, 0, 0, 0, 0, 0, 0];
        // our long constant index is usize which is 8 bytes
        for i in 1..=8 {
            constant_index_bytes[i - 1] = READ_BYTE!($self);
        }
        let constant_index = usize::from_ne_bytes(constant_index_bytes);
        $self
            .chunk
            .as_ref()
            .unwrap()
            .constants
            .values
            .get(constant_index as usize)
    }};
}

impl<'a> VM<'a> {
    pub(crate) fn init() -> VM<'a> {
        VM {
            chunk: None,
            ip: -1,
            stack: [0.0; STACK_MAX],
            stack_top: 0,
        }
    }

    fn reset_stack(&mut self) {
        self.stack_top = 0;
    }

    fn push(&mut self, value: Value) {
        self.stack[self.stack_top] = value;
        self.stack_top += 1;
    }

    fn pop(&mut self) -> Value {
        self.stack_top -= 1;
        self.stack[self.stack_top]
    }

    fn run(&mut self) -> InterpretResult {
        loop {
            let instruction = READ_BYTE!(self);
            let opcode = num::FromPrimitive::from_u8(instruction);

            if debug::is_debug() {
                println!("##### Stack ###### \n");
                for i in 0..self.stack.len() {
                    print!("[{:?}]", self.stack[i]);
                }
                println!("\n\n ##### Stack ######");

                self.chunk
                    .as_ref()
                    .unwrap()
                    .handle_instruction(&instruction, (self.ip - 1) as usize);
            }

            match opcode {
                Some(OpCode::Return) => {
                    println!("{:?}", self.pop());
                    return InterpretResult::InterpretOk;
                }
                Some(OpCode::Negate) => {
                    let local = self.pop();
                    self.push(-1.0 * local);
                }
                Some(OpCode::Add) => {
                    BINARY_OP!(self, +);
                }
                Some(OpCode::Multiply) => {
                    BINARY_OP!(self, *);
                }
                Some(OpCode::Subtract) => {
                    BINARY_OP!(self, -);
                }
                Some(OpCode::Divide) => {
                    BINARY_OP!(self, /);
                }
                Some(OpCode::Constant) => {
                    let constant = READ_CONSTANT!(self);
                    self.push(**constant.unwrap());
                }
                Some(OpCode::ConstantLong) => {
                    let constant = READ_CONSTANT_LONG!(self);
                    self.push(**constant.unwrap());
                }
                _ => {
                    return InterpretResult::InterpretOk;
                }
            }
        }
    }

    pub(crate) fn interpret_old(&mut self, chunk: Chunk<'a>) -> InterpretResult {
        self.chunk = Some(Box::new(chunk));
        self.ip = 0;
        self.run()
    }

    pub(crate) fn interpret<'m>(&mut self, source: String, chunk: Chunk<'a>) -> InterpretResult {
        let chunk_on_heap = Box::new(chunk);
        let chars: Vec<char> = source.chars().collect();
        let scanner = Scanner::init(0, 0, chars);
        let mut compiler = compiler::Compiler::init(scanner, chunk_on_heap);
        let (had_error, chunk) = compiler.compile(source);
        if had_error {
            return InterpretResult::InterpretCompileError;
        }
        self.chunk = Some(chunk);
        self.ip = 0;
        self.run()
    }
}
