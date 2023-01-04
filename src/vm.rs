extern crate num;

use crate::common::{random_color, FatPointer, Function, Obj, OpCode, Value};
use crate::debug;
use crate::hash_map::Table;
use crate::hasher::hash;
use crate::metrics;
use crate::scanner::Scanner;
use crate::{compiler, memory};
use colored::{Color, Colorize};

const STACK_MAX: usize = 512;

#[derive(Debug)]
pub(crate) struct VM {
    ip: i32,
    stack: Vec<Option<Value>>,
    stack_top: usize,
    table: Table<Value>,
    globals: Table<Value>,
    call_frames: Vec<Option<CallFrame>>,
    frame_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    function: Function,
    ip: usize,
    cf_stack_top: usize,
    color: Color,
}

impl CallFrame {
    fn print_name(&self) {
        match self.function.name.clone() {
            Some(ptr) => {
                let cf_name = memory::read_string(ptr.ptr, ptr.size);
                println!(
                    "{}",
                    format!("****** CallFrame: {:?} ******", cf_name)
                        .color(self.color)
                        .bold()
                );
            }
            None => println!(
                "{}",
                format!("****** CallFrame: {:?} ******", "Main")
                    .color(self.color)
                    .bold()
            ),
        }
    }
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
        let index = READ_BYTE!($self, $frame) as usize;
        debug::info(format!("Reading constant from index: {:?}", index));
        $frame.function.chunk.constants.values.get(index)
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

        let mut call_frames: Vec<Option<CallFrame>> = Vec::new();
        call_frames.resize(512, None);

        VM {
            ip: -1,
            stack: local_stack,
            stack_top: 0,
            table: Table::init(10),
            globals: Table::init(10),
            call_frames,
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
        debug::info(format!("Runtime error: {:?}", message));
    }

    fn run(&mut self) -> InterpretResult {
        let mut current_frame = self.call_frames[self.frame_count - 1]
            .as_ref()
            .unwrap()
            .clone();
        loop {
            let instruction = READ_BYTE!(self, current_frame);
            let opcode = num::FromPrimitive::from_u8(instruction);
            self.print_debug_info(&mut current_frame, &instruction, &opcode);

            match opcode {
                Some(OpCode::Return) => {
                    let is_last_frame = self.return_op(&mut current_frame);
                    if is_last_frame {
                        return InterpretResult::InterpretOk;
                    }
                    current_frame = self.call_frames[self.frame_count - 1]
                        .as_ref()
                        .unwrap()
                        .clone();
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
                    debug::info(format!(
                        "DefineGlobalVariable: Read constant value: {:?}",
                        constant
                    ));
                    let variable_name = Into::<FatPointer>::into(constant);
                    let value = self.peek(0);
                    self.globals.insert(variable_name, value);
                    self.pop();
                }
                Some(OpCode::Pop) => {
                    self.pop();
                }
                Some(OpCode::Closure) => {
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let function_obj = Into::<Obj>::into(constant);
                    let closure = Obj::Closure(Box::new(function_obj));
                    self.push(Value::from(closure));
                }
                Some(OpCode::Call) => {
                    let arg_count = READ_BYTE!(self, current_frame);
                    let old_frame = current_frame.clone();
                    if !self.execute_function(self.peek(arg_count as usize), arg_count) {
                        return InterpretResult::InterpretRuntimeError;
                    }
                    current_frame = self.call_frames[self.frame_count - 1]
                        .as_ref()
                        .unwrap()
                        .clone();
                    self.call_frames[self.frame_count - 2] = Some(old_frame);
                }
                Some(OpCode::JumpIfFalse) => {
                    if self.is_falsey(self.peek(0)) {
                        //current_frame.ip += offset as usize;
                        current_frame = self.update_offset(current_frame, true);
                    } else {
                        current_frame.ip = current_frame.ip + 2;
                    }
                }
                Some(OpCode::Jump) => {
                    current_frame = self.update_offset(current_frame, true);
                }
                Some(OpCode::Loop) => {
                    current_frame = self.update_offset(current_frame, false);
                }
                Some(OpCode::GetLocalVariable) => {
                    let b = READ_BYTE!(self, current_frame);
                    let val = self.stack[current_frame.cf_stack_top + b as usize]
                        .clone()
                        .unwrap();
                    self.push(val.clone());
                }
                Some(OpCode::SetLocalVariable) => {
                    let b = READ_BYTE!(self, current_frame);
                    self.stack[current_frame.cf_stack_top + b as usize] = Some(self.peek(0));
                }
                Some(OpCode::GetGlobalVariable) => {
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    debug::info(format!(
                        "GetGlobalVariable: Read constant value: {:?}",
                        constant
                    ));
                    let variable_name = Into::<FatPointer>::into(constant);
                    if let Some(ret) = self.push_obj_value_to_stack(variable_name) {
                        return ret;
                    }
                }
                Some(OpCode::SetGlobalVariable) => {
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let variable_name = Into::<FatPointer>::into(constant);
                    if let Some(ret) = self.set_global_variable(variable_name) {
                        return ret;
                    }
                }
                Some(OpCode::Print) => {
                    debug::print_value(self.pop(), true);
                }
                _ => {
                    debug::info(format!("Stopping vm: {:?}", opcode));
                    self.call_frames[self.frame_count - 1] = Some(current_frame);
                    return InterpretResult::InterpretOk;
                }
            }
        }
    }

