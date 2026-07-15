//! Emitters: per-target rendering of the target-neutral IR.
//!
//! This is the ONLY layer where target literals (Rust byte literals, escape
//! rendering, expression transpilation) may be produced — the IR itself
//! stays DSL-level (see ir.rs module docs and rust/PROGRESS.md).

pub mod rust;
pub mod rust_pushdown;
