#![feature(proc_macro_hygiene)]
#![allow(unused_imports)]

use skyline::{hook, install_hook};

mod hooks;
mod nro_hook;
mod rtld;

// I've copy pasted this from jugeeya so much
#[macro_export]
macro_rules! c_str {
    ($l:tt) => {
        [$l.as_bytes(), "\u{0}".as_bytes()].concat().as_ptr();
    }
}

extern "C" fn test() -> u32 {
    2
}

#[hook(replace = test)]
fn test_replacement() -> u32 {

    let original_test = original!();

    let val = original_test();

    println!("[override] original value: {}", val); // 2

    val + 1
}

#[skyline::main(name = "smashline_hook")]
pub fn main() {
    println!("Hello from Skyline Rust Plugin!");

    install_hook!(test_replacement);

    let x = test();

    println!("[main] test returned: {}", x); // 3
}
