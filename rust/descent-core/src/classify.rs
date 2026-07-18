//! Static positional / delimited classification of grammar functions.
//!
//! **Report-only.** Reads the IR and prints a classification; does NOT affect
//! code generation. Answers the question the UDON EOF refactor turns on: is each
//! function **positional** (closes on geometry — newline / dedent / EOF, no
//! anomaly) or **delimited** (closes only on a printed end-sequence)? See
//! `spec/TODO-EOF-refactor.md` in the UDON repo.
//!
//! It encodes the *rule* (classify by exit structure), not a hand-authored
//! answer key — so its output is an independent check against a human reading
//! and against fresh-eyes review.
//!
//! ## The rule (refined from the design doc's "closer-accept vs geometric")
//!
//! For each exit (a case / `|eof` handler / conditional clause whose commands
//! `return`), tag it:
//!   * **Delegation** — the edge makes a `/call`; it delegates its close to a
//!     callee (the closer-in-callee pattern). Classified later by the callee's
//!     kind.
//!   * **DelimFailure** — carries a `Warning(Unclosed*/Unterminated*)`: a
//!     delimited construct's keep-content-and-warn. Recorded as *function-level*
//!     (on the fn's own `|eof`) or *state-level*.
//!   * **SemanticClose** — carries an `error` (e.g. `MissingAttributeValue`): a
//!     semantic check riding on a positional close. The litmus's "out".
//!   * **Geometric** — returns on `\n` / space / a dedent condition (`col <= …`),
//!     or on a matched char left *unconsumed* (an enclosing construct's
//!     terminator), or a bare fall-through `default` — geometry ended it.
//!   * **Closer** — returns having *consumed* a non-geometric char/param, with
//!     no `/call`: the construct's own printed end-sequence.
//!
//! Kind (in order):
//!   1. **function-level** DelimFailure → **Delimited** (the whole function
//!      warns-if-EOF-while-open; any geometric is a positional tail *after* the
//!      closer — freeform's `post_close`).
//!   2. state-level DelimFailure **and** a clean Geometric → **Mixed** (a
//!      delimited sub-region inside a positional function — `typed_value`'s
//!      `<…>` envelope; the extract-into-own-function candidate).
//!   3. any clean Geometric → **Positional**.
//!   4. a Closer or (state-level) DelimFailure → **Delimited**.
//!   5. only Delegations → inherit (fixed point): any callee Positional →
//!      Positional; else all-Delimited → Delimited.
//!   6. nothing → **Inert**.

use crate::ir::{Case, Command, Function, ParserIR};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Positional,
    Delimited,
    Mixed,
    Inert,
    Unresolved, // pending delegation inheritance
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Positional => "POSITIONAL",
            Kind::Delimited => "DELIMITED ",
            Kind::Mixed => "MIXED     ",
            Kind::Inert => "inert     ",
            Kind::Unresolved => "unresolved",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FnClass {
    pub name: String,
    pub return_type: Option<String>,
    pub kind: Kind,
    closers: Vec<String>,
    geometrics: Vec<String>,
    delegations: Vec<String>, // callee names
    delim_failures: Vec<String>,
    semantic_closes: Vec<String>,
    fn_level_delimfail: bool,
    notes: Vec<String>,
}

fn has_return(cmds: &[Command]) -> bool {
    cmds.iter().any(|c| {
        c.ctype == "return"
            || c.clauses.as_ref().is_some_and(|cl| cl.iter().any(|cl| has_return(&cl.commands)))
    })
}

fn has_advance(cmds: &[Command]) -> bool {
    cmds.iter().any(|c| {
        c.ctype == "advance"
            || c.ctype == "advance_to"
            || c.clauses.as_ref().is_some_and(|cl| cl.iter().any(|cl| has_advance(&cl.commands)))
    })
}

/// ("warning"|"error", code) carried on an edge, recursing into clauses.
fn anomaly(cmds: &[Command]) -> Option<(&'static str, String)> {
    for c in cmds {
        if c.ctype == "error" {
            return Some(("error", c.arg_str("value").unwrap_or("").to_string()));
        }
        if c.ctype.starts_with("inline_emit") {
            if let Some(ty) = c.args.get("type").and_then(|v| v.as_str()) {
                if ty == "Warning" {
                    let code = c.args.get("literal").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    return Some(("warning", code));
                }
                if ty == "Error" {
                    return Some(("error", "(inline Error)".to_string()));
                }
            }
        }
        if let Some(cl) = &c.clauses {
            for clause in cl {
                if let Some(a) = anomaly(&clause.commands) {
                    return Some(a);
                }
            }
        }
    }
    None
}

