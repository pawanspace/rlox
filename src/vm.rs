use crate::chunk::Chunk;
use crate::common::{FatPointer, Function, Obj, OpCode, Value};
use crate::debug;
use crate::hash_map::Table;
use crate::hasher::hash;
use crate::metrics;
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
    table: Table<Value>,
    globals: Table<Value>,
    call_frames: Vec<CallFrame>,
    frame_count: usize,
}
#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    function: Function,
    ip: usize,
    cf_stack_top: usize,
}

pub enum InterpretResult {
    InterpretOk,
    InterpretCompileError,
    InterpretRuntimeError,
}

macro_rules! READ_BYTE {
    ($self:ident, $frame:ident) => {
        *{
            let c = $frame.function.chunk.code.get($frame.ip as usize).clone();
            $frame.ip += 1;
            c.unwrap()
        }
    };
}

macro_rules! READ_CONSTANT {
    ($self:ident, $frame:ident) => {{
        $frame
            .function
            .chunk
            .constants
            .values
            .get(READ_BYTE!($self, $frame) as usize)
    }};
}

macro_rules! READ_FAT_PTR {
    ($self:ident, $value:tt) => {{
        let obj = Into::<Obj>::into($value);
        Into::<FatPointer>::into(obj)
    }};
}

macro_rules! BINARY_OP {
    ($self:ident, $op:tt) => {{
        let peek_0 = $self.peek(0);
        let peek_1 = $self.peek(1);
        if !peek_0.is_number() || !peek_1.is_number() {
            $self.runtime_error("Expected two numbers for binary operation.");
            return InterpretResult::InterpretRuntimeError;
        }
        let right_val = $self.pop();
        let left_val = $self.pop();
        $self.push(Value::from(Into::<f64>::into(left_val.clone()) $op Into::<f64>::into(right_val.clone())));
    }}
}

