use crate::common::{Obj, Value};
use crate::memory;

static INFO: bool = true;
static DEBUG: bool = true;
pub(crate) static PRINT_STACK: bool = false;

pub fn debug(message: String, new_line: bool) {
    if DEBUG && new_line {
        println!("[DEBUG] {}", message)
    } else if DEBUG && !new_line {
        print!("{}", message)
    }
}

pub fn info(message: String) {    
    if INFO {
        println!("[INFO] {:?}", message);
    }
}

pub(crate) fn print_value(value: Value, new_line: bool) {
    match value {
        Value::Obj(obj) => match obj {
            Obj::Str(fat_ptr) => unsafe {
                let str = memory::read_string(fat_ptr.ptr, fat_ptr.size);
                debug(format!("constant value: {:?}", str), new_line);
            },
            _ => debug(format!("constant value: {:?}", obj), new_line),
        },
        _ => debug(format!("constant value: {:?}", value), new_line),
    }
}
