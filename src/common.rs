use colored::Color;
use num_derive::FromPrimitive;
use rand::prelude::*;
use std::fmt::Debug;

use crate::{chunk::Chunk, hasher, memory};

#[derive(Debug)]
#[repr(u8)]
#[derive(FromPrimitive)]
pub(crate) enum OpCode {
    Return = 1,
    Constant = 2,
    ConstantLong = 3,
    Negate = 4,
    Add = 5,
    Subtract = 6,
    Multiply = 7,
    Divide = 8,
    Nil = 9,
    True = 10,
    False = 11,
    Not = 12,
    Equal = 13,
    Greater = 14,
    Less = 15,
    Print = 16,
    Pop = 17,
    DefineGlobalVariable = 18,
    SetGlobalVariable = 19,
    GetGlobalVariable = 20,
    SetLocalVariable = 21,
    GetLocalVariable = 22,
    JumpIfFalse = 23,
    Jump = 24,
    Loop = 25,
    Call = 26,
    Closure = 27,
    SetUpValue = 28,
    GetUpValue = 29,
}

#[derive(Debug, Clone)]
pub(crate) enum Value {
    Boolean(bool),
    Number(f64),
    Obj(Obj),
    Missing,
}

impl Value {
    #[inline]
    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Boolean(_))
    }

    #[inline]
    pub fn is_missing(&self) -> bool {
        matches!(self, Value::Missing)
    }

    #[inline]
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }

    #[inline]
    pub fn is_obj(&self) -> bool {
        matches!(self, Value::Obj(_))
    }

    #[inline]
    pub fn is_obj_string(&self) -> bool {
        return match self {
            Value::Obj(obj) => unsafe { obj.is_string() },
            _ => false,
        };
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if matches!(self, _other) {
            return match (self, other) {
                (Value::Boolean(l), Value::Boolean(r)) => l == r,
                (Value::Number(l), Value::Number(r)) => l == r,
                (Value::Missing, Value::Missing) => true,
                (Value::Obj(l), Value::Obj(r)) => l == r,
                _ => false,
            };
        }
        false
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Boolean(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(value)
    }
}

impl From<Obj> for Value {
    fn from(value: Obj) -> Self {
        Value::Obj(value)
    }
}

impl Into<bool> for Value {
    fn into(self) -> bool {
        match self {
            Value::Boolean(value) => value,
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => false,
        }
    }
}

impl Into<f64> for Value {
    fn into(self) -> f64 {
        match self {
            Value::Number(value) => value,
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => 0.0,
        }
    }
}

impl Into<Obj> for Value {
    fn into(self) -> Obj {
        match self {
            Value::Obj(value) => value,
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => panic!("Unexpected error"),
        }
    }
}

impl Into<FatPointer> for Value {
    fn into(self) -> FatPointer {
        match self {
            Value::Obj(obj) => Into::<FatPointer>::into(obj),
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => panic!("Unexpected error"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FatPointer {
    pub(crate) ptr: *mut u8,
    pub(crate) size: usize,
    pub(crate) hash: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Function {
    pub(crate) arity: u8,
    pub(crate) chunk: Chunk,
    pub(crate) name: Option<FatPointer>,
    pub(crate) func_type: FunctionType,
}

impl Function {
    pub(crate) fn new_function(fun_type: FunctionType) -> Function {
        Function {
            arity: 0,
            chunk: Chunk::init(),
            name: None,
            func_type: fun_type,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum FunctionType {
    Function,
    Script,
    Closure,
}

#[derive(Debug, Clone)]
pub(crate) enum Obj {
    Str(FatPointer),
    Fun(Function),
    Closure(Box<Obj>),
    Nil,
}

impl Obj {
    #[inline]
    pub fn is_string(&self) -> bool {
        matches!(self, Obj::Str(_))
    }

    #[inline]
    pub fn is_nil(&self) -> bool {
        matches!(self, Obj::Nil)
    }

    pub fn get_func_chunk(&mut self) -> &mut Chunk {
        match self {
            Obj::Fun(function) => &mut function.chunk,
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => panic!("Not able to convert to function from object"),
        }
    }
}

impl PartialEq for Obj {
    fn eq(&self, other: &Self) -> bool {
        if matches!(self, _other) {
            return match (self, other) {
                (Obj::Str(l), Obj::Str(r)) => l == r,
                _ => false,
            };
        }
        false
    }
}

impl From<FatPointer> for Obj {
    fn from(ptr: FatPointer) -> Self {
        Obj::Str(ptr)
    }
}

impl From<&mut str> for Obj {
    fn from(str_value: &mut str) -> Self {
        let hash_value = hasher::hash(str_value);
        let str_ptr = memory::allocate::<String>();
        memory::copy(str_value.as_mut_ptr(), str_ptr, str_value.len(), 0);
        let fat_ptr = FatPointer {
            ptr: str_ptr,
            size: str_value.len(),
            hash: hash_value,
        };
        Obj::from(fat_ptr.clone())
    }
}

impl Into<FatPointer> for Obj {
    fn into(self) -> FatPointer {
        match self {
            Obj::Str(ptr) => ptr,
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => FatPointer {
                ptr: "".to_string().as_mut_ptr(),
                size: 0 as usize,
                hash: 0,
            },
        }
    }
}

impl Into<Function> for Obj {
    fn into(self) -> Function {
        match self {
            Obj::Fun(function) => function,
            //@todo @pending check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => panic!("Not able to convert to function from object"),
        }
    }
}

pub(crate) fn random_color() -> Color {
    let r: u8 = rand::thread_rng().gen_range(1..=255);
    let g: u8 = rand::thread_rng().gen_range(1..=255);
    let b: u8 = rand::thread_rng().gen_range(1..=255);
    Color::TrueColor { r, g, b }
}