    fn set_global_variable(&mut self, variable_name: FatPointer) -> Option<InterpretResult> {
        let size = variable_name.size;
        let ptr = variable_name.ptr;
        let value = self.peek(0);

        if !self.globals.insert(variable_name.clone(), value) {
            self.globals.delete(variable_name.clone());
            let key = memory::read_string(ptr, size);
            let message = format!("Unable to find value for key {:?}", key);
            self.runtime_error(message.as_str());
            return Some(InterpretResult::InterpretRuntimeError);
        }

        None
    }

    fn push_obj_value_to_stack(&mut self, variable_name: FatPointer) -> Option<InterpretResult> {
        let size = variable_name.size;
        let ptr = variable_name.ptr;
        let value = self.get_variable_value(variable_name);

        match value {
            Some(val) => match value {
                Some(Value::Boolean(v)) => {
                    debug::info(format!("Boolean value pushing to stack {:?}", v));
                    self.push(val.clone());
                }
                Some(Value::Number(v)) => {
                    debug::info(format!("Number value pushing to stack {:?}", v));
                    self.push(val.clone());
                }
                Some(Value::Obj(obj)) => match obj {
                    Obj::Str(ptr) => {
                        let c_value = memory::read_string(ptr.ptr, ptr.size);
                        debug::info(format!(
                            "String Object value pushing to stack {:?}",
                            c_value
                        ));
                        self.push(val.clone());
                    }
                    Obj::Fun(function) => {
                        let function_name = function.name.as_ref().unwrap();
                        let name = memory::read_string(function_name.ptr, function_name.size);
                        debug::info(format!(
                            "Function Object value pushing to stack {:?} with name: {:?}",
                            function, name
                        ));
                        self.push(val.clone());
                    }
                    _ => {
                        debug::info(format!("Unknown object pushing to stack"));
                        self.push(val.clone());
                    }
                },
                _ => {
                    debug::info(format!("Unknown value pushing to stack"));
                    self.push(val.clone());
                }
            },
            None => {
                let key = memory::read_string(ptr, size);
                let message = format!("Unable to find value for key {:?}", key);
                self.runtime_error(message.as_str());
                return Some(InterpretResult::InterpretRuntimeError);
            }
        }
        None
    }

