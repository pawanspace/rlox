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
}

pub type Value = f64;
