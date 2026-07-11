//! descent-rs CLI: `generate` (parser generation) plus the
//! differential-testing probe subcommands (tokens/ast/context).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match (args.get(1).map(|s| s.as_str()), args.get(2)) {
        (Some("tokens"), Some(path)) => dump(path, false),
        (Some("ast"), Some(path)) => dump(path, true),
        (Some("context"), Some(path)) => {
            dump_context(path, args.get(3).map(|s| s.as_str()) == Some("true"))
        }
        (Some("generate"), Some(path)) => {
            let trace = args.get(3).is_some_and(|s| s == "--trace" || s == "true");
            generate(path, trace)
        }
        _ => {
            eprintln!("usage: descent-rs <tokens|ast|context> <file.desc> [trace]");
            eprintln!("       descent-rs generate <file.desc> [--trace]");
            ExitCode::from(2)
        }
    }
}

/// Generate Rust parser source to stdout (mirrors Ruby
/// `Descent.generate(file, target: :rust, trace:)` plus the regenerate
/// driver's blank-run collapse — see emit::rust::engine::post_process).
fn generate(path: &str, trace: bool) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ir = match libdescent::build_ir(&content, path) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let opts = libdescent::emit::rust::Options { trace, ..Default::default() };
    match libdescent::emit::rust::generate(&ir, &opts) {
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

/// Dump the Rust-emitter template context (differential vs
/// rust/tools/dump_context.rb on the Ruby side).
fn dump_context(path: &str, trace: bool) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ir = match libdescent::build_ir(&content, path) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let opts = libdescent::emit::rust::Options { trace, ..Default::default() };
    let ctx = libdescent::emit::rust::build_context(&ir, &opts);
    println!("{}", serde_json::to_string_pretty(&ctx).unwrap());
    ExitCode::SUCCESS
}

fn dump(path: &str, ast: bool) -> ExitCode {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let tokens = match libdescent::Lexer::new(&content, path).tokenize() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("LexerError: {e}");
            return ExitCode::FAILURE;
        }
    };
    let value = if ast {
        match libdescent::Parser::new(tokens).parse() {
            Ok(m) => libdescent::dump::machine_to_json(&m),
            Err(e) => {
                eprintln!("ParseError: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        libdescent::dump::tokens_to_json(&tokens)
    };
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
    ExitCode::SUCCESS
}
