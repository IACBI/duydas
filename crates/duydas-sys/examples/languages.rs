//! Prints the preferred UI languages the OS reports.

#![allow(clippy::print_stdout, reason = "example output")]

fn main() {
    println!("{:?}", duydas_sys::locale::preferred_languages());
}
