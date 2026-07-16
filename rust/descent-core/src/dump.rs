//! Canonical JSON serialization of Tokens and AST for differential testing
//! against Ruby descent (`rust/tools/dump_tokens.rb`, `rust/tools/dump_ast.rb`
//! produce the identical shape). Keep the two sides in lockstep.

use crate::ast::*;
use crate::lexer::Token;
use serde_json::{json, Value};

pub fn tokens_to_json(tokens: &[Token]) -> Value {
    Value::Array(
        tokens
            .iter()
            .map(|t| json!({ "tag": t.tag, "id": t.id, "rest": t.rest, "lineno": t.lineno }))
            .collect(),
    )
}

pub fn machine_to_json(m: &Machine) -> Value {
    json!({
        "name": m.name,
        "entry_point": m.entry_point,
        "types": m.types.iter().map(type_to_json).collect::<Vec<_>>(),
        "functions": m.functions.iter().map(function_to_json).collect::<Vec<_>>(),
        "keywords": m.keywords.iter().map(keywords_to_json).collect::<Vec<_>>(),
    })
}

fn type_to_json(t: &TypeDecl) -> Value {
    json!({ "name": t.name, "kind": t.kind, "lineno": t.lineno })
}

fn function_to_json(f: &Function) -> Value {
    json!({
        "name": f.name,
        "return_type": f.return_type,
        "params": f.params,
        "states": f.states.iter().map(state_to_json).collect::<Vec<_>>(),
        "eof_handler": f.eof_handler.as_ref().map(eof_to_json),
        "entry_actions": f.entry_actions.iter().map(command_to_json).collect::<Vec<_>>(),
        "lineno": f.lineno,
    })
}

fn state_to_json(s: &State) -> Value {
    json!({
        "name": s.name,
        "cases": s.cases.iter().map(case_to_json).collect::<Vec<_>>(),
        "eof_handler": s.eof_handler.as_ref().map(eof_to_json),
        "lineno": s.lineno,
    })
}

fn case_to_json(c: &Case) -> Value {
    json!({
        "chars": c.chars,
        "condition": c.condition,
        "substate": c.substate,
        "commands": c.commands.iter().map(command_to_json).collect::<Vec<_>>(),
        "lineno": c.lineno,
    })
}

fn eof_to_json(e: &EOFHandler) -> Value {
    json!({
        "commands": e.commands.iter().map(command_to_json).collect::<Vec<_>>(),
        "lineno": e.lineno,
    })
}

fn command_to_json(cmd: &Command) -> Value {
    match cmd {
        Command::Cmd { kind, lineno } => {
            let (ty, value) = kind_to_json(kind);
            json!({ "node": "command", "type": ty, "value": value, "lineno": lineno })
        }
        Command::Conditional { clauses, lineno } => json!({
            "node": "conditional",
            "clauses": clauses.iter().map(clause_to_json).collect::<Vec<_>>(),
            "lineno": lineno,
        }),
    }
}

fn clause_to_json(c: &Clause) -> Value {
    json!({
        "condition": c.condition,
        "commands": c.commands.iter().map(command_to_json).collect::<Vec<_>>(),
    })
}

/// Mirror of Ruby classify_command's `[type, value]` pairs.
fn kind_to_json(kind: &CmdKind) -> (&'static str, Value) {
    match kind {
        CmdKind::Advance => ("advance", Value::Null),
        CmdKind::AdvanceTo(s) => ("advance_to", json!(s)),
        CmdKind::Transition(s) => ("transition", json!(s)),
        CmdKind::Return(s) => ("return", json!(s)),
        CmdKind::Error(s) => ("error", json!(s)),
        CmdKind::Mark => ("mark", Value::Null),
        CmdKind::Term(v) => ("term", v.map(|n| json!(n)).unwrap_or(Value::Null)),
        CmdKind::Emit(v) => ("emit", v.as_ref().map(|s| json!(s)).unwrap_or(Value::Null)),
        CmdKind::Call(s) => ("call", json!(s)),
        CmdKind::CallMethod(s) => ("call_method", json!(s)),
        CmdKind::Scan(s) => ("scan", json!(s)),
        CmdKind::KeywordsLookup(s) => ("keywords_lookup", json!(s)),
        CmdKind::Prepend(s) => ("prepend", json!(s)),
        CmdKind::PrependParam(s) => ("prepend_param", json!(s)),
        CmdKind::Assign { var, expr } => ("assign", json!({ "var": var, "expr": expr })),
        CmdKind::AddAssign { var, expr } => ("add_assign", json!({ "var": var, "expr": expr })),
        CmdKind::SubAssign { var, expr } => ("sub_assign", json!({ "var": var, "expr": expr })),
        CmdKind::InlineEmitMark(s) => ("inline_emit_mark", json!(s)),
        CmdKind::Save(s) => ("save", json!(s)),
        CmdKind::InlineEmitSaved { ty, slot } => ("inline_emit_saved", json!({ "type": ty, "slot": slot })),
        CmdKind::InlineEmitLiteral { ty, literal } => {
            ("inline_emit_literal", json!({ "type": ty, "literal": literal }))
        }
        CmdKind::InlineEmitBare(s) => ("inline_emit_bare", json!(s)),
        CmdKind::Noop => ("noop", Value::Null),
    }
}

fn keywords_to_json(k: &Keywords) -> Value {
    json!({
        "name": k.name,
        "fallback": k.fallback,
        "mappings": k.mappings.iter().map(|m| json!({
            "keyword": m.keyword, "event_type": m.event_type
        })).collect::<Vec<_>>(),
        "lineno": k.lineno,
    })
}
