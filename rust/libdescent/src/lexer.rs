//! Tokenizer for .desc files (pipe-delimited UDON-shaped format).
//!
//! Faithful port of Ruby descent's `lib/descent/lexer.rb`, including its three
//! (mutually inconsistent) scanning layers:
//!   1. `strip_comments` — whole-content pre-pass, per-line bracket depth
//!      (depth resets each line), both quote kinds tracked.
//!   2. `split_on_pipes` — whole-content, single-level "sticky" bracket flag
//!      (`[` sets, `]` clears), both quote kinds, quotes escape-aware.
//!   3. `parse_part` — per-part comment re-strip with bracket AND paren depth,
//!      single quotes only; `extract_bracketed_id` with true nesting depth.
//! These layers are quirky but they are the executable spec; see PROGRESS.md
//! "desc-format proposals" for the rationalization queue.
//!
//! This Token stream is the front-end seam: a future descent.desc-generated
//! event parser (self-hosting bootstrap) can replace this module as long as it
//! produces the same `Token`s.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tag: String,
    pub id: String,
    pub rest: String,
    pub lineno: usize,
}

#[derive(Debug)]
pub struct LexerError {
    pub message: String,
    pub lineno: Option<usize>,
    pub source_file: String,
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.lineno {
            Some(l) => write!(f, "{}:{}: {}", self.source_file, l, self.message),
            None => write!(f, "{}: {}", self.source_file, self.message),
        }
    }
}

impl std::error::Error for LexerError {}

pub struct Lexer<'a> {
    content: &'a str,
    source_file: String,
}

impl<'a> Lexer<'a> {
    pub fn new(content: &'a str, source_file: &str) -> Self {
        Lexer { content, source_file: source_file.to_string() }
    }

    pub fn tokenize(&self) -> Result<Vec<Token>, LexerError> {
        let stripped = strip_comments(self.content);
        let stripped_chars: Vec<char> = stripped.chars().collect();

        let raw_parts = split_on_pipes(&stripped, &self.source_file)?;

        // Locate each part in the stripped content to derive line numbers
        // (mirrors Ruby: index search from a moving cursor, char-based).
        let mut parts: Vec<(String, usize)> = Vec::new();
        let mut current_pos: usize = 0; // char index
        for part in &raw_parts {
            if part.trim().is_empty() {
                continue;
            }
            let part_chars: Vec<char> = part.chars().collect();
            let found_pos = char_index_of(&stripped_chars, &part_chars, current_pos).unwrap_or(current_pos);
            let lineno = stripped_chars[..found_pos].iter().filter(|&&c| c == '\n').count() + 1;
            parts.push((rstrip(part).to_string(), lineno));
            current_pos = found_pos + part_chars.len();
        }

        let mut tokens = Vec::new();
        for (part, line) in parts {
            if let Some(tok) = parse_part(&part, line, &self.source_file)? {
                tokens.push(tok);
            }
        }
        Ok(tokens)
    }
}

/// Ruby String#index(substr, offset) on char indices.
fn char_index_of(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(haystack.len()));
    }
    if from >= haystack.len() {
        return None;
    }
    let end = haystack.len().saturating_sub(needle.len());
    (from..=end).find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// Ruby String#rstrip (trailing whitespace incl. \n, \0).
fn rstrip(s: &str) -> &str {
    s.trim_end_matches(|c: char| c.is_whitespace() || c == '\0')
}

/// Ruby String#strip.
fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\0')
}

