//! descent-rs CLI: `generate` (parser generation) plus the
//! differential-testing probe subcommands (tokens/ast/context).
//!
//! Front-end: udon-core reader by default; `--oracle` selects the
//! hand-ported lexer (the differential oracle — used by diff_reader.sh as
//! the reference side).

use descent_core::Frontend;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let frontend = if args.iter().any(|a| a == "--oracle") {
        Frontend::OracleLexer
    } else {
        Frontend::UdonCore
    };
    match (args.get(1).map(|s| s.as_str()), args.get(2)) {
        (Some("tokens"), Some(path)) => dump(path, false, frontend),
        (Some("ast"), Some(path)) => dump(path, true, frontend),
        (Some("context"), Some(path)) => {
            let trace = args.iter().skip(3).any(|s| s == "true");
            dump_context(path, trace, frontend)
        }
        (Some("classify"), Some(path)) => classify(path, frontend),
        (Some("generate"), Some(path)) => {
            let trace = args.iter().skip(3).any(|s| s == "--trace" || s == "true");
            if let Some(bi) = args.iter().position(|s| s == "--backend") {
                if args.get(bi + 1).map(|s| s.as_str()) == Some("pushdown") {
                    return generate_pushdown(path, &args, frontend);
                }
            }
            generate(path, trace, frontend)
        }
        _ => {
            eprintln!("usage: descent-rs <tokens|ast|context> <file.desc> [trace] [--oracle]");
            eprintln!("       descent-rs generate <file.desc> [--trace] [--oracle]");
            eprintln!("       descent-rs generate <file.desc> --backend pushdown [--event-path <rust::path>]");
            ExitCode::from(2)
        }
    }
}

/// Generate Rust parser source to stdout (mirrors Ruby
/// `Descent.generate(file, target: :rust, trace:)` plus the regenerate
/// driver's blank-run collapse — see emit::rust::engine::post_process).
fn generate(path: &str, trace: bool, frontend: Frontend) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ir = match descent_core::build_ir_with(&content, path, frontend) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let opts = descent_core::emit::rust::Options { trace, ..Default::default() };
    match descent_core::emit::rust::generate(&ir, &opts) {
        Ok(code) => {
            print!("{code}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("template error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Generate the pushdown (explicit-stack, resumable) parser to stdout.
fn generate_pushdown(path: &str, args: &[String], frontend: Frontend) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ir = match descent_core::build_ir_with(&content, path, frontend) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut opts = descent_core::emit::rust_pushdown::PdOptions::default();
    if let Some(pi) = args.iter().position(|s| s == "--event-path") {
        if let Some(p) = args.get(pi + 1) {
            opts.event_path = p.clone();
        }
    }
    print!("{}", descent_core::emit::rust_pushdown::generate(&ir, &opts));
    ExitCode::SUCCESS
}

/// Report-only positional/delimited classification of each grammar function.
fn classify(path: &str, frontend: Frontend) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ir = match descent_core::build_ir_with(&content, path, frontend) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    print!("{}", descent_core::classify::report(&ir));
    ExitCode::SUCCESS
}

/// Dump the Rust-emitter template context (differential vs
/// rust/tools/dump_context.rb on the Ruby side).
fn dump_context(path: &str, trace: bool, frontend: Frontend) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ir = match descent_core::build_ir_with(&content, path, frontend) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let opts = descent_core::emit::rust::Options { trace, ..Default::default() };
    let ctx = descent_core::emit::rust::build_context(&ir, &opts);
    println!("{}", serde_json::to_string_pretty(&ctx).unwrap());
    ExitCode::SUCCESS
}

fn dump(path: &str, ast: bool, frontend: Frontend) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let tokens = match descent_core::tokenize(&content, path, frontend) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("LexerError: {e}");
            return ExitCode::FAILURE;
        }
    };
    let value = if ast {
        match descent_core::Parser::new(tokens).parse() {
            Ok(m) => descent_core::dump::machine_to_json(&m),
            Err(e) => {
                eprintln!("ParseError: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        descent_core::dump::tokens_to_json(&tokens)
    };
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
    ExitCode::SUCCESS
}
