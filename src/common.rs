use std::fmt::{Debug};
use num_derive::FromPrimitive;
use Obj::Nil;


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
}

#[derive(Debug, Clone)]
pub(crate) enum Value {
    Boolean(bool),
    Number(f64),
    Obj(*mut Obj),
    Missing
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
            (Value::Obj(ptr))  => {
                unsafe {
                    (**ptr).is_string()
                }
            },
            _ => false
        }
    }
}


impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if matches!(self, _other) {
           return match (self, other) {
               (Value::Boolean(l), Value::Boolean(r)) =>  l == r,
               (Value::Number(l), Value::Number(r))  => l == r,
               (Value::Missing, Value::Missing)  => true,
                _ => false,
            }
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

impl From<*mut Obj> for Value {
    fn from(value: *mut Obj) -> Self {
        Value::Obj(value)
    }
}

impl Into<bool> for Value {
    fn into(self) -> bool {
        match self {
            Value::Boolean(value) => value,
            //@todo @pawanc check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => false
        }
    }
}

impl Into<f64> for Value {
    fn into(self) -> f64 {
        match self {
            Value::Number(value) => value,
            //@todo @pawanc check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => 0.0
        }
    }
}

impl Into<*mut Obj> for Value {
    fn into(self) -> *mut Obj {
        match self {
            Value::Obj(value) => value,
            //@todo @pawanc check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => panic!("Unexpected error")
        }
    }
}


#[derive(Debug, Clone)]
pub(crate) enum Obj {
    Str(String),
    Nil
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
}


impl From<String> for Obj {
    fn from(value: String) -> Self {
        Obj::Str(value)
    }
}

impl Into<String> for Obj {
    fn into(self) -> String {
        match self {
            Obj::Str(value) => value,
            //@todo @pawanc check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => String::new()
        }
    }
}

