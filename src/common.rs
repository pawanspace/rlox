use num_derive::FromPrimitive;

#[derive(Debug)]
#[repr(u8)]
#[derive(FromPrimitive)]
pub enum OpCode {
    Return = 1,
    Constant = 2
}

pub type Value = f64;

