use crate::chunk::Chunk;
use crate::common::{Data, OpCode, Value, ValueType};
use crate::compiler;
use crate::debug;
use crate::scanner::Scanner;
extern crate num;

const STACK_MAX: usize = 512;
#[derive(Debug)]
pub(crate) struct VM {
    chunk: Option<Box<Chunk>>,
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
    ($self:ident, $op:tt, $valueType:tt) => {{
        let peek_0 = $self.peek(0);
        let peek_1 = $self.peek(1);
        if !IS_NUMBER!(peek_0) || !IS_NUMBER!(peek_1) {
            $self.runtime_error();
            return InterpretResult::InterpretRuntimeError;
        }

        let right_val = $self.pop();
        let left_val = $self.pop();
        $self.push($valueType!(AS_NUMBER!(right_val) $op AS_NUMBER!(right_val)));
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

impl VM {
    pub(crate) fn init() -> VM {
        VM {
            chunk: None,
            ip: -1,
            stack: [Value {
                v_type: ValueType::Nil,
                data: Data {
                    boolean: false,
                },
            }; STACK_MAX],
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

    fn peek(&mut self, distance: usize) -> Value {
        self.stack[self.stack_top - 1 - distance]
    }

    fn runtime_error(&self) {
        println!("ERROR!!! RunTIME!!!!");
    }

    fn run(&mut self) -> InterpretResult {
        loop {
            let instruction = READ_BYTE!(self);
            let opcode = num::FromPrimitive::from_u8(instruction);

            if debug::is_debug() {
                // println!("##### Stack ###### \n");
                // for i in 0..self.stack.len() {
                //     print!("[{:?}]", self.stack[i]);
                // }
                // println!("\n\n ##### Stack ######");

                self.chunk
                    .as_ref()
                    .unwrap()
                    .handle_instruction(&instruction, (self.ip - 1) as usize);
            }

            match opcode {
                Some(OpCode::Return) => {
                    return InterpretResult::InterpretOk;
                }
                Some(OpCode::Negate) => {
                    let value = self.peek(0);
                    let is_number = !IS_NUMBER!(value);
                    if !is_number {
                        self.runtime_error();
                        return InterpretResult::InterpretRuntimeError;
                    }
                    let pop_val = self.pop();
                    self.push(NUMBER_VAL!(-1.0 * AS_NUMBER!(pop_val)));
                }
                Some(OpCode::Add) => {
                    BINARY_OP!(self, +, NUMBER_VAL);
                }
                Some(OpCode::Multiply) => {
                    BINARY_OP!(self, *, NUMBER_VAL);
                }
                Some(OpCode::Subtract) => {
                    BINARY_OP!(self, -, NUMBER_VAL);
                }
                Some(OpCode::Divide) => {
                    BINARY_OP!(self, /, NUMBER_VAL);
                }
                Some(OpCode::Greater) => {
                    BINARY_OP!(self, > , BOOL_VAL);
                }
                Some(OpCode::Less) => {
                    BINARY_OP!(self, < , BOOL_VAL);
                }
                Some(OpCode::Equal) => {
                    let left = self.pop();
                    let right = self.pop();
                    self.push(BOOL_VAL!(self.is_equal(left, right)));
                }
                Some(OpCode::Constant) => {
                    let constant = READ_CONSTANT!(self);
                    self.push(*constant.unwrap());
                }
                Some(OpCode::False) => {
                    self.push(BOOL_VAL!(false));
                }
                Some(OpCode::True) => {
                    self.push(BOOL_VAL!(true));
                }
                Some(OpCode::Nil) => {
                    self.push(NIL_VAL!());
                }
                Some(OpCode::Not) => {
                    let value = self.pop();
                    self.push(BOOL_VAL!(self.is_falsey(value)));
                }
                Some(OpCode::ConstantLong) => {
                    let constant = READ_CONSTANT_LONG!(self);
                    self.push(*constant.unwrap());
                }
                _ => {
                    return InterpretResult::InterpretOk;
                }
            }
        }
    }

    fn is_equal(&self, left: Value, right: Value) -> bool {
        if left.v_type != right.v_type {
            return false;
        }
        unsafe {
            match left.v_type {
                ValueType::Bool =>  left.data.boolean == right.data.boolean,
                ValueType::Number =>  left.data.number == right.data.number,
                ValueType::Nil => true,
                _ => false,
            }
        }

    }

    // we are treating nil as false
    fn is_falsey(&self, value: Value) -> bool {
        IS_NIL!(value) || (IS_BOOL!(value) && !AS_BOOL!(value))
    }

    pub(crate) fn interpret_old(&mut self, chunk: Chunk) -> InterpretResult {
        self.chunk = Some(Box::new(chunk));
        self.ip = 0;
        self.run()
    }

    pub(crate) fn interpret<'m>(&mut self, source: String, chunk: Chunk) -> InterpretResult {
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
