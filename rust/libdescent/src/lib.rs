//! libdescent — Rust rewrite of the descent parser generator.
//!
//! Pipeline (mirrors Ruby descent, see rust/PROGRESS.md):
//!   lexer (Tokens) -> parser (AST) -> ir_builder (target-neutral IR)
//!   -> emitter (per-target context + minijinja templates).
//!
//! The hand-ported `lexer` module is the differential oracle / reference
//! front-end; the production front-end will be a thin reader over udon-core
//! events producing the same `Token`s (see `rust/spikes/`).

pub mod ast;
pub mod charclass;
pub mod dump;
pub mod emit;
pub mod ir;
pub mod ir_builder;
pub mod lexer;
pub mod parser;

pub use ast::Machine;
pub use ir::ParserIR;
pub use ir_builder::{IRBuilder, ValidationError};
pub use lexer::{Lexer, LexerError, Token};
pub use parser::{ParseError, Parser};

/// Convenience: content -> AST.
pub fn parse(content: &str, source_file: &str) -> Result<Machine, Box<dyn std::error::Error>> {
    let tokens = Lexer::new(content, source_file).tokenize()?;
    Ok(Parser::new(tokens).parse()?)
}

/// Convenience: content -> target-neutral IR.
pub fn build_ir(content: &str, source_file: &str) -> Result<ParserIR, Box<dyn std::error::Error>> {
    let machine = parse(content, source_file)?;
    Ok(IRBuilder::new(&machine).build()?)
}
