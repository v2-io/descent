//! Rust-target emitter: builds the template context from the neutral IR.
//! Port of Ruby descent's `Generator#build_context` (lib/descent/generator.rb)
//! PLUS `IRBuilder#transform_call_args_by_type` (lib/descent/ir_builder.rb),
//! which Ruby runs earlier (baking Rust literals into its IR) and we run
//! here at context time — the context JSON is identical either way, which is
//! the differential checkpoint (`descent-rs context` vs
//! `rust/tools/dump_context.rb`).
//!
//! Faithfully reproduced Ruby quirks (see PROGRESS.md improvements ledger):
//! - call args are transformed only inside states (cases + state eof
//!   handlers); function-level eof handlers and entry actions keep raw args.
//! - `analyze_helper_usage` misses COL/PREV inside conditional-clause
//!   *conditions* (checks commands + case conditions only).
//! - `extract_local_init_values` uses a mini expr transpiler
//!   (COL/LINE/PREV/:param only — no escapes/char-literals).
//! - prepend_values render with Ruby's pre-escaping (`\` -> `\\`).

pub mod engine;
pub mod literals;

use crate::charclass;
use crate::ir::*;
use crate::ir_builder::tokenize_call_args;
use crate::lexer::re;
use serde_json::{json, Map, Value};

/// Unicode character classes that require the unicode-xid crate.
const UNICODE_CLASSES: &[&str] = &["xid_start", "xid_cont", "xlbl_start", "xlbl_cont"];

#[derive(Debug, Clone)]
pub struct Options {
    pub trace: bool,
    pub streaming: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { trace: false, streaming: true }
    }
}

/// Generate Rust parser source from the IR (Ruby: Generator#generate):
/// build the context, render the minijinja templates, post-process.
pub fn generate(ir: &ParserIR, opts: &Options) -> Result<String, minijinja::Error> {
    let ctx = build_context(ir, opts);
    let env = engine::make_env()?;
    let rendered = env.get_template("parser.j2")?.render(&ctx)?;
    Ok(engine::post_process(&rendered))
}

/// Build the full template context (Ruby: Generator#build_context).
pub fn build_context(ir: &ParserIR, opts: &Options) -> Value {
    let functions_data: Vec<Value> = ir.functions.iter().map(|f| function_to_value(f, ir)).collect();
    let usage = analyze_helper_usage(&functions_data);

    let saved_slots = collect_saved_slots(&functions_data);

    json!({
        "parser": ir.name,
        "entry_point": ir.entry_point,
        "saved_slots": saved_slots,
        "types": ir.types.iter().map(type_to_value).collect::<Vec<_>>(),
        "functions": functions_data,
        "keywords": ir.keywords.iter().map(keywords_to_value).collect::<Vec<_>>(),
        "custom_error_codes": ir.custom_error_codes,
        "trace": opts.trace,
        "uses_unicode": uses_unicode_classes(&functions_data),
        "uses_col": usage.col,
        "uses_prev": usage.prev,
        "uses_set_term": usage.set_term,
        "uses_span": usage.span,
        "uses_letter": usage.letter,
        "uses_label_cont": usage.label_cont,
        "uses_digit": usage.digit,
        "uses_hex_digit": usage.hex_digit,
        "uses_ws": usage.ws,
        "uses_nl": usage.nl,
        "max_scan_arity": usage.max_scan_arity,
        "streaming": opts.streaming,
    })
}

fn type_to_value(t: &TypeInfo) -> Value {
    json!({
        "name": t.name,
        "kind": t.kind,
        "emits_start": t.emits_start,
        "emits_end": t.emits_end,
    })
}

fn keywords_to_value(kw: &Keywords) -> Value {
    json!({
        "name": kw.name,
        "const_name": format!("{}_KEYWORDS", kw.name.to_uppercase()),
        "fallback_func": kw.fallback_func,
        "fallback_args": kw.fallback_args,
        "mappings": kw.mappings.iter().map(|m| json!({
            "keyword": m.keyword,
            "event_type": m.event_type,
        })).collect::<Vec<_>>(),
    })
}

