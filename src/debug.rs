pub fn debug(message: String) {
    let debug_flag = std::env::args().nth(1);
    if let Some("debug") = debug_flag.as_deref() {
        println!("{}", message)
    }
}