    fn print_debug_info(
        &mut self,
        current_frame: &mut CallFrame,
        instruction: &u8,
        opcode: &Option<OpCode>,
    ) {
        current_frame.print_name();
        if !matches!(opcode, None) {
            if debug::PRINT_STACK {
                debug::info(format!("##### Stack[Start] ###### \n"));
                for i in 0..self.stack.len() {
                    print!("[{:?}] ", self.stack[i]);
                }
                debug::info(format!("\n\n ##### Stack[End] ######"));
            }

            current_frame
                .function
                .chunk
                .handle_instruction(&instruction, (current_frame.ip - 1) as usize);
        }
    }

    fn return_op(&mut self, current_frame: &mut CallFrame) -> bool {
        let result = self.pop();
        self.frame_count -= 1;

        if self.frame_count == 0 {
            // @todo check if we need this pop.
            //self.pop();
            return true;
        }
        // + 1 for the first stack entry
        self.stack_top = current_frame.cf_stack_top;
        debug::info(format!("Pushing return value to stack: {:?}", result));
        self.push(result);
        false
    }

    fn execute_function(&mut self, callee: Value, arg_count: u8) -> bool {
        if callee.is_obj() {
            let obj = Into::<Obj>::into(callee);
            match obj {
                Obj::Fun(function) => {
                    if function.arity != arg_count {
                        self.runtime_error(
                            format!(
                                "Expected: {:?} arguments but received: {:?}",
                                function.arity, arg_count
                            )
                            .as_str(),
                        );
                    }
                    self.create_call_frame(function, arg_count);
                    return true;
                }
                Obj::Closure(obj) => {
                    let function = Into::<Function>::into(*obj);
                    if function.arity != arg_count {
                        self.runtime_error(
                            format!(
                                "Expected: {:?} arguments but received: {:?}",
                                function.arity, arg_count
                            )
                            .as_str(),
                        );
                    }
                    self.create_call_frame(function, arg_count);
                    return true;
                }
                _ => (),
            }
        }
        self.runtime_error("Can only execute function");
        false
    }

    fn create_call_frame(&mut self, function: Function, arg_count: u8) {
        let mut cf_stack_top = 0;
        if self.stack_top > 0 {
            /*
             * The funny little - 1 is to account for stack slot zero which the compiler
             * set aside for when we add methods later.
             * The parameters start at slot one so we make the window start
             * one slot earlier to align them with the arguments.
             * -1 is for name of the function
             */
            cf_stack_top = self.stack_top - (arg_count as usize) - (1 as usize);
        };

        let call_frame = CallFrame {
            function,
            ip: 0, //@todo check if this value should be 0 or not
            cf_stack_top,
            color: random_color(),
        };

        self.call_frames[self.frame_count] = Some(call_frame);
        self.frame_count += 1;
    }

    fn update_offset(&self, mut current_frame: CallFrame, add: bool) -> CallFrame {
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
        let second = Into::<FatPointer>::into(self.pop());
        let first = Into::<FatPointer>::into(self.pop());

        let ptr = memory::allocate::<String>();
        memory::copy(first.ptr, ptr, first.size, 0);
        memory::copy(second.ptr, ptr, second.size, first.size);

        let hash_value = hash(memory::read_string(ptr, first.size + second.size).as_str());
        Value::from(Obj::from(FatPointer {
            ptr,
            size: (first.size + second.size),
            hash: hash_value,
        }))
    }

    pub(crate) fn interpret<'m>(&mut self, source: String) -> InterpretResult {
        let chars: Vec<char> = source.chars().collect();
        let scanner = Scanner::init(0, 0, chars);

        let mut compiler = compiler::Compiler::init(scanner, &mut self.table);

        let (had_error, function_obj) = metrics::record("Compiler time".to_string(), || {
            compiler.compile(source.clone())
        });

        if had_error {
            return InterpretResult::InterpretCompileError;
        }
        self.ip = 0;

        self.push(Value::from(Obj::Closure(Box::new(function_obj.clone()))));
        let function = Into::<Function>::into(function_obj);
        debug::info(format!("Main function: {:?}", function.clone()));
        self.create_call_frame(function, 0);
        metrics::record("VM run time".to_string(), || self.run())
    }
}
