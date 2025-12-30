//! Test harness for descent-generated parsers.
//!
//! The `generated` module contains the parser under test.
//! Ruby tests write to `src/generated.rs` before running.

#[allow(dead_code)]
mod generated;

pub use generated::*;
