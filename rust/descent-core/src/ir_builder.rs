//! Transforms AST into IR with semantic analysis. Port of Ruby descent's
//! `IRBuilder` (lib/descent/ir_builder.rb) with one deliberate divergence:
//! `transform_call_args_by_type` (which bakes Rust byte literals into IR call
//! args) is NOT here — it moved to `emit::rust` (the January-flaw fix). The
//! IR this builder produces is target-neutral; `emit::rust::build_context`
//! applies the Ruby-equivalent transform when building the template context,
//! so the context-JSON differential against Ruby still holds.
//!
//! Likewise `collect_prepend_values` stores *neutral* byte values (Ruby
//! pre-Rust-escapes `<BS>` to `\\`); the emitter re-escapes (see
//! improvements ledger in rust/PROGRESS.md).

use crate::ast;
use crate::charclass;
use crate::ir::*;
use crate::lexer::re;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fmt;

/// Equivalent of Ruby's Descent::ValidationError.
#[derive(Debug)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ValidationError {}

type Result<T> = std::result::Result<T, ValidationError>;

pub struct IRBuilder<'a> {
    ast: &'a ast::Machine,
}

impl<'a> IRBuilder<'a> {
    pub fn new(ast: &'a ast::Machine) -> Self {
        IRBuilder { ast }
    }

    pub fn build(&self) -> Result<ParserIR> {
        // |const substitution pre-pass: replace declared SCREAMING_CASE
        // names with their integer values in every expression position
        // (assignments, conditions, call args, return values) so all
        // downstream analysis — both backends, param typing, scan/expects
        // inference — just sees numbers.
        let substituted;
        let ast: &ast::Machine = if self.ast.consts.is_empty() {
            self.ast
        } else {
            substituted = substitute_consts(self.ast)?;
            &substituted
        };

        let types = build_types(&ast.types);
        let mut functions = ast
            .functions
            .iter()
            .map(|f| build_function(f, &types))
            .collect::<Result<Vec<_>>>()?;
        let keywords = ast.keywords.iter().map(build_keywords).collect();

        // Collect custom error codes from /error(code) calls
        let custom_error_codes = collect_custom_error_codes(&functions);

        // Collect prepend values by tracing call sites (also propagates
        // param types from call sites / callees, mutating param_types).
        collect_prepend_values(&mut functions);

        // NOTE: Ruby calls transform_call_args_by_type here; deliberately
        // omitted (see module docs) — emit::rust does it at context time.

        Ok(ParserIR {
            name: self.ast.name.clone(),
            entry_point: self.ast.entry_point.clone(),
            types,
            functions,
            keywords,
            custom_error_codes,
        })
    }
}

// ---------------------------------------------------------------------------
// |const substitution (pre-pass over the AST)
// ---------------------------------------------------------------------------

/// Clone the machine with every declared const name replaced by its integer
/// value in expression positions: assignment exprs, `|if[...]` conditions
/// (case- and clause-level), call args, and return values. Emit positions
/// (event type names) are deliberately untouched.
fn substitute_consts(m: &ast::Machine) -> Result<ast::Machine> {
    let mut seen = std::collections::HashSet::new();
    for c in &m.consts {
        if !seen.insert(c.name.as_str()) {
            return Err(ValidationError(format!(
                "L{}: duplicate const '{}'",
                c.lineno, c.name
            )));
        }
    }
    let consts: Vec<(regex::Regex, String)> = m
        .consts
        .iter()
        .map(|c| {
            (
                re(&format!(r"\b{}\b", regex::escape(&c.name))),
                c.value.to_string(),
            )
        })
        .collect();

    let sub = |s: &str| -> String {
        let mut out = s.to_string();
        for (rx, val) in &consts {
            out = rx.replace_all(&out, val.as_str()).into_owned();
        }
        out
    };

    let mut out = m.clone();
    for func in &mut out.functions {
        for cmd in &mut func.entry_actions {
            substitute_consts_in_command(cmd, &sub, &m.consts);
        }
        if let Some(h) = &mut func.eof_handler {
            for cmd in &mut h.commands {
                substitute_consts_in_command(cmd, &sub, &m.consts);
            }
        }
        for state in &mut func.states {
            for kase in &mut state.cases {
                if let Some(cond) = &kase.condition {
                    kase.condition = Some(sub(cond));
                }
                for cmd in &mut kase.commands {
                    substitute_consts_in_command(cmd, &sub, &m.consts);
                }
            }
            if let Some(h) = &mut state.eof_handler {
                for cmd in &mut h.commands {
                    substitute_consts_in_command(cmd, &sub, &m.consts);
                }
            }
        }
    }
    Ok(out)
}

fn substitute_consts_in_command(
    cmd: &mut ast::Command,
    sub: &dyn Fn(&str) -> String,
    consts: &[ast::ConstDecl],
) {
    match cmd {
        ast::Command::Conditional { clauses, .. } => {
            for clause in clauses {
                if let Some(cond) = &clause.condition {
                    clause.condition = Some(sub(cond));
                }
                for c in &mut clause.commands {
                    substitute_consts_in_command(c, sub, consts);
                }
            }
        }
        ast::Command::Cmd { kind, .. } => {
            use ast::CmdKind::*;
            match kind {
                Assign { expr, .. } | AddAssign { expr, .. } | SubAssign { expr, .. } => {
                    *expr = sub(expr);
                }
                Call(value) => *value = sub(value),
                Return(value) => {
                    // Whole-token only: `|return OPEN` -> `|return 1`.
                    // (A bare uppercase word in return position is otherwise
                    // an event-type emit — consts win when declared.)
                    if let Some(c) = consts.iter().find(|c| c.name == value.trim()) {
                        *value = c.value.to_string();
                    }
                }
                _ => {}
            }
        }
    }
}

