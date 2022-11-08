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

pub fn is_debug() -> bool {
    // let debug_flag = std::env::args().nth(1);
    // Some("debug") == debug_flag.as_deref()
    true
}


pub(crate) fn print_value(value: Value, new_line: bool) {
    match value {
        Value::Obj(obj) =>
            match obj {
                Obj::Str(fat_ptr) => {
                    unsafe {
                        let mut bytes: Vec<u8> = Vec::new();
                        for i in 0..fat_ptr.size {
                            let b = *(fat_ptr.ptr.offset(i as isize));
                            bytes.push(b);
                        }
                        let str = String::from_utf8(bytes);
                        debug(format!(
                            "constant value: {:?}",
                            str.unwrap()
                        ), new_line);
                    }
                }
                _ =>  debug(format!(
                    "constant value: {:?}",
                    obj
                ), new_line)
            }
        _ => debug(format!(
            "constant value: {:?}",
            value
        ), new_line)
    }
}