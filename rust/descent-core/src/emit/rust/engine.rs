//! minijinja rendering engine for the Rust target: template environment,
//! Liquid-parity filters (port of Ruby's `LiquidFilters` in
//! lib/descent/generator.rb), and the post-process regex chain.
//!
//! The templates live at `rust/descent_core/templates/rust/{parser,_command}.j2`
//! (translations of Ruby descent's parser.liquid / _command.liquid) and are
//! embedded with `include_str!` so descent-rs is standalone.
//!
//! Liquid-parity notes (byte-identity instrument):
//! - `ltruthy` test = Liquid truthiness (only nil/false are falsy; "" and 0
//!   are truthy) — used wherever the Liquid template did a bare `{% if %}` on
//!   a nullable value.
//! - `ldefault` filter = Liquid `default:` (fires on nil/false/empty, unlike
//!   Jinja's undefined-only `default`).
//! - `render_command(cmd, func, return_type_info)` global fn replaces
//!   Liquid's `{% include 'command' %}` (and its recursion for
//!   conditionals); output is spliced inline exactly like Liquid includes,
//!   preserving the 20-space-indent quirk.
//! - Post-process = generator.rb's 4 regexes + the driver's `\n{3,}` →
//!   `\n\n` (a no-op after the collapse, kept for lockstep).

use crate::lexer::re;
use minijinja::value::Value;
use minijinja::{context, Environment, Error, State, UndefinedBehavior};

const PARSER_TEMPLATE: &str = include_str!("../../../templates/rust/parser.j2");
const COMMAND_TEMPLATE: &str = include_str!("../../../templates/rust/_command.j2");

/// Build the minijinja environment with templates, filters, and tests.
pub fn make_env() -> Result<Environment<'static>, Error> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Chainable);
    // Liquid keeps the template's trailing newline; Jinja strips it.
    env.set_keep_trailing_newline(true);
    // Liquid renders nil as the empty string; minijinja would print "none".
    // (Auto-escape is None for .j2, so plain Display matches the default
    // formatter for everything else.)
    env.set_formatter(|out, _state, value| {
        if value.is_undefined() || value.is_none() {
            return Ok(());
        }
        write!(out, "{value}")
            .map_err(|e| Error::new(minijinja::ErrorKind::WriteFailure, e.to_string()))
    });
    env.add_template("parser.j2", PARSER_TEMPLATE)?;
    env.add_template("_command.j2", COMMAND_TEMPLATE)?;

    env.add_filter("pascalcase", |v: Value| pascalcase(&value_str(&v)));
    env.add_filter("escape_rust_char", |v: Value| escape_rust_char(&v));
    env.add_filter("rust_expr", |v: Value| rust_expr(&value_str(&v)));
    env.add_filter("remove_first", |v: Value, sub: String| {
        value_str(&v).replacen(&sub, "", 1)
    });
    env.add_filter("chars", |v: Value| {
        value_str(&v)
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<String>>()
    });
    env.add_filter("ldefault", |v: Value, d: Value| {
        if liquid_empty(&v) {
            d
        } else {
            v
        }
    });
    // Liquid `.size` semantics: nil has no size (comparisons come out
    // false); minijinja's `length` errors on none. 0 gives the same
    // comparison outcomes as Liquid's nil in the template's guards.
    env.add_filter("lsize", |v: Value| {
        if v.is_undefined() || v.is_none() {
            0
        } else {
            v.len().unwrap_or(0)
        }
    });
    // Liquid to_s: nil -> "" (for string concatenation sites).
    env.add_filter("dstr", |v: Value| value_str(&v));

    env.add_test("ltruthy", |v: Value| liquid_truthy(&v));

    env.add_function(
        "render_command",
        |state: &State, cmd: Value, func: Value, return_type_info: Value| -> Result<String, Error> {
            let tmpl = state.env().get_template("_command.j2")?;
            tmpl.render(context! { cmd, func, return_type_info })
        },
    );

    Ok(env)
}

/// Liquid truthiness: only nil (none/undefined) and false are falsy.
fn liquid_truthy(v: &Value) -> bool {
    !(v.is_undefined() || v.is_none() || *v == Value::from(false))
}

/// Liquid `default:` trigger: nil, false, or empty (string/seq/map).
fn liquid_empty(v: &Value) -> bool {
    if v.is_undefined() || v.is_none() || *v == Value::from(false) {
        return true;
    }
    if let Some(s) = v.as_str() {
        return s.is_empty();
    }
    matches!(v.len(), Some(0))
}

