use std::alloc::{alloc, dealloc, Layout};
use std::fmt::Debug;
use std::mem::size_of_val;
use crate::common::Obj;


pub fn allocate<T>() -> *mut T {
    let layout = Layout::new::<T>();
    unsafe {
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("Unable to allocate pointer for layout {:?}", layout);
        }
        ptr as *mut T
    }
}

pub fn add<T>(ptr: *mut T, value: T) {
    unsafe {
        std::ptr::write(ptr, value);
    }
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

pub fn copy<T>(src: *mut T, dest: *mut T) {
    unsafe {
        std::ptr::copy(src, dest, size_of_val(&*src))
    }
}