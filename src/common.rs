use num_derive::FromPrimitive;
#[derive(Debug)]
#[repr(u8)]
#[derive(FromPrimitive)]
pub enum OpCode {
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

#[derive(Debug, PartialEq)]
#[repr(u8)]
#[derive(FromPrimitive, Copy, Clone)]
pub enum ValueType {
    Bool,
    Nil,
    Number,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Data {
    pub boolean: bool,
    pub number: f64,
}

#[derive(Debug, Copy, Clone)]
pub struct Value {
    pub v_type: ValueType,
    pub data: Data,
}

#[macro_export]
macro_rules! BOOL_VAL {
    ($value:ident) => {{
        Value {
            v_type: ValueType::Bool,
            data: Data {
                boolean: $value,
                number: 0.0,
            },
        }
    }};
}

#[macro_export]
macro_rules! NIL_VAL {
    () => {{
        Value {
            v_type: ValueType::Nil,
            data: Data {
                number: 0.0,
                boolean: false,
            },
        }
    }};
}

#[macro_export]
macro_rules! NUMBER_VAL {
    ($value:ident) => {{
        Value {
            v_type: ValueType::Number,
            data: Data {
                number: $value,
                boolean: false,
            },
        }
    }};
}

#[macro_export]
macro_rules! AS_BOOL {
    ($value:ident) => {{
        $value.data.boolean
    }};
}

#[macro_export]
macro_rules! AS_NUMBER {
    ($value:ident) => {{
        $value.data.number
    }};
}

#[macro_export]
macro_rules! IS_BOOL {
    ($value:ident) => {{
        $value.v_type == ValueType::Bool
    }};
}

#[macro_export]
macro_rules! IS_NUMBER {
    ($value:ident) => {{
        $value.v_type == ValueType::Number
    }};
}

#[macro_export]
macro_rules! IS_NIL {
    ($value:ident) => {{
        $value.v_type == ValueType::Nil
    }};
}