/// Stringify a template value the way Liquid does when filtering (nil → "").
fn value_str(v: &Value) -> String {
    if v.is_undefined() || v.is_none() {
        String::new()
    } else if let Some(s) = v.as_str() {
        s.to_string()
    } else {
        v.to_string()
    }
}

// ---------------------------------------------------------------------------
// Filters (ports of Ruby LiquidFilters)
// ---------------------------------------------------------------------------

/// Rust byte-literal spellings of the DSL escape aliases
/// (Ruby: RUST_ESCAPE_SEQUENCES, derived from ESCAPE_BYTE_VALUES).
fn rust_escape_seq(alias: &str) -> Option<&'static str> {
    Some(match alias {
        "<P>" => "b'|'",
        "<R>" => "b']'",
        "<L>" => "b'['",
        "<RB>" => "b'}'",
        "<LB>" => "b'{'",
        "<RP>" => "b')'",
        "<LP>" => "b'('",
        "<BS>" => "b'\\\\'",
        "<SQ>" => "b'\\''",
        "<DQ>" => "b'\"'",
        "<NL>" => "b'\\n'",
        "<WS>" => "b' '",
        "<>" => "b\"\"",
        _ => return None,
    })
}

/// Convert a character to Rust byte literal format (Ruby: escape_rust_char).
/// Examples: "\n" -> "b'\n'", "|" -> "b'|'", " " -> "b' '"; nil -> "b'?'".
fn escape_rust_char(v: &Value) -> String {
    if v.is_undefined() || v.is_none() {
        return "b'?'".to_string();
    }
    let s = value_str(v);
    let escaped = match s.as_str() {
        "\n" => "\\n".to_string(),
        "\t" => "\\t".to_string(),
        "\r" => "\\r".to_string(),
        "\\" => "\\\\".to_string(),
        "'" => "\\'".to_string(),
        other => other.to_string(),
    };
    format!("b'{escaped}'")
}