fn build_keywords(kw: &ast::Keywords) -> Keywords {
    let mut fallback_func = None;
    let mut fallback_args = None;

    if let Some(fb) = &kw.fallback {
        if let Some(caps) = re(r"^/([0-9A-Za-z_]+)\(([^)]*)\)$").captures(fb) {
            fallback_func = Some(caps[1].to_string());
            fallback_args = Some(caps[2].to_string());
        } else if let Some(caps) = re(r"^/([0-9A-Za-z_]+)$").captures(fb) {
            fallback_func = Some(caps[1].to_string());
        }
    }

    Keywords {
        name: kw.name.clone(),
        fallback_func,
        fallback_args,
        mappings: kw.mappings.clone(),
        lineno: kw.lineno,
    }
}

fn build_types(type_decls: &[ast::TypeDecl]) -> Vec<TypeInfo> {
    type_decls
        .iter()
        .map(|t| {
            let is_bracket = t.kind == "BRACKET";
            TypeInfo {
                name: t.name.clone(),
                kind: t.kind.to_lowercase(),
                emits_start: is_bracket,
                emits_end: is_bracket,
                lineno: t.lineno,
            }
        })
        .collect()
}

fn build_function(func: &ast::Function, types: &[TypeInfo]) -> Result<Function> {
    let return_type_info = func
        .return_type
        .as_ref()
        .and_then(|rt| types.iter().find(|t| &t.name == rt));
    // Ruby: `info&.bracket? || info&.content?` — nil (not false) when the
    // return type is absent/unknown; serialized as JSON null.
    let emits_events = return_type_info.map(|t| t.is_bracket() || t.is_content());

    let locals = infer_locals(func);
    let states = func
        .states
        .iter()
        .map(|s| build_state(s, &func.params))
        .collect::<Result<Vec<_>>>()?;

    let (expects_char, emits_content_on_close) = infer_expects(&states);
    let param_types = infer_param_types(&func.params, &states);

    let eof_handler = match &func.eof_handler {
        Some(h) => {
            let cmds = h
                .commands
                .iter()
                .map(build_command)
                .collect::<Result<Vec<_>>>()?;
            Some(mark_returns_after_inline_emits(cmds))
        }
        None => None,
    };

    let entry_actions = func
        .entry_actions
        .iter()
        .map(build_command)
        .collect::<Result<Vec<_>>>()?;

    Ok(Function {
        name: func.name.clone(),
        return_type: func.return_type.clone(),
        params: func.params.clone(),
        param_types,
        locals,
        states,
        eof_handler,
        entry_actions,
        emits_events,
        expects_char,
        emits_content_on_close,
        prepend_values: vec![],
        lineno: func.lineno,
    })
}

fn build_state(state: &ast::State, params: &[String]) -> Result<State> {
    let cases = state
        .cases
        .iter()
        .map(|c| build_case(c, params))
        .collect::<Result<Vec<_>>>()?;
    let mut scan_chars = infer_scan_chars(&cases);
    let is_self_looping = cases
        .iter()
        .any(|c| c.is_default() && has_self_transition(c));

    let has_default = cases.iter().any(|c| c.is_default());

    // A state with an explicit |eof handler is never "unconditional": the
    // unconditional rendering executes its single case without consulting
    // EOF, which would silently drop the |eof commands (e.g. Unclosed*
    // errors emitted at end-of-input).
    let is_unconditional = state.eof_handler.is_none()
        && cases.first().is_some_and(|c| {
            c.chars.is_none() && c.special_class.is_none() && c.param_ref.is_none() && c.condition.is_none()
        });

    let eof_handler = match &state.eof_handler {
        Some(h) => {
            let cmds = h
                .commands
                .iter()
                .map(build_command)
                .collect::<Result<Vec<_>>>()?;
            Some(mark_returns_after_inline_emits(cmds))
        }
        None => None,
    };

    // Inject '\n' into scan_chars if not already a user target (and room).
    let mut newline_injected = false;
    if let Some(chars) = &mut scan_chars {
        if !chars.iter().any(|c| c == "\n") && chars.len() < 6 {
            chars.insert(0, "\n".to_string());
            newline_injected = true;
        }
    }

    Ok(State {
        name: state.name.clone(),
        cases,
        eof_handler,
        scan_chars,
        is_self_looping,
        has_default,
        is_unconditional,
        newline_injected,
        lineno: state.lineno,
    })
}

fn build_case(kase: &ast::Case, params: &[String]) -> Result<Case> {
    if let Some(chars) = &kase.chars {
        validate_char_syntax(chars, kase.lineno)?;
    }
    validate_prepend_commands(&kase.commands, params, kase.lineno)?;
    validate_call_args(&kase.commands, params, kase.lineno)?;
    let (chars, special_class, param_ref) = parse_chars(kase.chars.as_deref(), params);
    let commands = kase
        .commands
        .iter()
        .map(build_command)
        .collect::<Result<Vec<_>>>()?;

    // Fix #11: inline emit before a bare return suppresses auto-emit.
    let commands = mark_returns_after_inline_emits(commands);

    Ok(Case {
        chars,
        special_class,
        param_ref,
        condition: kase.condition.clone(),
        substate: kase.substate.clone(),
        commands,
        lineno: kase.lineno,
    })
}