/// Layer 1: strip comments, preserving line structure.
/// Per line: bracket depth (resets each line), both quote kinds (escape-aware),
/// `;` at depth 0 outside quotes starts a comment.
fn strip_comments(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    // Ruby String#lines: split after \n, keeping the terminator.
    for line in split_lines_keep_nl(content) {
        let chars: Vec<char> = line.chars().collect();
        let mut depth: i64 = 0;
        let mut in_quote: Option<char> = None;
        let mut prev_char: Option<char> = None;
        let mut comment_start: Option<usize> = None;

        for (i, &c) in chars.iter().enumerate() {
            if c == '\'' && prev_char != Some('\\') && in_quote != Some('"') {
                in_quote = if in_quote == Some('\'') { None } else { Some('\'') };
            } else if c == '"' && prev_char != Some('\\') && in_quote != Some('\'') {
                in_quote = if in_quote == Some('"') { None } else { Some('"') };
            } else if in_quote.is_none() {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    ';' => {
                        if depth == 0 {
                            comment_start = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            prev_char = Some(c);
        }

        match comment_start {
            Some(cs) => {
                let kept: String = chars[..cs].iter().collect();
                out.push_str(rstrip(&kept));
                out.push('\n');
            }
            None => out.push_str(line),
        }
    }
    out
}

fn split_lines_keep_nl(s: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, b) in s.bytes().enumerate() {
        if b == b'\n' {
            lines.push(&s[start..=i]);
            start = i + 1;
        }
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

/// Layer 2: split content on pipes, but not inside bracket context or quotes.
///
/// `pub`: reader seam — the udon-core front-end re-splits sameline command
/// tails (UDON Text runs containing pipes) with this exact splitter.
pub fn split_on_pipes(content: &str, source_file: &str) -> Result<Vec<String>, LexerError> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_bracket = false;
    let mut in_quote: Option<char> = None;
    let mut prev_char: Option<char> = None;
    let mut lineno: usize = 1;
    let mut quote_start_line: Option<usize> = None;

    for c in content.chars() {
        if c == '\n' {
            lineno += 1;
        }
        match c {
            '\'' => {
                current.push(c);
                if in_quote == Some('\'') && prev_char != Some('\\') {
                    in_quote = None;
                } else if in_quote.is_none() {
                    in_quote = Some('\'');
                    quote_start_line = Some(lineno);
                }
            }
            '"' => {
                current.push(c);
                if in_quote == Some('"') && prev_char != Some('\\') {
                    in_quote = None;
                } else if in_quote.is_none() {
                    in_quote = Some('"');
                    quote_start_line = Some(lineno);
                }
            }
            '[' => {
                // Only first [ opens the bracket context - nested [ are literal
                if in_quote.is_none() {
                    in_bracket = true;
                }
                current.push(c);
            }
            ']' => {
                // ] always closes the bracket context (only one level)
                current.push(c);
                if in_quote.is_none() {
                    in_bracket = false;
                }
            }
            '|' => {
                if in_bracket || in_quote.is_some() {
                    current.push(c);
                } else {
                    if !current.is_empty() {
                        parts.push(std::mem::take(&mut current));
                    }
                }
            }
            _ => current.push(c),
        }
        prev_char = Some(c);
    }

    if let Some(q) = in_quote {
        return Err(LexerError {
            message: format!(
                "Unterminated {} quote - opened but never closed",
                if q == '\'' { "single" } else { "double" }
            ),
            lineno: quote_start_line,
            source_file: source_file.to_string(),
        });
    }

    if !current.is_empty() {
        parts.push(current);
    }
    Ok(parts)
}

/// Extract the content inside [...] from a part, respecting single quotes.
/// Returns (content, Some(end_char_pos)) or ("", None) if no brackets found.
///
/// `pub`: reader seam — the udon-core front-end re-extracts bracket-ids from
/// raw source with these exact rules (single quotes only, true nesting),
/// because udon-core fragments/garbles ids containing spaces or quoted
/// whitespace.
pub fn extract_bracketed_id(part: &[char]) -> (String, Option<usize>) {
    let start_pos = match part.iter().position(|&c| c == '[') {
        Some(p) => p,
        None => return (String::new(), None),
    };
    let mut i = start_pos + 1;
    let mut depth: i64 = 1;
    let mut in_quote = false;
    let mut content = String::new();

    while i < part.len() && depth > 0 {
        let c = part[i];
        match c {
            '\'' => {
                content.push(c);
                in_quote = !in_quote;
            }
            '[' => {
                content.push(c);
                if !in_quote {
                    depth += 1;
                }
            }
            ']' => {
                if in_quote {
                    content.push(c);
                } else {
                    depth -= 1;
                    if depth > 0 {
                        content.push(c);
                    }
                }
            }
            _ => content.push(c),
        }
        i += 1;
    }

    (content, if depth == 0 { Some(i) } else { None })
}

/// Layer 3: per-part comment strip (brackets + parens + single quotes),
/// then tag/id/rest decomposition.
///
/// `pub`: reader seam — the udon-core front-end reconstructs descent *parts*
/// from UDON events and feeds them through this shared decomposition, so
/// tag-casing/id/rest quirks live in exactly one place.
pub fn parse_part(part: &str, lineno: usize, source_file: &str) -> Result<Option<Token>, LexerError> {
    // Comment strip round 2, per line within the part.
    let mut kept_lines: Vec<String> = Vec::new();
    for (line_idx, line) in split_lines_keep_nl(part).iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut bracket_depth: i64 = 0;
        let mut paren_depth: i64 = 0;
        let mut in_quote = false;
        let mut quote_start_col: Option<usize> = None;
        let mut comment_start: Option<usize> = None;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_quote {
                if c == '\'' && (i == 0 || chars[i - 1] != '\\') {
                    in_quote = false;
                }
            } else {
                match c {
                    '\'' => {
                        in_quote = true;
                        quote_start_col = Some(i);
                    }
                    '[' => bracket_depth += 1,
                    ']' => bracket_depth -= 1,
                    '(' => paren_depth += 1,
                    ')' => paren_depth -= 1,
                    ';' => {
                        if bracket_depth == 0 && paren_depth == 0 {
                            comment_start = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }

        if in_quote {
            return Err(LexerError {
                message: format!(
                    "Unterminated single quote at column {}",
                    quote_start_col.unwrap_or(0) + 1
                ),
                lineno: Some(lineno + line_idx),
                source_file: source_file.to_string(),
            });
        }

        let kept: String = match comment_start {
            Some(cs) => chars[..cs].iter().collect(),
            None => chars.iter().collect(),
        };
        kept_lines.push(rstrip(&kept).to_string());
    }
    let part = strip(&kept_lines.join("\n")).to_string();
    let part_chars: Vec<char> = part.chars().collect();

    // Extract raw tag.
    let fn_call_re = re(r"^/\w+\(");
    let fn_call_full_re = re(r"^/\w+\([^)]*\)");
    let simple_tag_re = re(r"^(\.|[^ \[]+)");

    let raw_tag: String = if fn_call_re.is_match(&part) {
        match fn_call_full_re.find(&part) {
            Some(m) => m.as_str().to_string(),
            None => simple_tag_re
                .find(&part)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
        }
    } else {
        simple_tag_re
            .find(&part)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    let raw_tag = strip(&raw_tag).to_string();

    let tag: String = if re(r"(?i)^emit\(").is_match(&raw_tag) {
        raw_tag.clone()
    } else if fn_call_re.is_match(&raw_tag) {
        // Function call - downcase name, preserve case of arguments
        let name = re(r"^/(\w+)\(").captures(&raw_tag).unwrap()[1].to_string();
        let args = re(r"\(([^)]*)\)")
            .captures(&raw_tag)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        format!("/{}({})", name.to_lowercase(), args)
    } else if re(r"^[A-Z]+(_[A-Z]+)*$").is_match(&raw_tag) {
        // SCREAMING_SNAKE_CASE - character class name; lowercase it
        raw_tag.to_lowercase()
    } else if raw_tag.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        // PascalCase - inline type emit, preserve case
        raw_tag.clone()
    } else {
        raw_tag.to_lowercase()
    };

    // Extract ID from brackets.
    let (id, id_end_pos) = extract_bracketed_id(&part_chars);

    // Strip the tag prefix to get the remainder.
    let after_tag: String = if fn_call_re.is_match(&raw_tag) {
        fn_call_full_re.replace(&part, "").to_string()
    } else {
        simple_tag_re.replace(&part, "").to_string()
    };

    let rest: String = if id_end_pos.is_some() {
        let after_chars: Vec<char> = after_tag.chars().collect();
        match after_chars.iter().position(|&c| c == '[') {
            Some(bracket_pos) => {
                let from = bracket_pos + id.chars().count() + 2;
                if from <= after_chars.len() {
                    let tail: String = after_chars[from..].iter().collect();
                    strip(&tail).to_string()
                } else {
                    String::new()
                }
            }
            None => String::new(), // Ruby would raise here; treat as empty
        }
    } else {
        strip(&after_tag).to_string()
    };

    // For parser name and similar, take only first word/line.
    let rest = if tag == "parser" || tag == "entry-point" {
        rest.split('\n').next().map(|s| strip(s).to_string()).unwrap_or_default()
    } else {
        rest
    };

    if tag.is_empty() && id.is_empty() && rest.is_empty() {
        return Ok(None);
    }

    Ok(Some(Token { tag, id, rest, lineno }))
}

/// Compiled-regex cache helper.
pub(crate) fn re(pattern: &str) -> regex::Regex {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = cache.lock().unwrap();
    map.entry(pattern.to_string())
        .or_insert_with(|| regex::Regex::new(pattern).unwrap())
        .clone()
}
