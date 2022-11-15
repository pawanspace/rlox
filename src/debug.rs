use crate::common::{Obj, Value};
use crate::memory;

pub fn debug(message: String, new_line: bool) {
    if is_debug() && new_line {
        println!("[DEBUG] {}", message)
    } else if is_debug() && !new_line {
        print!("{}", message)
    }
}

pub fn info(message: String) {
    println!("[INFO] {:?}", message)
}

pub fn is_debug() -> bool {
    // let debug_flag = std::env::args().nth(1);
    // Some("debug") == debug_flag.as_deref()
    true
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
