pub fn debug(message: String) {
    if is_debug() {
        println!("{}", message)
    }
}

pub fn is_debug() -> bool {
    // let debug_flag = std::env::args().nth(1);
    // Some("debug") == debug_flag.as_deref()
    true
}