fn function_to_value(func: &Function, ir: &ParserIR) -> Value {
    // Initial values for locals extracted from entry_actions assignments
    let local_init_values = extract_local_init_values(&func.entry_actions);

    let mutable_locals = find_mutable_locals(func);

    // Filter out pure assignments from entry_actions (they become initializers)
    let filtered_entry_actions: Vec<&Command> = func
        .entry_actions
        .iter()
        .filter(|cmd| {
            !(cmd.ctype == "assign"
                && cmd
                    .arg_str("var")
                    .is_some_and(|v| local_init_values.contains_key(v)))
        })
        .collect();

    let mut param_types = Map::new();
    for (p, t) in &func.param_types {
        param_types.insert(p.clone(), json!(t.as_str()));
    }

    let mut locals = Map::new();
    for l in &func.locals {
        locals.insert(l.clone(), json!("i32")); // Ruby maps every local to :i32
    }

    let mut prepend_values = Map::new();
    for (p, values) in &func.prepend_values {
        let escaped: Vec<String> = values
            .iter()
            .map(|v| v.replace('\\', "\\\\")) // Ruby stores these pre-Rust-escaped
            .collect();
        prepend_values.insert(p.clone(), json!(escaped));
    }

    json!({
        "name": func.name,
        "return_type": func.return_type,
        "params": func.params,
        "param_types": param_types,
        "locals": locals,
        "local_init_values": local_init_values,
        "mutable_locals": mutable_locals,
        "states": func.states.iter().map(|s| state_to_value(s, ir)).collect::<Vec<_>>(),
        // Function-level eof handler: call args stay RAW (Ruby's
        // transform_call_args_by_type only touches states)
        "eof_handler": func.eof_handler.as_ref().map(|cmds| {
            cmds.iter().map(|c| command_to_value(c, None)).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "entry_actions": filtered_entry_actions.iter().map(|c| command_to_value(c, None)).collect::<Vec<_>>(),
        "emits_events": func.emits_events,
        "expects_char": func.expects_char,
        "emits_content_on_close": func.emits_content_on_close,
        "prepend_values": prepend_values,
        "lineno": func.lineno,
    })
}

/// Extract initial values for locals from entry_actions assignments,
/// transpiled with Ruby's mini expr transpiler (COL/LINE/PREV/:param only).
fn extract_local_init_values(entry_actions: &[Command]) -> Map<String, Value> {
    let mut init_values = Map::new();
    for cmd in entry_actions {
        if cmd.ctype != "assign" {
            continue;
        }
        let (Some(var), Some(expr)) = (cmd.arg_str("var"), cmd.arg_str("expr")) else {
            continue;
        };
        init_values.insert(var.to_string(), json!(mini_rust_expr(expr)));
    }
    init_values
}

fn mini_rust_expr(expr: &str) -> String {
    let s = re(r"\bCOL\b").replace_all(expr, "self.col()");
    let s = re(r"\bLINE\b").replace_all(&s, "self.line as i32");
    let s = re(r"\bPREV\b").replace_all(&s, "self.prev()");
    re(r"(?i):([a-z_][0-9a-z_]*)").replace_all(&s, "$1").into_owned()
}

/// Locals reassigned in the function body (states' cases + state eof
/// handlers), in first-seen order (Ruby: Set insertion order).
fn find_mutable_locals(func: &Function) -> Vec<String> {
    let mut mutable: Vec<String> = vec![];

    for state in &func.states {
        for kase in &state.cases {
            collect_mutable_vars(&kase.commands, &mut mutable);
        }
        if let Some(eof) = &state.eof_handler {
            collect_mutable_vars(eof, &mut mutable);
        }
    }

    mutable
}

fn collect_mutable_vars(commands: &[Command], mutable: &mut Vec<String>) {
    for cmd in commands {
        match cmd.ctype.as_str() {
            "assign" | "add_assign" | "sub_assign" => {
                if let Some(var) = cmd.arg_str("var") {
                    if !mutable.iter().any(|m| m == var) {
                        mutable.push(var.to_string());
                    }
                }
            }
            "conditional" => {
                if let Some(clauses) = &cmd.clauses {
                    for clause in clauses {
                        collect_mutable_vars(&clause.commands, mutable);
                    }
                }
            }
            _ => {}
        }
    }
}

fn state_to_value(state: &State, ir: &ParserIR) -> Value {
    json!({
        "name": state.name,
        "cases": state.cases.iter().map(|c| case_to_value(c, ir)).collect::<Vec<_>>(),
        "eof_handler": state.eof_handler.as_ref().map(|cmds| {
            cmds.iter().map(|c| command_to_value(c, Some(ir))).collect::<Vec<_>>()
        }).unwrap_or_default(),
        "scan_chars": state.scan_chars,
        "scannable": state.scannable(),
        "is_self_looping": state.is_self_looping,
        "has_default": state.has_default,
        "is_unconditional": state.is_unconditional,
        "newline_injected": state.newline_injected,
        "lineno": state.lineno,
    })
}

fn case_to_value(kase: &Case, ir: &ParserIR) -> Value {
    json!({
        "chars": kase.chars,
        "special_class": kase.special_class,
        "param_ref": kase.param_ref,
        "condition": kase.condition,
        "is_conditional": kase.is_conditional(),
        "substate": kase.substate,
        "commands": kase.commands.iter().map(|c| command_to_value(c, Some(ir))).collect::<Vec<_>>(),
        "is_default": kase.is_default(),
        "lineno": kase.lineno,
    })
}

/// Serialize a command to the context shape. When `transform` carries the IR
/// (i.e. we're inside a state), call args are rendered to Rust literals by
/// target param type — Ruby's transform_call_args_by_type, applied lazily.
fn command_to_value(cmd: &Command, transform: Option<&ParserIR>) -> Value {
    let mut args: Map<String, Value> = cmd.args.as_object().cloned().unwrap_or_default();

    if cmd.ctype == "call" {
        if let Some(ir) = transform {
            if let Some(call_args) = args.get("call_args").and_then(|v| v.as_str()) {
                if let Some(target) = ir.functions.iter().find(|f| {
                    args.get("name").and_then(|n| n.as_str()) == Some(f.name.as_str())
                }) {
                    let transformed = transform_args_for_target(call_args, target);
                    args.insert("call_args".to_string(), json!(transformed));
                }
            }
        }
    }

    if cmd.ctype == "conditional" {
        let clauses: Vec<Value> = cmd
            .clauses
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|clause| {
                json!({
                    "condition": clause.condition,
                    "commands": clause.commands.iter().map(|c| command_to_value(c, transform)).collect::<Vec<_>>(),
                })
            })
            .collect();
        args.insert("clauses".to_string(), Value::Array(clauses));
    }

    json!({ "type": cmd.ctype, "args": args })
}

/// Transform call arguments based on the target function's parameter types.
/// Port of Ruby IRBuilder#transform_args_for_target.
fn transform_args_for_target(args_str: &str, target: &Function) -> String {
    if target.params.is_empty() {
        return args_str.to_string();
    }

    let args = tokenize_call_args(args_str);

    args.iter()
        .enumerate()
        .map(|(i, arg)| {
            let Some(param) = target.params.get(i) else {
                return arg.clone();
            };
            let param_type = target.param_type(param);

            // Numeric literals are numbers, not characters
            if re(r"^-?\d+$").is_match(arg) {
                return match param_type {
                    Some(ParamType::Bytes) => "b\"\"".to_string(), // numeric sentinel -> empty bytes
                    Some(ParamType::Byte) => format!("{arg}u8"),
                    _ => arg.clone(),
                };
            }

            match param_type {
                Some(ParamType::Bytes) => literals::to_rust_bytes(&charclass::parse(arg)),
                Some(ParamType::Byte) => literals::to_rust_byte(&charclass::parse(arg)),
                _ => arg.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Helper-usage analysis (Ruby: Generator#analyze_helper_usage), operating on
// the serialized functions data exactly as Ruby does.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Usage {
    col: bool,
    prev: bool,
    set_term: bool,
    span: bool,
    letter: bool,
    label_cont: bool,
    digit: bool,
    hex_digit: bool,
    ws: bool,
    nl: bool,
    max_scan_arity: usize,
}

fn analyze_helper_usage(functions_data: &[Value]) -> Usage {
    let mut usage = Usage::default();

    for func in functions_data {
        check_expressions_in_function(func, &mut usage);

        for state in arr(func.get("states")) {
            // Track max scan arity
            if state.get("scannable").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(chars) = state.get("scan_chars").and_then(|v| v.as_array()) {
                    usage.max_scan_arity = usage.max_scan_arity.max(chars.len());
                }
            }

            for kase in arr(state.get("cases")) {
                check_special_class(kase.get("special_class"), &mut usage);
            }
        }
    }

    // span() is used for bracket types and errors (always needed if we have types)
    usage.span = true;

    usage
}

/// Collect every SAVE(slot) / USE_SAVED(slot) name so the template can
/// declare one parser field per slot (sorted, deduped).
fn collect_saved_slots(functions_data: &[Value]) -> Vec<String> {
    let mut slots = std::collections::BTreeSet::new();
    fn walk(v: &Value, slots: &mut std::collections::BTreeSet<String>) {
        match v {
            Value::Object(map) => {
                if let (Some(t), Some(args)) = (map.get("type").and_then(|t| t.as_str()), map.get("args")) {
                    if t == "save" || t == "inline_emit_saved" {
                        if let Some(slot) = args.get("slot").and_then(|s| s.as_str()) {
                            slots.insert(slot.to_string());
                        }
                    }
                }
                for val in map.values() {
                    walk(val, slots);
                }
            }
            Value::Array(a) => a.iter().for_each(|val| walk(val, slots)),
            _ => {}
        }
    }
    for f in functions_data {
        walk(f, &mut slots);
    }
    slots.into_iter().collect()
}

fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}

fn check_expressions_in_function(func: &Value, usage: &mut Usage) {
    let all_commands = collect_all_commands(func);

    for cmd in &all_commands {
        let ctype = cmd.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let args = cmd.get("args");

        // Condition expressions (commands never carry a 'condition' arg, but
        // Ruby checks anyway; kept for lockstep)
        check_expression(args.and_then(|a| a.get("condition")), usage);

        if ctype == "call" {
            check_expression(args.and_then(|a| a.get("call_args")), usage);
        }

        if matches!(ctype, "assign" | "add_assign" | "sub_assign") {
            check_expression(args.and_then(|a| a.get("expr")), usage);
        }

        if ctype == "term" {
            usage.set_term = true;
        }

        if ctype == "advance_to" {
            if let Some(value) = args.and_then(|a| a.get("value")).and_then(|v| v.as_str()) {
                usage.max_scan_arity = usage.max_scan_arity.max(value.chars().count());
            }
        }
    }

    // Case conditions
    for state in arr(func.get("states")) {
        for kase in arr(state.get("cases")) {
            check_expression(kase.get("condition"), usage);
        }
    }
}

/// Collect all commands from a serialized function: entry actions, function
/// eof handler, state eof handlers, case commands, and ONE level of
/// conditional clauses under case commands (Ruby's exact traversal —
/// including its blind spots; see module docs).
fn collect_all_commands(func: &Value) -> Vec<&Value> {
    let mut commands: Vec<&Value> = vec![];

    commands.extend(arr(func.get("entry_actions")));
    commands.extend(arr(func.get("eof_handler")));

    for state in arr(func.get("states")) {
        commands.extend(arr(state.get("eof_handler")));
        for kase in arr(state.get("cases")) {
            for cmd in arr(kase.get("commands")) {
                commands.push(cmd);
                if cmd.get("type").and_then(|v| v.as_str()) == Some("conditional") {
                    for clause in arr(cmd.get("args").and_then(|a| a.get("clauses"))) {
                        commands.extend(arr(clause.get("commands")));
                    }
                }
            }
        }
    }

    commands
}

fn check_expression(expr: Option<&Value>, usage: &mut Usage) {
    let Some(expr) = expr.and_then(|v| v.as_str()) else {
        return;
    };
    if re(r"\bCOL\b").is_match(expr) {
        usage.col = true;
    }
    if re(r"\bPREV\b").is_match(expr) {
        usage.prev = true;
    }
}

fn check_special_class(special_class: Option<&Value>, usage: &mut Usage) {
    let Some(sc) = special_class.and_then(|v| v.as_str()) else {
        return;
    };
    match sc {
        "letter" => usage.letter = true,
        "label_cont" => usage.label_cont = true,
        "digit" => usage.digit = true,
        "hex_digit" => usage.hex_digit = true,
        "ws" => usage.ws = true,
        "nl" => usage.nl = true,
        _ => {}
    }
}

fn uses_unicode_classes(functions_data: &[Value]) -> bool {
    functions_data.iter().any(|func| {
        arr(func.get("states")).iter().any(|state| {
            arr(state.get("cases")).iter().any(|kase| {
                kase.get("special_class")
                    .and_then(|v| v.as_str())
                    .is_some_and(|sc| UNICODE_CLASSES.contains(&sc))
            })
        })
    })
}
