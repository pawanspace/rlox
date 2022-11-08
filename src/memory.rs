use std::alloc::{alloc, dealloc, Layout};
use std::fmt::Debug;
use std::mem;




pub fn allocate<T>() -> *mut u8 {
    let layout = Layout::new::<T>();
    unsafe {
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("Unable to allocate pointer for layout {:?}", layout);
        }
        ptr
    }
}

pub fn allocate_for_value<T>(value: T) -> *mut u8 {
    let layout = Layout::for_value::<T>(&value);
    println!("Layout size: {:?}", layout.size());
    unsafe {
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("Unable to allocate pointer for layout {:?}", layout);
        }
        ptr
    }
}

pub fn add<T>(ptr: *mut u8, value: T) {
    unsafe {
        std::ptr::write(ptr as *mut T, value);
    }
}

pub fn size_of<T>(ptr: *mut u8) -> usize {
    unsafe { mem::size_of_val(&ptr) }
}

pub fn print<T>(ptr: *mut u8) where T: Debug {
    unsafe {
        println!("{:?}", *ptr); }
}

pub fn drop<T>(ptr: *mut u8) {
    let layout = Layout::new::<T>();
    unsafe {
        dealloc(ptr, layout);
    }
}

pub fn get<T>(ptr: *mut T) -> T  {
    unsafe { std::ptr::read(ptr) }
}

pub fn copy(src: *mut u8, dest: *mut u8, length: usize, offset: usize) {
    unsafe {
        std::ptr::copy_nonoverlapping(src, dest.offset(offset as isize), length)
    }
}