/// Mark return commands that follow inline emits to suppress auto-emit.
fn mark_returns_after_inline_emits(commands: Vec<Command>) -> Vec<Command> {
    let mut has_inline_emit = false;

    commands
        .into_iter()
        .map(|cmd| {
            match cmd.ctype.as_str() {
                "inline_emit_bare" | "inline_emit_mark" | "inline_emit_literal" | "inline_emit_saved"
                | "inline_emit_param" => {
                    has_inline_emit = true;
                    cmd
                }
                "return" => {
                    let emit_type_nil = cmd.args.get("emit_type").is_none_or(|v| v.is_null());
                    let return_value_nil = cmd.args.get("return_value").is_none_or(|v| v.is_null());
                    if has_inline_emit && emit_type_nil && return_value_nil {
                        let mut args = cmd.args.as_object().cloned().unwrap_or_default();
                        args.insert("suppress_auto_emit".to_string(), json!(true));
                        Command { ctype: cmd.ctype, args: Value::Object(args), clauses: None }
                    } else {
                        cmd
                    }
                }
                _ => cmd,
            }
        })
        .collect()
}

fn build_command(cmd: &ast::Command) -> Result<Command> {
    let (kind, lineno) = match cmd {
        ast::Command::Conditional { clauses, .. } => {
            let clauses = clauses
                .iter()
                .map(|c| {
                    Ok(Clause {
                        condition: c.condition.clone(),
                        commands: c
                            .commands
                            .iter()
                            .map(build_command)
                            .collect::<Result<Vec<_>>>()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(Command::conditional(clauses));
        }
        ast::Command::Cmd { kind, lineno } => (kind, *lineno),
    };

    use ast::CmdKind::*;
    let (ctype, args): (&str, Value) = match kind {
        Assign { var, expr } => ("assign", json!({ "var": var, "expr": expr })),
        AddAssign { var, expr } => ("add_assign", json!({ "var": var, "expr": expr })),
        SubAssign { var, expr } => ("sub_assign", json!({ "var": var, "expr": expr })),
        AdvanceTo(v) => ("advance_to", json!({ "value": validate_advance_to(v, lineno)? })),
        Scan(v) => ("scan", json!({ "value": process_escapes(v) })),
        Emit(v) => ("emit", json!({ "value": v })),
        CallMethod(v) => ("call_method", json!({ "value": v })),
        Transition(v) => ("transition", json!({ "value": v })),
        Error(v) => ("error", json!({ "value": v })),
        Call(v) => ("call", parse_call_value(v)),
        InlineEmitBare(v) => ("inline_emit_bare", json!({ "type": v })),
        InlineEmitMark(v) => ("inline_emit_mark", json!({ "type": v })),
        Save(v) => ("save", json!({ "slot": v })),
        KeywordsTry { var, name } => ("keywords_try", json!({ "var": var, "name": name })),
        InlineEmitSaved { ty, slot } => {
            ("inline_emit_saved", json!({ "type": ty, "slot": slot }))
        }
        InlineEmitLiteral { ty, literal } => {
            ("inline_emit_literal", json!({ "type": ty, "literal": literal }))
        }
        InlineEmitParam { ty, param } => {
            ("inline_emit_param", json!({ "type": ty, "param_ref": param }))
        }
        Term(v) => ("term", json!({ "offset": v.unwrap_or(0) })),
        Prepend(v) => ("prepend", json!({ "literal": process_escapes(v) })),
        PrependParam(v) => ("prepend_param", json!({ "param_ref": v })),
        KeywordsLookup(v) => ("keywords_lookup", json!({ "name": v })),
        Return(v) => ("return", parse_return_value(v)),
        Advance => ("advance", json!({})),
        Mark => ("mark", json!({})),
        Noop => ("noop", json!({})),
    };

    Ok(Command::new(ctype, args))
}

/// Process character class/literal to get the actual bytes.
fn process_escapes(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    charclass::parse(s).bytes.unwrap_or_default()
}

/// Validate and process advance_to (`->[...]`) arguments.
fn validate_advance_to(s: &str, lineno: usize) -> Result<String> {
    if s.is_empty() {
        return Err(ValidationError(format!(
            "L{lineno}: ->[] requires at least one character"
        )));
    }

    let result = charclass::parse(s);

    if result.special_class.is_some() {
        return Err(ValidationError(format!(
            "L{}: ->[] does not support character classes like {}. \
             Only literal bytes are supported (uses SIMD memchr).",
            lineno,
            s.to_uppercase()
        )));
    }

    if let Some(p) = &result.param_ref {
        return Err(ValidationError(format!(
            "L{lineno}: ->[] does not support parameter references like :{p}. \
             Only literal bytes are supported (uses SIMD memchr)."
        )));
    }

    let bytes = result.bytes.unwrap_or_default();
    if bytes.is_empty() {
        return Err(ValidationError(format!(
            "L{lineno}: ->[] resolved to empty bytes from '{s}'"
        )));
    }

    let n = bytes.chars().count();
    if n > 6 {
        return Err(ValidationError(format!(
            "L{lineno}: ->[{s}] has {n} chars but maximum is 6 \
             (chained memchr limit). Split into multiple scans or restructure grammar."
        )));
    }

    Ok(bytes)
}

/// Validate character syntax in `c[...]` before parsing.
fn validate_char_syntax(chars_str: &str, lineno: usize) -> Result<()> {
    if chars_str.is_empty() {
        return Ok(());
    }

    // Already using proper class syntax - <...> wrapper
    if chars_str.starts_with('<') && chars_str.ends_with('>') {
        return Ok(());
    }

    // Properly quoted string
    let chars: Vec<char> = chars_str.chars().collect();
    if chars_str.starts_with('\'') && chars_str.ends_with('\'') && chars.len() >= 2 {
        return Ok(());
    }

    // Parameter reference
    if re(r"(?i)^:[a-z_][0-9a-z_]*$").is_match(chars_str) {
        return Ok(());
    }

    // Pure special class (SCREAMING_CASE)
    if re(r"^[A-Z]+(?:_[A-Z]+)*$").is_match(chars_str) {
        return Ok(());
    }

    // <TOKEN> escape sequences outside a proper <...> class wrapper
    if re(r"<[A-Z]+>").is_match(chars_str) {
        return Err(ValidationError(format!(
            "Line {lineno}: Escape sequence like <SQ>, <P> etc. found outside \
             class wrapper in c[{chars_str}]. \
             Wrap everything in a class: c[<...>] not c[THING <ESC> ...]"
        )));
    }

    // Combined class + chars (e.g., LETTER'[.?!)
    if re(r"^[A-Z]+(?:_[A-Z]+)*'").is_match(chars_str) {
        let class_name = re(r"^([A-Z_]+)")
            .captures(chars_str)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        return Err(ValidationError(format!(
            "Line {lineno}: Invalid character syntax in c[{chars_str}]. \
             Bare quote after class name is ambiguous. \
             Use class syntax instead: c[<{class_name} ...>]"
        )));
    }

    // Unterminated quotes
    let quote_count = chars.iter().filter(|&&c| c == '\'').count();
    if quote_count % 2 == 1 {
        return Err(ValidationError(format!(
            "Line {lineno}: Unterminated quote in c[{chars_str}]. \
             Single quotes must be paired. \
             To match a literal quote, use c[<SQ>] or c['\\'']"
        )));
    }

    // Any character outside /A-Za-z0-9_-/ that isn't quoted
    for (i, ch) in chars.iter().enumerate() {
        let ch = *ch;
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            continue;
        }
        if ch == '\'' || ch == '\\' {
            continue;
        }

        let quote_depth = chars[0..i].iter().filter(|&&c| c == '\'').count();
        if quote_depth % 2 == 1 {
            continue; // Inside quotes, OK
        }

        let suggestion = match ch {
            '|' => "c[<P>] or c['|']".to_string(),
            '[' => "c[<L>] or c['[']".to_string(),
            ']' => "c[<R>] or c[']']".to_string(),
            '{' => "c[<LB>] or c['{']".to_string(),
            '}' => "c[<RB>] or c['}']".to_string(),
            '(' => "c[<LP>] or c['(']".to_string(),
            ')' => "c[<RP>] or c[')']".to_string(),
            '"' => "c[<DQ>] or c['\"']".to_string(),
            ' ' => "c[<WS>] or c[' ']".to_string(),
            '\t' => "c['\\t']".to_string(),
            '\n' => "c['\\n']".to_string(),
            other => format!("c['{other}']"),
        };

        return Err(ValidationError(format!(
            "Line {}: Unquoted '{}' in c[{}]. \
             Characters outside /A-Za-z0-9_-/ must be quoted. Use {}",
            lineno,
            ch.escape_debug(),
            chars_str,
            suggestion
        )));
    }

    Ok(())
}

/// Validate PREPEND commands: PREPEND(param) should be PREPEND(:param).
fn validate_prepend_commands(commands: &[ast::Command], params: &[String], lineno: usize) -> Result<()> {
    if params.is_empty() {
        return Ok(());
    }

    for cmd in commands {
        let ast::Command::Cmd { kind: ast::CmdKind::Prepend(value), .. } = cmd else {
            continue;
        };
        let literal = value.trim();
        if !re(r"(?i)^[a-z_][0-9a-z_]*$").is_match(literal) {
            continue;
        }
        if !params.iter().any(|p| p == literal) {
            continue;
        }
        return Err(ValidationError(format!(
            "Line {lineno}: PREPEND({literal}) looks like a parameter reference. \
             Use PREPEND(:{literal}) to reference the '{literal}' parameter, \
             or PREPEND('{literal}') for a literal string."
        )));
    }

    Ok(())
}

/// Validate call args: /func(param) where param names a parameter should be :param.
fn validate_call_args(commands: &[ast::Command], params: &[String], lineno: usize) -> Result<()> {
    if params.is_empty() {
        return Ok(());
    }

    for cmd in commands {
        let ast::Command::Cmd { kind: ast::CmdKind::Call(value), .. } = cmd else {
            continue;
        };
        if !value.contains('(') {
            continue;
        }

        // Ruby: call_str[/\((.+)\)/, 1] — greedy, first '(' to last ')'
        let Some(caps) = re(r"\((.+)\)").captures(value) else {
            continue;
        };
        let args_str = &caps[1];
        if args_str.is_empty() {
            continue;
        }

        for arg in tokenize_call_args(args_str) {
            let arg = arg.trim();
            if arg.starts_with(':') || arg.starts_with('\'') || arg.starts_with('"') || arg.starts_with('<') {
                continue;
            }
            if re(r"^-?\d+$").is_match(arg) {
                continue;
            }
            if re(r"^[A-Z]+$").is_match(arg) {
                continue; // COL, LINE, PREV - built-in vars
            }
            if arg.contains(' ') || arg.contains('.') || arg.contains('(') {
                continue;
            }
            if !re(r"(?i)^[a-z_][0-9a-z_]*$").is_match(arg) {
                continue;
            }
            if !params.iter().any(|p| p == arg) {
                continue;
            }
            return Err(ValidationError(format!(
                "Line {lineno}: /...(...{arg}...) - bare identifier '{arg}' matches a parameter name. \
                 Use ':{arg}' to pass the parameter value, or \"'{arg}'\" for a literal string."
            )));
        }
    }

    Ok(())
}

/// Tokenize call arguments respecting quotes and angle brackets (commas
/// inside quotes/angles don't split). Shared by validation, the prepend
/// tracer, and `emit::rust`'s call-arg transform (Ruby has two identical
/// copies: tokenize_call_args_for_validation / tokenize_call_args).
pub fn tokenize_call_args(args_str: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![];
    let mut current = String::new();
    let mut in_quote = false;
    let mut in_angle: i32 = 0;

    for c in args_str.chars() {
        match c {
            '\'' => {
                in_quote = !in_quote;
                current.push(c);
            }
            '<' => {
                in_angle += 1;
                current.push(c);
            }
            '>' => {
                if in_angle > 0 {
                    in_angle -= 1;
                }
                current.push(c);
            }
            ',' => {
                if in_quote || in_angle > 0 {
                    current.push(c);
                } else {
                    args.push(current.trim().to_string());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }

    if !current.is_empty() {
        args.push(current.trim().to_string());
    }
    args
}

/// Parse character specification for c[...] using the unified parser.
/// Returns (chars, special_class, param_ref).
fn parse_chars(
    chars_str: Option<&str>,
    params: &[String],
) -> (Option<Vec<String>>, Option<String>, Option<String>) {
    let Some(chars_str) = chars_str else {
        return (None, None, None);
    };

    let result = charclass::parse(chars_str);

    // Unknown param - treat the whole thing as literal chars
    if let Some(p) = &result.param_ref {
        if !params.iter().any(|x| x == p) {
            let chars: Vec<String> = format!(":{p}").chars().map(|c| c.to_string()).collect();
            return (Some(chars), None, None);
        }
    }

    let chars = if result.chars.is_empty() { None } else { Some(result.chars) };
    (chars, result.special_class, result.param_ref)
}

/// Parse return value specification. Mirrors Ruby parse_return_value; the
/// emit_mode symbol becomes a JSON string ("mark"/"literal"/"bare").
fn parse_return_value(value: &str) -> Value {
    if value.is_empty() {
        return json!({});
    }

    if let Some(caps) = re(r"^([A-Z][0-9A-Za-z_]*)\(USE_MARK\)$").captures(value) {
        return json!({ "emit_type": &caps[1], "emit_mode": "mark" });
    }
    if let Some(caps) = re(r"^([A-Z][0-9A-Za-z_]*)\(([^)]+)\)$").captures(value) {
        return json!({
            "emit_type": &caps[1],
            "emit_mode": "literal",
            "literal": process_escapes(&caps[2]),
        });
    }
    if re(r"^[A-Z][0-9A-Za-z_]*$").is_match(value) {
        return json!({ "emit_type": value, "emit_mode": "bare" });
    }
    if re(r"^[a-z_][0-9A-Za-z_]*$").is_match(value) {
        // Variable name - for INTERNAL types returning a computed value
        return json!({ "return_value": value });
    }
    if re(r"^-?\d+$").is_match(value) {
        // Integer literal (typically a substituted |const name)
        return json!({ "return_value": value });
    }

    json!({}) // Unknown format, use default
}

/// Parse a call command value into name and args.
fn parse_call_value(value: &str) -> Value {
    let Some(paren_pos) = value.find('(') else {
        return json!({ "name": value, "call_args": null });
    };

    let name = &value[..paren_pos];
    let rest = &value[paren_pos + 1..];

    // Strip only ONE trailing ')' if present (supports "func())" -> args ")")
    let call_args = rest.strip_suffix(')').unwrap_or(rest);
    let call_args_val: Value = if call_args.is_empty() { Value::Null } else { json!(call_args) };

    let mut obj = Map::new();
    obj.insert("name".to_string(), json!(name));
    obj.insert("call_args".to_string(), call_args_val);
    if name == "error" {
        obj.insert("is_error".to_string(), json!(true));
    }
    Value::Object(obj)
}

/// Infer SCAN optimization chars from a simple self-looping default case.
fn infer_scan_chars(cases: &[Case]) -> Option<Vec<String>> {
    let default_case = cases.iter().find(|c| c.is_default())?;
    if !simple_self_loop(default_case) {
        return None;
    }

    let non_default: Vec<&Case> = cases
        .iter()
        .filter(|c| !c.is_default() && !c.is_conditional())
        .collect();

    // SCAN can only target statically-known bytes. If any case matches a
    // parameter (|c[:param]|) or a character class (LETTER, XLBL_CONT, ...),
    // a memchr scan for the static chars would skip right past positions
    // those cases must inspect — the state is not scannable at all.
    // (Found via udon typed_value:string, where the `c[:bracket]` case was
    // silently skipped and `[a.md]` swallowed the closing `]`.)
    if non_default
        .iter()
        .any(|c| c.param_ref.is_some() || c.special_class.is_some())
    {
        return None;
    }

    let mut explicit_chars: Vec<String> = vec![];
    for kase in non_default {
        if let Some(chars) = &kase.chars {
            for c in chars {
                if !explicit_chars.contains(c) {
                    explicit_chars.push(c.clone());
                }
            }
        }
    }

    if explicit_chars.is_empty() || explicit_chars.len() > 6 {
        return None;
    }

    Some(explicit_chars)
}

/// A simple self-loop has only advance and/or (empty-target) transition commands,
/// with at least one self-transition.
fn simple_self_loop(kase: &Case) -> bool {
    let mut has_self_transition = false;

    for cmd in &kase.commands {
        match cmd.ctype.as_str() {
            "advance" => {}
            "transition" => {
                let val = cmd.arg_str("value");
                if val.is_none() || val == Some("") {
                    has_self_transition = true;
                }
            }
            _ => return false,
        }
    }

    has_self_transition
}

/// Any self-transition (for is_self_looping metadata).
fn has_self_transition(kase: &Case) -> bool {
    kase.commands.iter().any(|cmd| {
        if cmd.ctype != "transition" {
            return false;
        }
        let val = cmd.arg_str("value");
        val.is_none() || val == Some("")
    })
}

/// Infer expected closing delimiter from return cases.
fn infer_expects(states: &[State]) -> (Option<String>, bool) {
    let mut return_cases: Vec<&Case> = vec![];

    for state in states {
        for kase in &state.cases {
            if kase.commands.iter().any(|c| c.ctype == "return") {
                return_cases.push(kase);
            }
        }
    }

    if return_cases.is_empty() {
        return (None, false);
    }

    let char_matches: Vec<&String> = return_cases
        .iter()
        .filter_map(|kase| {
            if kase.is_default() || kase.special_class.is_some() {
                return None;
            }
            match &kase.chars {
                Some(chars) if chars.len() == 1 => Some(&chars[0]),
                _ => None,
            }
        })
        .collect();

    if char_matches.len() != return_cases.len() {
        return (None, false);
    }
    if !char_matches.windows(2).all(|w| w[0] == w[1]) {
        return (None, false);
    }

    let expects_char = char_matches[0].clone();

    let emits_content = return_cases
        .iter()
        .any(|kase| kase.commands.iter().any(|c| c.ctype == "term"));

    (Some(expects_char), emits_content)
}

/// Collect custom error codes from /error(code) calls (case commands only,
/// recursing into conditional clauses — mirrors Ruby's traversal exactly).
fn collect_custom_error_codes(functions: &[Function]) -> Vec<String> {
    let mut codes: BTreeSet<String> = BTreeSet::new();

    for func in functions {
        if let Some(cmds) = &func.eof_handler {
            collect_error_codes_from_commands(cmds, &mut codes);
        }
        for state in &func.states {
            // |eof handlers can carry /error(code) too (e.g. Unclosed*)
            if let Some(cmds) = &state.eof_handler {
                collect_error_codes_from_commands(cmds, &mut codes);
            }
            for kase in &state.cases {
                collect_error_codes_from_commands(&kase.commands, &mut codes);
            }
        }
    }

    codes.into_iter().collect()
}

fn collect_error_codes_from_commands(commands: &[Command], codes: &mut BTreeSet<String>) {
    for cmd in commands {
        match cmd.ctype.as_str() {
            "error" => {
                if let Some(code) = cmd.arg_str("value") {
                    if !code.is_empty() {
                        codes.insert(code.to_string());
                    }
                }
            }
            "call" => {
                // /error(code) is parsed as :call with is_error: true
                if cmd.args.get("is_error").and_then(|v| v.as_bool()) == Some(true) {
                    if let Some(code) = cmd.arg_str("call_args") {
                        if !code.is_empty() {
                            codes.insert(code.to_string());
                        }
                    }
                }
            }
            "conditional" => {
                if let Some(clauses) = &cmd.clauses {
                    for clause in clauses {
                        collect_error_codes_from_commands(&clause.commands, codes);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Infer local variables from assignments (entry actions + case commands,
/// recursing conditionals). Operates on the AST, as in Ruby.
fn infer_locals(func: &ast::Function) -> Vec<String> {
    let mut locals: Vec<String> = vec![];

    for cmd in &func.entry_actions {
        collect_locals_from_commands(std::slice::from_ref(cmd), &mut locals);
    }

    for state in &func.states {
        for kase in &state.cases {
            collect_locals_from_commands(&kase.commands, &mut locals);
        }
    }

    locals
}

fn collect_locals_from_commands(commands: &[ast::Command], locals: &mut Vec<String>) {
    for cmd in commands {
        match cmd {
            ast::Command::Conditional { clauses, .. } => {
                for clause in clauses {
                    collect_locals_from_commands(&clause.commands, locals);
                }
            }
            ast::Command::Cmd { kind, .. } => match kind {
                ast::CmdKind::Assign { var, .. }
                | ast::CmdKind::AddAssign { var, .. }
                | ast::CmdKind::SubAssign { var, .. } => {
                    if !locals.contains(var) {
                        locals.push(var.clone());
                    }
                }
                _ => {}
            },
        }
    }
}

/// Infer parameter types from usage in states.
fn infer_param_types(params: &[String], states: &[State]) -> Vec<(String, ParamType)> {
    if params.is_empty() {
        return vec![];
    }

    let mut types: Vec<(String, ParamType)> =
        params.iter().map(|p| (p.clone(), ParamType::I32)).collect();

    let set = |types: &mut Vec<(String, ParamType)>, name: &str, t: ParamType| {
        if let Some(entry) = types.iter_mut().find(|(p, _)| p == name) {
            entry.1 = t;
        }
    };

    for state in states {
        for kase in &state.cases {
            // Param used in character match - needs u8 comparison
            if let Some(p) = &kase.param_ref {
                set(&mut types, p, ParamType::Byte);
            }

            // Conditions with param == 'char' comparisons
            if let Some(cond) = &kase.condition {
                for param in params {
                    let esc = regex::escape(param);
                    let hit = re(&format!(r"\b{esc}\s*[!=]=\s*'")).is_match(cond)
                        || re(&format!(r"'\s*[!=]=\s*{esc}\b")).is_match(cond);
                    if hit {
                        set(&mut types, param, ParamType::Byte);
                    }
                }
            }

            // PREPEND(:param) / TypeName(:param) - needs bytes slice
            for cmd in &kase.commands {
                if cmd.ctype == "prepend_param" || cmd.ctype == "inline_emit_param" {
                    if let Some(p) = cmd.arg_str("param_ref") {
                        set(&mut types, p, ParamType::Bytes);
                    }
                }
            }
        }
    }

    types
}

/// A call site: caller function index, callee name, raw args (comma-split,
/// stripped — Ruby uses a plain `split(',')` here, not the quote-aware
/// tokenizer). Extracted from top-level case commands only (no eof handlers,
/// no conditional recursion), matching Ruby's traversal.
struct CallSite {
    caller: usize,
    callee: String,
    args: Vec<String>,
}

fn plain_call_sites(functions: &[Function]) -> Vec<CallSite> {
    let mut sites = vec![];
    for (fi, func) in functions.iter().enumerate() {
        for state in &func.states {
            for kase in &state.cases {
                for cmd in &kase.commands {
                    if cmd.ctype == "call" {
                        let Some(call_args) = cmd.arg_str("call_args") else {
                            continue;
                        };
                        let Some(name) = cmd.arg_str("name") else {
                            continue;
                        };
                        sites.push(CallSite {
                            caller: fi,
                            callee: name.to_string(),
                            args: call_args.split(',').map(|a| a.trim().to_string()).collect(),
                        });
                    } else if cmd.ctype == "assign" {
                        // `x = /fn(args)` — an assignment-from-call is a call
                        // site too; without this, param types (byte/bytes)
                        // fail to propagate through captured calls.
                        let Some(expr) = cmd.arg_str("expr") else {
                            continue;
                        };
                        if let Some(c) = re(r"^/(\w+)\((.*)\)\s*$").captures(expr.trim()) {
                            sites.push(CallSite {
                                caller: fi,
                                callee: c[1].to_string(),
                                args: c[2].split(',').map(|a| a.trim().to_string()).collect(),
                            });
                        }
                    }
                }
            }
        }
    }
    sites
}

/// Check if a value MUST be a byte slice: only the empty class `<>`.
fn bytes_like_value(arg: &str) -> bool {
    arg == "<>"
}

/// Infer param types from call-site values AND propagate from callees.
fn propagate_param_types(functions: &mut [Function]) {
    let index_by_name: std::collections::HashMap<String, usize> = functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.clone(), i))
        .collect();
    let sites = plain_call_sites(functions);

    // First pass: infer :bytes from literal `<>` values at call sites
    for site in &sites {
        let Some(&ti) = index_by_name.get(&site.callee) else {
            continue;
        };
        let target_params = functions[ti].params.clone();
        for (arg, target_param) in site.args.iter().zip(target_params.iter()) {
            if bytes_like_value(arg) && functions[ti].param_type(target_param) == Some(ParamType::I32) {
                set_param_type(&mut functions[ti], target_param, ParamType::Bytes);
            }
        }
    }

    // Second pass: propagate types from callees to callers (iterative)
    let mut changed = true;
    while changed {
        changed = false;
        for site in &sites {
            let Some(&ti) = index_by_name.get(&site.callee) else {
                continue;
            };
            let target_params = functions[ti].params.clone();
            for (arg, target_param) in site.args.iter().zip(target_params.iter()) {
                let Some(caps) = re(r"^:([0-9A-Za-z_]+)$").captures(arg) else {
                    continue;
                };
                let our_param = caps[1].to_string();
                let Some(our_type) = functions[site.caller].param_type(&our_param) else {
                    continue;
                };
                let Some(target_type) = functions[ti].param_type(target_param) else {
                    continue;
                };

                if target_type == ParamType::Bytes && our_type != ParamType::Bytes {
                    set_param_type(&mut functions[site.caller], &our_param, ParamType::Bytes);
                    changed = true;
                } else if target_type == ParamType::Byte && our_type == ParamType::I32 {
                    set_param_type(&mut functions[site.caller], &our_param, ParamType::Byte);
                    changed = true;
                }
            }
        }
    }
}

fn set_param_type(func: &mut Function, param: &str, t: ParamType) {
    if let Some(entry) = func.param_types.iter_mut().find(|(p, _)| p == param) {
        entry.1 = t;
    }
}

/// Collect prepend values by tracing call sites to functions with
/// PREPEND(:param). Values are stored **neutral** (unescaped bytes);
/// `emit::rust` renders them (Ruby pre-escapes `<BS>` here — ledger item).
fn collect_prepend_values(functions: &mut Vec<Function>) {
    propagate_param_types(functions);

    // Step 1: which functions have PREPEND(:param), and which param
    // (top-level case commands only, mirroring Ruby)
    let mut prepend_params: Vec<(String, String)> = vec![]; // (func, param), last wins
    for func in functions.iter() {
        for state in &func.states {
            for kase in &state.cases {
                for cmd in &kase.commands {
                    if cmd.ctype == "prepend_param" {
                        if let Some(p) = cmd.arg_str("param_ref") {
                            if let Some(e) = prepend_params.iter_mut().find(|(f, _)| f == &func.name) {
                                e.1 = p.to_string();
                            } else {
                                prepend_params.push((func.name.clone(), p.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }

    if prepend_params.is_empty() {
        return;
    }

    // Step 2: all call sites (case commands AND state eof handlers, with
    // conditional recursion) — collect byte values passed
    let mut prepend_values: std::collections::HashMap<String, BTreeSet<String>> =
        std::collections::HashMap::new();

    for func in functions.iter() {
        for state in &func.states {
            for kase in &state.cases {
                collect_call_values_from_commands(&kase.commands, &prepend_params, &mut prepend_values);
            }
            if let Some(eof) = &state.eof_handler {
                collect_call_values_from_commands(eof, &prepend_params, &mut prepend_values);
            }
        }
    }

    // Step 3: attach to functions
    for func in functions.iter_mut() {
        if let Some((_, param_name)) = prepend_params.iter().find(|(f, _)| f == &func.name) {
            let values: Vec<String> = prepend_values
                .get(&func.name)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default();
            func.prepend_values = vec![(param_name.clone(), values)];
        }
    }
}

fn collect_call_values_from_commands(
    commands: &[Command],
    prepend_params: &[(String, String)],
    prepend_values: &mut std::collections::HashMap<String, BTreeSet<String>>,
) {
    for cmd in commands {
        match cmd.ctype.as_str() {
            "call" => {
                let Some(func_name) = cmd.arg_str("name") else {
                    continue;
                };
                if !prepend_params.iter().any(|(f, _)| f == func_name) {
                    continue;
                }
                if let Some(byte_value) = parse_byte_literal(cmd.arg_str("call_args")) {
                    prepend_values
                        .entry(func_name.to_string())
                        .or_default()
                        .insert(byte_value);
                }
            }
            "conditional" => {
                if let Some(clauses) = &cmd.clauses {
                    for clause in clauses {
                        collect_call_values_from_commands(&clause.commands, prepend_params, prepend_values);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Parse a call argument into a byte value. NEUTRAL divergence from Ruby:
/// `<BS>` yields a single backslash (Ruby stores the pre-Rust-escaped `\\`);
/// `emit::rust` re-escapes when building the context.
fn parse_byte_literal(arg: Option<&str>) -> Option<String> {
    let arg = arg?;
    if arg.is_empty() {
        return None;
    }

    match arg {
        "0" => None, // 0 means no prepend
        "<P>" => Some("|".to_string()),
        "<L>" => Some("[".to_string()),
        "<R>" => Some("]".to_string()),
        "<LB>" => Some("{".to_string()),
        "<RB>" => Some("}".to_string()),
        "<LP>" => Some("(".to_string()),
        "<RP>" => Some(")".to_string()),
        "<BS>" => Some("\\".to_string()),
        _ => {
            if let Some(caps) = re(r"^'(.)'$").captures(arg) {
                return Some(caps[1].to_string());
            }
            if let Some(caps) = re("^\"(.)\"$").captures(arg) {
                return Some(caps[1].to_string());
            }
            if let Some(caps) = re(r"^'\\(.)'$").captures(arg) {
                return Some(charclass::parse_quoted_string(&format!("\\{}", &caps[1])));
            }
            if re(r"^.$").is_match(arg) {
                return Some(arg.to_string());
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn const_substitution_reaches_all_expression_positions() {
        let desc = r#"
|parser const_test
|type[Text] CONTENT
|type[INT]  INTERNAL
|const[FIN] 0
|const[OPEN] 1
|entry-point /document

|function[document] | m = OPEN
  |state[:main]
    |c['\n']    | ->                    |>>
    |default    | st = /line(OPEN)      |>> :route
  |state[:route]
    |if[st == FIN]  | m = FIN           |>> :main
    |default        |                   |>> :main

|function[line:INT] :mode
  |state[:main]
    |c['\n']    | -> | Text(USE_MARK)   |return FIN
    |default    | MARK | ->             |>>
"#;
        let ir = crate::build_ir_with(desc, "test.desc", crate::Frontend::OracleLexer).unwrap();
        let doc = &ir.functions[0];
        // entry action / initializer
        assert_eq!(doc.entry_actions[0].arg_str("expr"), Some("1"));
        // call arg
        let call = &doc.states[0].cases[1].commands[0];
        assert_eq!(call.ctype, "assign");
        assert_eq!(call.arg_str("expr"), Some("/line(1)"));
        // condition
        assert_eq!(doc.states[1].cases[0].condition.as_deref(), Some("st == 0"));
        // assignment inside the guarded case
        assert_eq!(doc.states[1].cases[0].commands[0].arg_str("expr"), Some("0"));
        // |return CONST -> numeric return_value
        let line = &ir.functions[1];
        let ret = line.states[0].cases[0]
            .commands
            .iter()
            .find(|c| c.ctype == "return")
            .unwrap();
        assert_eq!(ret.arg_str("return_value"), Some("0"));
    }

    #[test]
    fn inline_emit_param_infers_bytes_and_suppresses_auto_emit() {
        let desc = r#"
|parser p
|type[Attr] CONTENT
|type[INT]  INTERNAL
|entry-point /doc
|function[doc]
  |state[:m]
    |c['?'] | -> | s = /sfx('$?', '?') |>>
    |default | ->                      |>>
|function[sfx:INT] :tag :ch
  |state[:m]
    |c[' ']  | Attr(:tag) | ->         |return
    |default | PREPEND(:ch)            |return
"#;
        let ir = crate::build_ir_with(desc, "t.desc", crate::Frontend::OracleLexer).unwrap();
        let sfx = ir.functions.iter().find(|f| f.name == "sfx").unwrap();
        assert_eq!(sfx.param_type("tag"), Some(crate::ir::ParamType::Bytes));
        assert_eq!(sfx.param_type("ch"), Some(crate::ir::ParamType::Bytes));
        let emit = &sfx.states[0].cases[0].commands[0];
        assert_eq!(emit.ctype, "inline_emit_param");
        assert_eq!(emit.arg_str("type"), Some("Attr"));
        assert_eq!(emit.arg_str("param_ref"), Some("tag"));
        // Both backends must render the assignment-call's args by target
        // type (b"$?", not the invalid char literal '$?').
        let rec = crate::emit::rust::generate(&ir, &Default::default()).unwrap();
        assert!(rec.contains("self.parse_sfx(b\"$?\", b\"?\", on_event)"), "recursive assign-call args");
        assert!(rec.contains("std::borrow::Cow::Borrowed(tag)"), "recursive emit param payload");
        let pd = crate::emit::rust_pushdown::generate(&ir, &Default::default());
        assert!(pd.contains("f.tag.to_vec()"), "pushdown emit param payload");
    }

    #[test]
    fn duplicate_const_rejected() {
        let desc = "|parser p\n|const[X] 1\n|const[X] 2\n|entry-point /f\n|function[f]\n  |state[:m]\n    |default | -> |>>\n";
        assert!(crate::build_ir_with(desc, "t.desc", crate::Frontend::OracleLexer).is_err());
    }
}