fn callees(cmds: &[Command]) -> Vec<String> {
    let mut out = vec![];
    for c in cmds {
        if c.ctype == "call" {
            if let Some(name) = c.arg_str("name").or_else(|| c.arg_str("value")) {
                // strip any "(args)"
                let n = name.split('(').next().unwrap_or(name);
                out.push(n.to_string());
            }
        }
        if let Some(cl) = &c.clauses {
            for clause in cl {
                out.extend(callees(&clause.commands));
            }
        }
    }
    out
}

fn is_geometric_char(chars: &Option<Vec<String>>) -> bool {
    matches!(chars.as_deref(), Some(cs) if cs.iter().all(|c| c == "\n" || c == " " || c == " \t" || c == "\t" || c == " \t\n"))
}

#[derive(PartialEq)]
enum Tag {
    Closer,
    Geometric,
    Delegation,
    DelimFailure,
    SemanticClose,
}

/// Only `Unclosed*` / `Unterminated*` warnings are closer-failures; other
/// warnings (segment-ingest, NoDialectsLoaded, InconsistentIndentation) are
/// benign and don't make an edge delimited.
fn is_closer_failure(code: &str) -> bool {
    code.starts_with("Unclosed") || code.starts_with("Unterminated")
}

fn tag_edge(case: &Case) -> Tag {
    if let Some((sev, code)) = anomaly(&case.commands) {
        if sev == "error" {
            return Tag::SemanticClose;
        }
        if is_closer_failure(&code) {
            return Tag::DelimFailure;
        }
        // benign warning: fall through to geometric/closer tagging below
    }
    if !callees(&case.commands).is_empty() {
        return Tag::Delegation;
    }
    // geometry: newline/space triggers, dedent conditions, unconsumed matches
    if is_geometric_char(&case.chars) {
        return Tag::Geometric;
    }
    if case.condition.is_some() {
        return Tag::Geometric; // dedent guard (col <= …) etc.
    }
    let consumed = has_advance(&case.commands);
    if (case.chars.is_some() || case.param_ref.is_some() || case.special_class.is_some() || case.is_default())
        && consumed
    {
        Tag::Closer // consumed a non-geometric printed end-sequence
    } else {
        Tag::Geometric // unconsumed terminator-stop / bare fall-through
    }
}

fn classify_fn_direct(func: &Function) -> FnClass {
    let mut closers = vec![];
    let mut geometrics = vec![];
    let mut delegations = vec![];
    let mut delim_failures = vec![];
    let mut semantic_closes = vec![];
    let mut fn_level_delimfail = false;

    // function-level eof handler
    if let Some(h) = &func.eof_handler {
        if has_return(h) {
            match anomaly(h) {
                Some((sev, code)) if sev == "warning" && is_closer_failure(&code) => {
                    fn_level_delimfail = true;
                    delim_failures.push(format!("fn-eof warning({code})"));
                }
                Some((sev, code)) if sev == "error" => {
                    semantic_closes.push(format!("fn-eof error({code})"))
                }
                _ => {}
            }
        }
    }

    for st in &func.states {
        if let Some(h) = &st.eof_handler {
            if has_return(h) {
                match anomaly(h) {
                    Some((sev, code)) if sev == "warning" && is_closer_failure(&code) => {
                        delim_failures.push(format!("eof warning({code})@{}", st.name))
                    }
                    Some((sev, code)) if sev == "error" => {
                        semantic_closes.push(format!("eof error({code})@{}", st.name))
                    }
                    _ => {}
                }
            }
        }
        for case in &st.cases {
            if !has_return(&case.commands) {
                continue;
            }
            let detail = format!("{}@{} [{}]", trig(case), st.name, case.lineno);
            match tag_edge(case) {
                Tag::Closer => closers.push(detail),
                Tag::Geometric => geometrics.push(detail),
                Tag::Delegation => delegations.extend(callees(&case.commands)),
                Tag::DelimFailure => delim_failures.push(detail),
                Tag::SemanticClose => semantic_closes.push(detail),
            }
        }
    }
    delegations.sort();
    delegations.dedup();

    let has_geometric = !geometrics.is_empty();
    let has_closer = !closers.is_empty();
    let has_delimfail = !delim_failures.is_empty();

    let kind = if fn_level_delimfail {
        Kind::Delimited
    } else if has_delimfail && has_geometric {
        Kind::Mixed
    } else if has_geometric {
        Kind::Positional
    } else if has_closer || has_delimfail {
        Kind::Delimited
    } else if !delegations.is_empty() {
        Kind::Unresolved // resolve by inheritance
    } else {
        Kind::Inert
    };

    FnClass {
        name: func.name.clone(),
        return_type: func.return_type.clone(),
        kind,
        closers,
        geometrics,
        delegations,
        delim_failures,
        semantic_closes,
        fn_level_delimfail,
        notes: vec![],
    }
}

