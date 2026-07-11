//! SPIKE probe for desc-format proposal #2 (comment-rule unification):
//! empirically compare Ruby descent's comment rule (strip_comments: per-line,
//! `;` at bracket-depth 0 outside single/double quotes) against udon-core's
//! comment events (CommentStart/CommentEnd) on the RAW .desc bytes.
//!
//! Output: per-file site classification —
//!   MATCH      both rules start a comment at the same byte
//!   RUBY_ONLY  Ruby strips it, udon does not comment it (udon would leak
//!              comment bytes into content)
//!   UDON_ONLY  udon comments it, Ruby keeps it (udon would eat live bytes —
//!              the dangerous class: quoted-';' args, degraded regions)
//!
//! Usage: comment_audit FILE.desc [-v]

use std::ops::Range;
use std::process::ExitCode;

/// Ruby Lexer#strip_comments logic: return global byte offsets where Ruby
/// starts a comment (the `;` position), one max per line.
fn ruby_comment_starts(source: &[u8]) -> Vec<usize> {
    let mut starts = vec![];
    let mut line_start = 0;

    for line in source.split_inclusive(|&b| b == b'\n') {
        let mut depth: i32 = 0;
        let mut in_quote: Option<u8> = None;
        let mut prev: Option<u8> = None;

        for (i, &c) in line.iter().enumerate() {
            if c == b'\'' && prev != Some(b'\\') && in_quote != Some(b'"') {
                in_quote = if in_quote == Some(b'\'') { None } else { Some(b'\'') };
            } else if c == b'"' && prev != Some(b'\\') && in_quote != Some(b'\'') {
                in_quote = if in_quote == Some(b'"') { None } else { Some(b'"') };
            } else if in_quote.is_none() {
                match c {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b';' => {
                        if depth == 0 {
                            starts.push(line_start + i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            prev = Some(c);
        }
        line_start += line.len();
    }
    starts
}

/// udon-core comment regions (outermost CommentStart..CommentEnd pairs).
fn udon_comment_regions(source: &[u8]) -> Vec<Range<usize>> {
    let mut regions: Vec<Range<usize>> = vec![];
    let mut depth = 0usize;
    let mut open_start = 0usize;

    udon_core::Parser::new(source).parse(|ev| {
        use udon_core::Event::*;
        match ev {
            CommentStart { span } => {
                if depth == 0 {
                    open_start = span.start;
                }
                depth += 1;
            }
            CommentEnd { span } => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    regions.push(open_start..span.end);
                }
            }
            _ => {}
        }
    });
    regions
}

fn line_of(source: &[u8], pos: usize) -> usize {
    source[..pos.min(source.len())].iter().filter(|&&b| b == b'\n').count() + 1
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: comment_audit <file.desc> [-v]");
        return ExitCode::from(2);
    };
    let verbose = args.iter().any(|a| a == "-v");

    let source = match std::fs::read(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let ruby = ruby_comment_starts(&source);
    let udon = udon_comment_regions(&source);

    let mut matched = 0;
    let mut ruby_only: Vec<usize> = vec![];
    for &r in &ruby {
        // A Ruby comment "matches" if some udon comment region starts at the
        // same `;` (udon spans may or may not include the `;` itself; accept
        // a start within 1 byte) — otherwise udon leaks these bytes as content.
        if udon.iter().any(|u| u.start == r || u.start == r + 1 || (u.start <= r && r < u.end)) {
            matched += 1;
        } else {
            ruby_only.push(r);
        }
    }

    let udon_only: Vec<&Range<usize>> = udon
        .iter()
        .filter(|u| !ruby.iter().any(|&r| u.start == r || u.start == r + 1 || (u.start <= r && r < u.end)))
        .collect();

    println!(
        "{path}: ruby_comments={} udon_comments={} match={matched} ruby_only={} udon_only={}",
        ruby.len(),
        udon.len(),
        ruby_only.len(),
        udon_only.len()
    );

    if verbose {
        for &r in &ruby_only {
            let eol = source[r..].iter().position(|&b| b == b'\n').map(|p| r + p).unwrap_or(source.len());
            println!(
                "  RUBY_ONLY L{}: {}",
                line_of(&source, r),
                String::from_utf8_lossy(&source[r..eol.min(r + 60)])
            );
        }
        for u in &udon_only {
            println!(
                "  UDON_ONLY L{}: {}",
                line_of(&source, u.start),
                String::from_utf8_lossy(&source[u.start..u.end.min(u.start + 60)])
            );
        }
    }

    ExitCode::SUCCESS
}