/// Convert snake_case/camelCase to PascalCase, preserving existing PascalCase
/// (Ruby: pascalcase — split on `[_\s-]` and lower→upper transitions, then
/// Ruby-`capitalize` each part: first char up, rest DOWN).
pub fn pascalcase(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut parts: Vec<String> = vec![];
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' || c == '-' || c.is_whitespace() {
            parts.push(std::mem::take(&mut cur));
            continue;
        }
        if i > 0 && chars[i - 1].is_ascii_lowercase() && c.is_ascii_uppercase() {
            parts.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    parts.push(cur);
    // Ruby String#split drops trailing empty strings
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
        .iter()
        .map(|p| {
            let mut it = p.chars();
            match it.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &it.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

/// Transform DSL expressions to Rust (Ruby: rust_expr).
/// - /function(args) -> self.parse_function(transformed_args, on_event)
/// - /function -> self.parse_function(on_event)
/// - COL -> self.col(); LINE -> self.line as i32; PREV -> self.prev()
/// - Character literals 'x' -> b'x' (only if not already a byte literal)
/// - Escape aliases <R> <P> ... -> byte literals; :param -> param
pub fn rust_expr(s: &str) -> String {
    // IMPORTANT: function calls FIRST, before COL/LINE/PREV expansion —
    // otherwise /element(COL) becomes /element(self.col()) and [^)]* breaks
    // on the ) inside self.col().
    let result = re(r"/(\w+)\(([^)]*)\)")
        .replace_all(s, |caps: &regex::Captures| {
            let func = &caps[1];
            let args = transform_call_args(&caps[2]);
            let args = expand_special_vars(&args);
            format!("self.parse_{func}({args}, on_event)")
        })
        .into_owned();
    let result = re(r"/(\w+)")
        .replace_all(&result, |caps: &regex::Captures| {
            format!("self.parse_{}(on_event)", &caps[1])
        })
        .into_owned();

    // Now expand special variables in the rest of the expression
    let result = expand_special_vars(&result);

    // Transform standalone args (handles :param, <R>, etc. outside calls)
    let result = transform_call_args(&result);

    // Convert char literals to byte literals unless already b'…' (Ruby uses
    // a lookbehind `(?<!b)'(\\.|.)'`; the optional captured `b` is the
    // lookbehind-free equivalent).
    let result = re(r"(b?)'(\\.|.)'")
        .replace_all(&result, |caps: &regex::Captures| {
            if &caps[1] == "b" {
                caps[0].to_string()
            } else {
                format!("b'{}'", &caps[2])
            }
        })
        .into_owned();

    // Escape-sequence aliases
    re(r"<[A-Z]+>")
        .replace_all(&result, |caps: &regex::Captures| {
            rust_escape_seq(&caps[0])
                .map(str::to_string)
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

/// Expand special variables: COL, LINE, PREV, :param (Ruby:
/// expand_special_vars).
fn expand_special_vars(s: &str) -> String {
    let s = re(r"\bCOL\b").replace_all(s, "self.col()");
    let s = re(r"\bLINE\b").replace_all(&s, "self.line as i32");
    let s = re(r"\bPREV\b").replace_all(&s, "self.prev()");
    re(r"(?i):([a-z_]\w*)").replace_all(&s, "$1").into_owned()
}

/// Transform function call arguments for Rust (Ruby: transform_call_args).
/// Splits on commas (Ruby semantics: trailing empties dropped), strips each
/// arg, maps escape aliases / :params / quotes / char literals / punctuation
/// to byte literals, and rejoins with ", ".
fn transform_call_args(args: &str) -> String {
    let mut parts: Vec<&str> = if args.is_empty() {
        vec![]
    } else {
        args.split(',').collect()
    };
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }

    parts
        .iter()
        .map(|raw| {
            let arg = raw.trim();
            if let Some(escaped) = rust_escape_seq(arg) {
                return escaped.to_string();
            }
            if let Some(caps) = re(r"^:(\w+)$").captures(arg) {
                return caps[1].to_string(); // :param -> param
            }
            match arg {
                "\"" => return "b'\"'".to_string(),  // Bare double quote
                "'" => return "b'\\''".to_string(), // Bare single quote
                _ => {}
            }
            if re(r"^\d+$").is_match(arg) || re(r"^-?\d+$").is_match(arg) {
                return arg.to_string(); // numeric literals
            }
            if let Some(caps) = re(r"^'(.)'$").captures(arg) {
                return format!("b'{}'", &caps[1]); // char literal
            }
            if let Some(caps) = re(r#"^"(.)"$"#).captures(arg) {
                return format!("b'{}'", &caps[1]); // quoted char
            }
            if re(r"^[!;:#*\-_<>/\\@$%^&+=?,.]$").is_match(arg) {
                return format!("b'{arg}'"); // single punctuation
            }
            arg.to_string() // pass through (variables, expressions)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Post-process (Ruby: Generator#generate tail + the regenerate driver)
// ---------------------------------------------------------------------------

/// Clean up whitespace from the rendered template — the exact regex chain
/// Ruby applies (order matters).
pub fn post_process(s: &str) -> String {
    // Remove whitespace-only lines
    let s = re(r"(?m)^[ \t]+$").replace_all(s, "");
    // Collapse all blank lines
    let s = re(r"\n{2,}").replace_all(&s, "\n");
    // Blank before use/pub/impl after a column-0 comment
    let s = re(r"(?m)^(//.*)\n(use |pub |impl )").replace_all(&s, "${1}\n\n${2}");
    // Blank after } before a new item
    let s = re(r"(?m)(\})\n([ \t]*(?://|#\[|pub |fn ))").replace_all(&s, "${1}\n\n${2}");
    // Driver-level: collapse 3+ newlines (no-op after the chain above; kept
    // for lockstep with udon's regenerate-parser invocation)
    re(r"\n{3,}").replace_all(&s, "\n\n").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascalcase_matches_ruby() {
        assert_eq!(pascalcase("identity"), "Identity");
        assert_eq!(pascalcase("after_name"), "AfterName");
        assert_eq!(pascalcase("UnclosedInterpolation"), "UnclosedInterpolation");
        assert_eq!(pascalcase("boolTrue"), "BoolTrue");
        assert_eq!(pascalcase("UnexpectedEOF"), "UnexpectedEof");
        assert_eq!(pascalcase(""), "");
    }

    #[test]
    fn rust_expr_calls_and_vars() {
        assert_eq!(
            rust_expr("/element(COL)"),
            "self.parse_element(self.col(), on_event)"
        );
        assert_eq!(rust_expr("/text"), "self.parse_text(on_event)");
        assert_eq!(rust_expr("COL - 1"), "self.col() - 1");
        assert_eq!(rust_expr("b == ' '"), "b == b' '");
        assert_eq!(rust_expr("b'x' == b'x'"), "b'x' == b'x'");
        assert_eq!(rust_expr("<R>"), "b']'");
        assert_eq!(rust_expr(":close"), "close");
    }

    #[test]
    fn transform_call_args_shapes() {
        assert_eq!(transform_call_args(":a, <R>, 5"), "a, b']', 5");
        assert_eq!(transform_call_args("'x'"), "b'x'");
        assert_eq!(transform_call_args(";"), "b';'");
        assert_eq!(transform_call_args(""), "");
    }
}
