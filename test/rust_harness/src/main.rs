//! CLI for running descent-generated parsers.
//!
//! Reads input from stdin, parses it, and outputs events one per line.
//!
//! Usage: run_parser < input.txt
//!
//! Output format: Event variant with human-readable content.
//! For content types, byte slices are shown as UTF-8 strings when valid.

use descent_harness::Parser;
use std::io::Read;

fn main() {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).expect("Failed to read stdin");

    Parser::new(&input).parse(|event| {
        println!("{}", event.format_line());
    });
}
