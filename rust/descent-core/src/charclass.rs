//! Unified character class parser. Port of Ruby descent's
//! `CharacterClass` (lib/descent/ir_builder.rb, module CharacterClass) —
//! *minus* the Rust-literal renderers (`to_rust_byte`/`to_rust_bytes` and the
//! escape helpers), which are target-specific and live in `emit::rust`
//! (that relocation is the fix for the January Rust-literals-in-IR flaw).
//!
//! Handles all character literal and class syntax:
//! - Single chars: `'x'`, `'\n'`, `'\x00'`
//! - Strings: `'hello'` (decomposed to chars for classes)
//! - Classes: `<...>` with space-separated tokens
//! - Predefined classes: `LETTER`, `DIGIT`, `SQ`, `P`, `0-9`, ...
//! - Empty class: `<>` (empty set / empty string)
//! - Param refs: `:name`
//!
//! The same parsing is used everywhere: `c[...]`, function args, `PREPEND`.

use crate::lexer::re;

/// Predefined single-character classes (DSL-reserved chars).
pub const SINGLE_CHAR: &[(&str, &str)] = &[
    ("P", "|"),
    ("L", "["),
    ("R", "]"),
    ("LB", "{"),
    ("RB", "}"),
    ("LP", "("),
    ("RP", ")"),
    ("SQ", "'"),
    ("DQ", "\""),
    ("BS", "\\"),
];

/// Predefined character ranges (keys are case-sensitive, matched verbatim).
pub const RANGES: &[(&str, &str)] = &[
    ("0-9", "0123456789"),
    ("0-7", "01234567"),
    ("0-1", "01"),
    ("a-z", "abcdefghijklmnopqrstuvwxyz"),
    ("A-Z", "ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
    ("a-f", "abcdef"),
    ("A-F", "ABCDEF"),
];

/// Predefined multi-character classes (expanded to char sets).
pub const MULTI_CHAR: &[(&str, &str)] = &[
    (
        "LETTER",
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
    ),
    ("DIGIT", "0123456789"),
    ("HEX_DIGIT", "0123456789abcdefABCDEF"),
    (
        "LABEL_CONT",
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-",
    ),
    ("WS", " \t"),
    ("NL", "\n"),
];

/// Special classes that require runtime checks (can't be expanded to a char list).
pub const SPECIAL_CLASSES: &[&str] = &["XID_START", "XID_CONT", "XLBL_START", "XLBL_CONT"];

fn lookup<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Result of parsing a class specification. Mirrors Ruby's
/// `{ chars:, special_class:, param_ref:, bytes: }` hash, including the
/// nil-vs-empty distinction on `bytes`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassParse {
    /// Literal chars (each entry one character, as a String for parity with
    /// Ruby's `String#chars`).
    pub chars: Vec<String>,
    /// Downcased special class name (e.g. "xid_start"), if any.
    pub special_class: Option<String>,
    /// Parameter name for `:name` refs, if any.
    pub param_ref: Option<String>,
    /// Raw byte content; `None` mirrors Ruby's `bytes: nil` (special classes
    /// and param refs), `Some("")` mirrors `bytes: ''`.
    pub bytes: Option<String>,
}

impl ClassParse {
    fn empty() -> Self {
        ClassParse { chars: vec![], special_class: None, param_ref: None, bytes: Some(String::new()) }
    }
}

fn str_chars(s: &str) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

/// Parse a class specification string (contents of `c[...]` or `<...>` or bare).
/// Faithful port of `CharacterClass.parse` (the `context:` parameter is
/// dropped: Ruby only threads it through without ever consulting it).
pub fn parse(input: &str) -> ClassParse {
    if input.is_empty() {
        return ClassParse::empty();
    }

    let s = input.trim();

    // Handle explicit class wrapper <...>
    if s.starts_with('<') && s.ends_with('>') && s.len() >= 2 {
        let inner = s[1..s.len() - 1].trim();
        if inner.is_empty() {
            return ClassParse::empty(); // <>
        }
        return parse_class_content(inner);
    }

    // Handle param reference :name
    if let Some(param) = s.strip_prefix(':') {
        return ClassParse {
            chars: vec![],
            special_class: None,
            param_ref: Some(param.to_string()),
            bytes: None,
        };
    }

    // Handle quoted string 'content' (Ruby: /^'.*'$/ — line anchors)
    let n_chars = s.chars().count();
    if re("(?m)^'.*'$").is_match(s) && n_chars >= 2 {
        let content = parse_quoted_string(&s[1..s.len() - 1]);
        return ClassParse {
            chars: str_chars(&content),
            special_class: None,
            param_ref: None,
            bytes: Some(content),
        };
    }

    // Handle double-quoted string "content"
    if re("(?m)^\".*\"$").is_match(s) && n_chars >= 2 {
        let content = s[1..s.len() - 1].to_string();
        return ClassParse {
            chars: str_chars(&content),
            special_class: None,
            param_ref: None,
            bytes: Some(content),
        };
    }

    // Bare shorthand (only /[A-Za-z0-9_-]/ allowed)
    if re("(?m)^[A-Za-z0-9_-]+$").is_match(s) {
        let upper = s.to_uppercase();
        if SPECIAL_CLASSES.contains(&upper.as_str()) {
            return ClassParse {
                chars: vec![],
                special_class: Some(upper.to_lowercase()),
                param_ref: None,
                bytes: None,
            };
        } else if let Some(expansion) = lookup(MULTI_CHAR, &upper) {
            return ClassParse {
                chars: str_chars(expansion),
                special_class: None,
                param_ref: None,
                bytes: Some(expansion.to_string()),
            };
        } else if let Some(ch) = lookup(SINGLE_CHAR, &upper) {
            return ClassParse {
                chars: vec![ch.to_string()],
                special_class: None,
                param_ref: None,
                bytes: Some(ch.to_string()),
            };
        } else if let Some(range) = lookup(RANGES, s) {
            return ClassParse {
                chars: str_chars(range),
                special_class: None,
                param_ref: None,
                bytes: Some(range.to_string()),
            };
        } else {
            // Bare alphanumeric - decompose to individual chars
            return ClassParse {
                chars: str_chars(s),
                special_class: None,
                param_ref: None,
                bytes: Some(s.to_string()),
            };
        }
    }

    // Invalid bare content (special chars without quotes): treat as literal
    // bytes (Ruby comments "this should probably error").
    ClassParse {
        chars: str_chars(s),
        special_class: None,
        param_ref: None,
        bytes: Some(s.to_string()),
    }
}

