use std::fmt::{Debug, Formatter};
use num_derive::FromPrimitive;
use crate::common::Obj::Str;

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
    Obj(Box<Obj>),
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
}


impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if matches!(self, other) {
           return match self {
                Value::Boolean(_) =>  Into::<bool>::into(self.clone()) == Into::<bool>::into(other.clone()),
                Value::Number(_) => Into::<bool>::into(self.clone()) == Into::<bool>::into(other.clone()),
                Value::Missing => true,
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

impl From<Box<Obj>> for Value {
    fn from(value: Box<Obj>) -> Self {
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

impl Into<Box<Obj>> for Value {
    fn into(self) -> Box<Obj> {
        match self {
            Value::Obj(value) => value,
            //@todo @pawanc check if it should be false this can be wrong in most cases
            // may be we should throw error
            _ => Box::new(Obj::Nil)
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