fn trig(case: &Case) -> String {
    if case.is_default() {
        "default".into()
    } else if let Some(c) = &case.condition {
        format!("if[{c}]")
    } else if let Some(p) = &case.param_ref {
        format!(":{p}")
    } else if let Some(cl) = &case.special_class {
        cl.clone()
    } else if let Some(cs) = &case.chars {
        format!("'{}'", cs.join(""))
    } else {
        "?".into()
    }
}

pub fn classify(ir: &ParserIR) -> Vec<FnClass> {
    let mut classes: Vec<FnClass> = ir.functions.iter().map(classify_fn_direct).collect();

    // Fixed-point delegation inheritance: an Unresolved function (only
    // delegations) inherits — any Positional callee → Positional; else all
    // resolved-Delimited → Delimited.
    for _ in 0..classes.len() + 1 {
        let snapshot: HashMap<String, Kind> =
            classes.iter().map(|c| (c.name.clone(), c.kind)).collect();
        let mut changed = false;
        for c in classes.iter_mut() {
            if c.kind != Kind::Unresolved {
                continue;
            }
            let kinds: Vec<Kind> =
                c.delegations.iter().filter_map(|d| snapshot.get(d).copied()).collect();
            if kinds.iter().any(|k| *k == Kind::Positional || *k == Kind::Mixed) {
                c.kind = Kind::Positional;
                c.notes.push(format!("closer-in-callee → positional via {:?}", c.delegations));
                changed = true;
            } else if !kinds.is_empty() && kinds.iter().all(|k| *k == Kind::Delimited) {
                c.kind = Kind::Delimited;
                c.notes.push(format!("closer-in-callee → delimited via {:?}", c.delegations));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // Any still-Unresolved: leave as-is (they delegate only to unresolved/inert).
    classes
}

pub fn report(ir: &ParserIR) -> String {
    use std::fmt::Write;
    let classes = classify(ir);
    let mut b = String::new();
    let _ = writeln!(b, "# descent classification — {} functions\n", classes.len());
    let (mut np, mut nd, mut nm, mut ni, mut nu) = (0, 0, 0, 0, 0);
    for c in &classes {
        match c.kind {
            Kind::Positional => np += 1,
            Kind::Delimited => nd += 1,
            Kind::Mixed => nm += 1,
            Kind::Inert => ni += 1,
            Kind::Unresolved => nu += 1,
        }
    }
    let _ = writeln!(b, "positional={np} delimited={nd} MIXED={nm} inert={ni} unresolved={nu}\n");
    for c in &classes {
        let rt = c.return_type.as_deref().unwrap_or("-");
        let df = if c.fn_level_delimfail { " [fn-level Unclosed]" } else { "" };
        let _ = writeln!(b, "{}  {:<24} : {rt}{df}", c.kind.label(), c.name);
        if !c.closers.is_empty() {
            let _ = writeln!(b, "    closer   : {}", c.closers.join(" | "));
        }
        if !c.geometrics.is_empty() {
            let _ = writeln!(b, "    geometric: {}", c.geometrics.join(" | "));
        }
        if !c.delegations.is_empty() {
            let _ = writeln!(b, "    delegates: {}", c.delegations.join(", "));
        }
        if !c.delim_failures.is_empty() {
            let _ = writeln!(b, "    delimfail: {}", c.delim_failures.join(" | "));
        }
        if !c.semantic_closes.is_empty() {
            let _ = writeln!(b, "    semantic : {}", c.semantic_closes.join(" | "));
        }
        for n in &c.notes {
            let _ = writeln!(b, "    note     : {n}");
        }
    }
    b
}