/// Parse the content inside `<...>` (space-separated tokens).
pub fn parse_class_content(content: &str) -> ClassParse {
    if content.is_empty() {
        return ClassParse::empty();
    }

    let mut all_chars: Vec<String> = vec![];
    let mut all_bytes = String::new();
    let mut special_class: Option<String> = None;
    let mut param_ref: Option<String> = None;

    for token in tokenize_class_content(content) {
        let result = parse(&token);
        if result.special_class.is_some() {
            // Only one special class allowed (last one wins, as in Ruby)
            special_class = result.special_class;
        } else if result.param_ref.is_some() {
            param_ref = result.param_ref;
        } else {
            all_chars.extend(result.chars);
            if let Some(b) = result.bytes {
                all_bytes.push_str(&b);
            }
        }
    }

    // uniq preserving first occurrence
    let mut seen: Vec<String> = vec![];
    for c in all_chars {
        if !seen.contains(&c) {
            seen.push(c);
        }
    }

    ClassParse { chars: seen, special_class, param_ref, bytes: Some(all_bytes) }
}

/// Tokenize class content respecting single quotes.
pub fn tokenize_class_content(content: &str) -> Vec<String> {
    let chars: Vec<char> = content.chars().collect();
    let mut tokens: Vec<String> = vec![];
    let mut current = String::new();
    let mut in_quote = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !in_quote {
            in_quote = true;
            current.push(c);
        } else if c == '\'' && in_quote {
            current.push(c);
            in_quote = false;
        } else if c == '\\' && in_quote && i + 1 < chars.len() {
            current.push(c);
            current.push(chars[i + 1]);
            i += 1;
        } else if c == ' ' && !in_quote {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Parse a quoted string with escape sequences. Port of
/// `CharacterClass.parse_quoted_string`.
///
/// Divergence note: for `\xHH` with HH >= 0x80 Ruby appends a raw byte
/// (producing a binary string); we map it to the Unicode codepoint U+00HH.
/// No corpus grammar uses high-byte escapes.
pub fn parse_quoted_string(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' {
            if i + 1 < chars.len() {
                match chars[i + 1] {
                    'n' => {
                        result.push('\n');
                        i += 2;
                    }
                    't' => {
                        result.push('\t');
                        i += 2;
                    }
                    'r' => {
                        result.push('\r');
                        i += 2;
                    }
                    '\\' => {
                        result.push('\\');
                        i += 2;
                    }
                    '\'' => {
                        result.push('\'');
                        i += 2;
                    }
                    '"' => {
                        result.push('"');
                        i += 2;
                    }
                    'x' => {
                        // Hex byte: \xHH (Ruby requires a char after the two
                        // hex digits: `i + 3 < str.length`)
                        if i + 3 < chars.len()
                            && chars[i + 2].is_ascii_hexdigit()
                            && chars[i + 3].is_ascii_hexdigit()
                        {
                            let hex: String = chars[i + 2..=i + 3].iter().collect();
                            let byte = u8::from_str_radix(&hex, 16).unwrap();
                            result.push(byte as char);
                            i += 4;
                        } else {
                            result.push(chars[i + 1]);
                            i += 2;
                        }
                    }
                    'u' => {
                        // Unicode: \uXXXX (same off-by-one shape as \x)
                        if i + 5 < chars.len() && chars[i + 2..=i + 5].iter().all(|c| c.is_ascii_hexdigit()) {
                            let hex: String = chars[i + 2..=i + 5].iter().collect();
                            let cp = u32::from_str_radix(&hex, 16).unwrap();
                            result.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            i += 6;
                        } else {
                            result.push(chars[i + 1]);
                            i += 2;
                        }
                    }
                    '0' => {
                        result.push('\0');
                        i += 2;
                    }
                    other => {
                        result.push(other);
                        i += 2;
                    }
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}
