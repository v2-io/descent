//! Dump udon-core-derived descent Tokens as canonical JSON (same shape as
//! `descent-rs tokens`) for the reader differential.

use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: udon-reader <file.desc>");
        return ExitCode::from(2);
    };
    let source = match std::fs::read(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match udon_reader::Reader::tokens(&source, &path) {
        Ok((tokens, warnings)) => {
            for w in &warnings {
                eprintln!("WARN {w}");
            }
            let json = libdescent::dump::tokens_to_json(&tokens);
            println!("{}", serde_json::to_string_pretty(&json).unwrap());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("LexerError: {e}");
            ExitCode::FAILURE
        }
    }
}
