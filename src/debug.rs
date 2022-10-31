use crate::common::{Obj, Value};

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

pub(crate) fn same_line_info(value: Value) {
    unsafe {
        let ptr = Into::<*mut Obj>::into(value);

        if ptr.is_null() {
            print!("[{:?}] ", "NullPtr")
        } else {
            print!("[{:?}] ", *(ptr))
        }
    }
}


pub fn is_debug() -> bool {
    // let debug_flag = std::env::args().nth(1);
    // Some("debug") == debug_flag.as_deref()
    true
}


pub(crate) fn print_value(value: Value, new_line: bool) {
    match value {
        Value::Obj(ptr) =>
            unsafe {
                debug(format!(
                    "constant value: {:?}",
                    *ptr
                ), new_line);
            },
        _ => debug(format!(
            "constant value: {:?}",
            value
        ), new_line)
    }
}