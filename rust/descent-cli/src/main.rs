//! descent-rs CLI. Current subcommands are differential-testing probes;
//! `generate` arrives with the emitter (PROGRESS.md step 4-5).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match (args.get(1).map(|s| s.as_str()), args.get(2)) {
        (Some("tokens"), Some(path)) => dump(path, false),
        (Some("ast"), Some(path)) => dump(path, true),
        _ => {
            eprintln!("usage: descent-rs <tokens|ast> <file.desc>");
            ExitCode::from(2)
        }
    }
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
