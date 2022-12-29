use std::collections::HashMap;
use std::time::{Instant, Duration};

static mut EVENTS: Option<HashMap<String, Duration>> = None;

fn init_events() {
    unsafe {
        if matches!(EVENTS, None) {
            EVENTS = Some(HashMap::new());
        }
    }
}

pub(crate) fn record<R>(name: String, mut func: impl FnMut() -> R) -> R {
    init_events();  
    let start = Instant::now();
    let result = func();
    let total_time = start.elapsed();
    unsafe { EVENTS.as_mut().unwrap().insert(name, total_time); }        
    result
}

pub(crate) fn display() {
    println!("\n\n\n");
    unsafe {
        EVENTS.as_ref().unwrap().iter().for_each(|(key, value)| {
            println!("***** {:?}: {:?} *****", key, value);
        });
    }
}

