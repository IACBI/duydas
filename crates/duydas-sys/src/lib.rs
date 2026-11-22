//! Operating-system bindings for Duydaş, written by hand instead of pulled from crates.
//!
//! This is the only crate in the workspace allowed to use `unsafe`. Every block carries a
//! `SAFETY:` comment, does one unsafe operation, and is wrapped in a safe function.

pub mod locale;
