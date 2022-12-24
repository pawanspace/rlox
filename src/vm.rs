use crate::chunk::Chunk;
use crate::common::{FatPointer, Obj, OpCode, Value};
use crate::debug;
use crate::hash_map::Table;
use crate::hasher::hash;
use crate::scanner::Scanner;
use crate::{compiler, memory};
extern crate num;

const STACK_MAX: usize = 512;

#[derive(Debug)]
pub(crate) struct VM {
    chunk: Option<Box<Chunk>>,
    ip: i32,
    stack: Vec<Option<Value>>,
    stack_top: usize,
    table:  Table<Value>,
    globals: Table<Value>
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

macro_rules! READ_FAT_PTR {
    ($self:ident, $value:tt) => {{        
        let obj = Into::<Obj>::into($value);           
        Into::<FatPointer>::into(obj)        
    }};
}

macro_rules! BINARY_OP {
    ($self:ident, $op:tt, $t_type:ty) => {{
        let peek_0 = $self.peek(0);
        let peek_1 = $self.peek(1);
        if !peek_0.is_number() || !peek_1.is_number() {
            $self.runtime_error("Expected two numbers for binary operation.");
            return InterpretResult::InterpretRuntimeError;
        }

        let right_val = $self.pop();
        let left_val = $self.pop();
        $self.push(Value::from(Into::<$t_type>::into(left_val.clone()) $op Into::<$t_type>::into(right_val.clone())));
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
        let mut local_stack = Vec::with_capacity(STACK_MAX);

        for _i in 0..STACK_MAX {
            local_stack.push(None);
        }

        VM {
            chunk: None,
            ip: -1,
            stack: local_stack,
            stack_top: 0,
            table: Table::init(10),
            globals: Table::init(10)
        }
    }

    fn reset_stack(&mut self) {
        self.stack_top = 0;
    }

    fn push(&mut self, value: Value) {
        self.stack[self.stack_top] = Option::Some(value);
        self.stack_top += 1;
    }

    fn pop(&mut self) -> Value {
        self.stack_top -= 1;
        self.stack[self.stack_top].as_ref().unwrap().clone()
    }

    fn peek(&mut self, distance: usize) -> Value {
        self.stack[self.stack_top - 1 - distance]
            .as_ref()
            .unwrap()
            .clone()
    }

    fn runtime_error(&self, message: &str) {
        println!("Runtime error: {:?}", message);
    }

    fn run(&mut self) -> InterpretResult {
        loop {
            let instruction = READ_BYTE!(self);
            let opcode = num::FromPrimitive::from_u8(instruction);
            if debug::is_debug() && !matches!(opcode, None) {
                println!("##### Stack[Start] ###### \n");
                for i in 0..self.stack.len() {
                    print!("[{:?}] ", self.stack[i]);
                }
                println!("\n\n ##### Stack[End] ######");

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
                    if !value.is_number() {
                        self.runtime_error("Expected number for Negate opcode!");
                        return InterpretResult::InterpretRuntimeError;
                    }
                    let pop_val = self.pop();
                    self.push(Value::from(-1.0 * Into::<f64>::into(pop_val)));
                }
                Some(OpCode::Add) => {
                    let value = self.peek(0);
                    match value {
                        Value::Obj(obj) => {
                            if obj.is_string() {
                                if self.peek(1).is_obj_string() {
                                    let combined = self.concat();
                                    self.push(combined);
                                }
                            } else {
                                self.runtime_error("Expected String value on right side while adding to another string.");
                                return InterpretResult::InterpretRuntimeError;
                            }
                        }
                        Value::Number(_value) => BINARY_OP!(self, +, f64),
                        _ => {
                            self.runtime_error("Unknown type detected for Add operation");
                            return InterpretResult::InterpretOk;
                        }
                    }
                }
                Some(OpCode::Multiply) => {
                    BINARY_OP!(self, *, f64);
                }
                Some(OpCode::Subtract) => {
                    BINARY_OP!(self, -, f64);
                }
                Some(OpCode::Divide) => {
                    BINARY_OP!(self, /, f64);
                }
                Some(OpCode::Greater) => {
                    BINARY_OP!(self, >, bool);
                }
                Some(OpCode::Less) => {
                    BINARY_OP!(self, <, bool);
                }
                Some(OpCode::Equal) => {
                    let left = self.pop();
                    let right = self.pop();
                    self.push(Value::from(self.is_equal(left, right)));
                }
                Some(OpCode::Constant) => {
                    let constant = READ_CONSTANT!(self);
                    self.push((*constant.unwrap()).clone());
                }
                Some(OpCode::False) => {
                    self.push(Value::from(false));
                }
                Some(OpCode::True) => {
                    self.push(Value::from(true));
                }
                Some(OpCode::Nil) => {
                    self.push(Value::Missing);
                }
                Some(OpCode::Not) => {
                    let value = self.pop();
                    self.push(Value::from(self.is_falsey(value)));
                }
                Some(OpCode::ConstantLong) => {
                    let constant = READ_CONSTANT_LONG!(self);
                    self.push((*constant.unwrap()).clone());
                }
                Some(OpCode::DefineGlobalVariable) => {
                    let constant = READ_CONSTANT!(self).unwrap().clone();
                    let variable_name = READ_FAT_PTR!(self, constant);
                    let value = self.peek(0);
                    self.globals.insert(variable_name, value);
                    self.pop();
                }
                Some(OpCode::Pop) => {
                    self.pop();
                },
                Some(OpCode::GetGloablVariable) => {
                    let constant = READ_CONSTANT!(self).unwrap().clone();
                    let variable_name = READ_FAT_PTR!(self, constant);   
                    let size = variable_name.size;        
                    let ptr = variable_name.ptr;         
                    let value = self.get_variable_value(variable_name);

                    match value {
                        Some(val) => {
                            let obj = Into::<Obj>::into(val.clone());
                            let ptr = Into::<FatPointer>::into(obj);
                            let c_value = memory::read_string(ptr.ptr, ptr.size);
                            print!("Value pushing to stack {:?}", c_value);
                            self.push(val.clone())
                        },
                        None => {
                            let key = memory::read_string(ptr, size);
                            let message = format!("Unable to find value for key {:?}", key);
                            self.runtime_error(message.as_str());
                            return InterpretResult::InterpretRuntimeError
                        }
                    }                    
                }
                Some(OpCode::SetGlobalVariable) => {
                    let constant = READ_CONSTANT!(self).unwrap().clone();
                    let variable_name = READ_FAT_PTR!(self, constant);   
                    let size = variable_name.size;        
                    let ptr = variable_name.ptr;   
                    let value = self.peek(0);
                    
                    if !self.globals.insert(variable_name.clone(), value) {                        
                        self.globals.delete(variable_name.clone());
                        let key = memory::read_string(ptr, size);
                        let message = format!("Unable to find value for key {:?}", key);
                        self.runtime_error(message.as_str());
                        return InterpretResult::InterpretRuntimeError
                    }                    
                }
                Some(OpCode::Print) => {
                    debug::print_value(self.pop(),  true);
                }
                _ => {
                    return InterpretResult::InterpretOk;
                }
            }
        }
    }

    fn get_variable_value(&self, variable_name: FatPointer) -> Option<&Value> {
        self.globals.get(variable_name)
    }

    fn is_equal(&self, left: Value, right: Value) -> bool {
        left == right
    }

    // we are treating nil as false
    fn is_falsey(&self, value: Value) -> bool {
        value.is_missing() || (value.is_boolean() && !Into::<bool>::into(value))
    }

    fn concat(&mut self) -> Value {
        let second = self.pop();
        let first = self.pop();
        let obj1 = Into::<Obj>::into(first);
        let obj2 = Into::<Obj>::into(second);
        let ptr_1 = Into::<FatPointer>::into(obj1);
        let ptr_2 = Into::<FatPointer>::into(obj2);

        let ptr = memory::allocate::<String>();
        memory::copy(ptr_1.ptr, ptr, ptr_1.size, 0);
        memory::copy(ptr_2.ptr, ptr, ptr_2.size, ptr_1.size);

        let hash_value = hash(memory::read_string(ptr, ptr_1.size + ptr_2.size).as_str());
        Value::from(Obj::from(FatPointer {
            ptr,
            size: (ptr_1.size + ptr_2.size),
            hash: hash_value,
        }))
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
        let mut compiler = compiler::Compiler::init(scanner, chunk_on_heap, &mut self.table);
        let (had_error, chunk) = compiler.compile(source);
        if had_error {
            return InterpretResult::InterpretCompileError;
        }
        self.chunk = Some(chunk);
        self.ip = 0;
        self.run()
    }
}
