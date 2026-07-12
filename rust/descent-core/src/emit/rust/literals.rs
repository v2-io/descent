//! Rust literal rendering. Port of the target-specific half of Ruby's
//! `CharacterClass` (to_rust_byte / to_rust_bytes / escape helpers, from
//! lib/descent/ir_builder.rb) — relocated out of the IR builder so that Rust
//! literals are baked in only at emit time (the January-flaw fix).

use crate::charclass::ClassParse;

/// Convert a parsed class result to a Rust byte literal for a `:byte` (u8)
/// param. Param refs pass through as the bare parameter name.
pub fn to_rust_byte(result: &ClassParse) -> String {
    if let Some(p) = &result.param_ref {
        return p.clone();
    }
    let bytes = result.bytes.as_deref().unwrap_or("");
    if result.chars.is_empty() && bytes.is_empty() {
        return "0u8".to_string(); // Empty = never-match sentinel
    }

    let ch = bytes
        .chars()
        .next()
        .or_else(|| result.chars.first().and_then(|s| s.chars().next()))
        .unwrap_or('?');
    escape_rust_byte(ch)
}

/// Convert a parsed class result to a Rust byte string for a `:bytes`
/// (&[u8]) param.
pub fn to_rust_bytes(result: &ClassParse) -> String {
    if let Some(p) = &result.param_ref {
        return p.clone();
    }
    match result.bytes.as_deref() {
        None | Some("") => "b\"\"".to_string(),
        Some(bytes) => format!("b\"{}\"", escape_rust_byte_string(bytes)),
    }
}

/// Escape a single character for a Rust byte literal `b'x'`.
pub fn escape_rust_byte(ch: char) -> String {
    let escaped = match ch {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        '\r' => "\\r".to_string(),
        '\0' => "\\0".to_string(),
        '\\' => "\\\\".to_string(),
        '\'' => "\\'".to_string(),
        c if (c as u32) < 32 || (c as u32) > 126 => format!("\\x{:02x}", c as u32),
        c => c.to_string(),
    };
    format!("b'{escaped}'")
}

/// Escape a string for a Rust byte string literal `b"..."`.
pub fn escape_rust_byte_string(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '\n' => "\\n".to_string(),
            '\t' => "\\t".to_string(),
            '\r' => "\\r".to_string(),
            '\0' => "\\0".to_string(),
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            c if (c as u32) < 32 || (c as u32) > 126 => format!("\\x{:02x}", c as u32),
            c => c.to_string(),
        })
        .collect()
}