macro_rules! READ_CONSTANT_LONG {
    ($self:ident, $frame:ident) => {{
        let mut constant_index_bytes = [0, 0, 0, 0, 0, 0, 0, 0];
        // our long constant index is usize which is 8 bytes
        for i in 1..=8 {
            constant_index_bytes[i - 1] = READ_BYTE!($self, $frame);
        }
        let constant_index = usize::from_ne_bytes(constant_index_bytes);
        $frame
            .function
            .chunk
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
            globals: Table::init(10),
            call_frames: Vec::new(),
            frame_count: 0,
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

    fn peek(&self, distance: usize) -> Value {
        self.stack[self.stack_top - 1 - distance]
            .as_ref()
            .unwrap()
            .clone()
    }

    fn runtime_error(&self, message: &str) {
        println!("Runtime error: {:?}", message);
    }

    fn run(&mut self) -> InterpretResult {
        let mut current_frame =  self.call_frames[self.frame_count - 1].clone();
        loop {
            let instruction = READ_BYTE!(self, current_frame);
            let opcode = num::FromPrimitive::from_u8(instruction);
            if debug::is_debug() && !matches!(opcode, None) {
                if debug::print_stack() {
                    println!("##### Stack[Start] ###### \n");
                    for i in 0..self.stack.len() {
                        print!("[{:?}] ", self.stack[i]);
                    }
                    println!("\n\n ##### Stack[End] ######");
                }

                current_frame
                    .function
                    .chunk
                    .handle_instruction(&instruction, (current_frame.ip - 1) as usize);
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
                        Value::Number(_value) => BINARY_OP!(self, +),
                        _ => {
                            self.runtime_error("Unknown type detected for Add operation");
                            return InterpretResult::InterpretOk;
                        }
                    }
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
                Some(OpCode::Greater) => {
                    BINARY_OP!(self, >);
                }
                Some(OpCode::Less) => {
                    BINARY_OP!(self, <);
                }
                Some(OpCode::Equal) => {
                    let left = self.pop();
                    let right = self.pop();
                    self.push(Value::from(self.is_equal(left, right)));
                }
                Some(OpCode::Constant) => {
                    let constant = READ_CONSTANT!(self, current_frame);                    
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
                    let constant = READ_CONSTANT_LONG!(self, current_frame);
                    self.push((*constant.unwrap()).clone());
                }
                Some(OpCode::DefineGlobalVariable) => {
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let variable_name = READ_FAT_PTR!(self, constant);
                    let value = self.peek(0);
                    self.globals.insert(variable_name, value);
                    self.pop();
                }
                Some(OpCode::Pop) => {
                    self.pop();
                }
                Some(OpCode::JumpIfFalse) => {                    
                    if self.is_falsey(self.peek(0)) {
                        //current_frame.ip += offset as usize;
                        current_frame = self.update_offset(current_frame, true);
                    } else {
                        current_frame.ip = current_frame.ip + 2;
                    }
                },
                Some(OpCode::Jump) => {                    
                    current_frame = self.update_offset(current_frame, true);
                },
                Some(OpCode::Loop) => {
                    current_frame = self.update_offset(current_frame, false);
                },
                Some(OpCode::GetLocalVariable) => {
                    let b = READ_BYTE!(self, current_frame);
                    let val = self.stack[current_frame.cf_stack_top + b as usize].clone().unwrap();
                    self.push(val.clone());
                }
                Some(OpCode::SetLocalVariable) => {
                    let b = READ_BYTE!(self, current_frame);
                    self.stack[current_frame.cf_stack_top + b as usize] = Some(self.peek(0));                    
                }
                Some(OpCode::GetGlobalVariable) => {
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let variable_name = READ_FAT_PTR!(self, constant);
                    let size = variable_name.size;
                    let ptr = variable_name.ptr;
                    let value = self.get_variable_value(variable_name);

                    match value {
                        Some(val) => match value {
                            Some(Value::Boolean(v)) => {
                                println!("Boolean value pushing to stack {:?}", v);
                                self.push(val.clone());
                            }
                            Some(Value::Number(v)) => {
                                println!("Number value pushing to stack {:?}", v);
                                self.push(val.clone());
                            }
                            Some(Value::Obj(obj)) => {
                                let ptr = Into::<FatPointer>::into(obj.clone());
                                let c_value = memory::read_string(ptr.ptr, ptr.size);
                                println!("Object value pushing to stack {:?}", c_value);
                                self.push(val.clone());
                            }
                            _ => {
                                println!("Unknown value pushing to stack");
                                self.push(val.clone());
                            }
                        },
                        None => {
                            let key = memory::read_string(ptr, size);
                            let message = format!("Unable to find value for key {:?}", key);
                            self.runtime_error(message.as_str());
                            return InterpretResult::InterpretRuntimeError;
                        }
                    }
                }
                Some(OpCode::SetGlobalVariable) => {
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let variable_name = READ_FAT_PTR!(self, constant);
                    let size = variable_name.size;
                    let ptr = variable_name.ptr;
                    let value = self.peek(0);

                    if !self.globals.insert(variable_name.clone(), value) {
                        self.globals.delete(variable_name.clone());
                        let key = memory::read_string(ptr, size);
                        let message = format!("Unable to find value for key {:?}", key);
                        self.runtime_error(message.as_str());
                        return InterpretResult::InterpretRuntimeError;
                    }
                }
                Some(OpCode::Print) => {
                    debug::print_value(self.pop(), true);
                }
                _ => {
                    println!("Stopping vm: {:?}", opcode);
                    self.call_frames[self.frame_count - 1] = current_frame;
                    return InterpretResult::InterpretOk;
                }
            }
        }        
    }


    fn update_offset(&self, mut current_frame: CallFrame, add: bool) -> CallFrame{ 
        let offset_bytes: [u8; 2] = [
            current_frame.function.chunk.code[(current_frame.ip + 1) as usize],
            current_frame.function.chunk.code[(current_frame.ip) as usize],
        ];
        current_frame.ip = current_frame.ip + 2;
        // adding 2 because we read offset bytes
        let offset = u16::from_ne_bytes(offset_bytes);        
        if add {
            current_frame.ip += offset as usize;
        } else {
            current_frame.ip -= offset as usize;
        }                

        current_frame
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

        let (had_error, function_obj) = metrics::record("Compiler time".to_string(), || {
            compiler.compile(source.clone())
        });

        if had_error {
            return InterpretResult::InterpretCompileError;
        }
        self.ip = 0;
        self.push(Value::from(function_obj.clone()));
        let function = Into::<Function>::into(function_obj);
        self.call_frames.push(CallFrame {
            function,
            ip: 0, //@todo check if this value should be 0 or not
            cf_stack_top: 0,
        });
        self.frame_count += 1;
        metrics::record("VM run time".to_string(), || self.run())
    }
}
