pub fn f(x: &u8) -> u8 {
    unsafe { *std::ptr::from_ref(x) }
}
