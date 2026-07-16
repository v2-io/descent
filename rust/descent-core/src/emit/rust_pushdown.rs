//! Pushdown (explicit-stack) Rust backend — the resumable parser.
//!
//! Generates a `PushdownParser` from the same neutral IR as the recursive
//! backend: a trampoline over an explicit frame stack, suspendable at any
//! byte boundary (`push_chunk` / `finish`). Feasibility, the four crux
//! analyses, and the differential methodology live in the UDON repo:
//! `notes/spikes/explicit-stack-feasibility-2026-07.md`. The transformation
//! is defunctionalization of a closed, statically-known call graph:
//!
//! - Each grammar function becomes a `Frame` variant (params + locals +
//!   state). The indent stack IS the frame stack.
//! - Every suspension-capable command splits its sequence: `call` sets the
//!   current frame's state to a fresh continuation and pushes the callee;
//!   `return` emits (type-driven) and pops; `->[c]` (advance_to) gets its
//!   own state so a chunk boundary mid-scan resumes idempotently; a
//!   mid-sequence `->` past the buffer end becomes `pending_skip`, drained
//!   when bytes arrive.
//! - Capture (mark/term/prepend) stays contiguous by design: the parser
//!   owns an accumulation buffer and drains only bytes no capture can still
//!   reference, so `mark..pos` never spans a seam and `TERM(-1)` needs no
//!   cross-chunk special case. Spans are global offsets.
//! - "EOF" splits into two conditions: buffer exhausted (suspend, return
//!   `NeedMoreData`) vs `finish()`ed (run the same type-driven EOF behavior
//!   the recursive backend infers, cascading pops unwinding the stack).
//!
//! Emission is borrow-from-buffer (v2): content events carry
//! `Cow::Borrowed` slices of the accumulation buffer wherever a drain
//! cannot invalidate them — only PREPEND-combined content and SAVE-slot
//! re-emission are owned. Delivery contract: a borrowed event is valid
//! only during the callback that receives it (enforced by the HRTB
//! `for<'e> FnMut(StreamEvent<'e>)` bound — nothing can outlive the call
//! without an explicit copy). Deliberate remaining limit: no `--trace`
//! plumbing. The generated module imports `ParseErrorCode` / `StreamEvent`
//! / `ParseResult` from the sibling recursive module so the two backends
//! interoperate.

use crate::ir::*;
use crate::lexer::re;
use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::rust::engine::{pascalcase, rust_expr};
use super::rust::literals::escape_rust_byte;

/// Options for pushdown generation.
#[derive(Debug, Clone)]
pub struct PdOptions {
    /// Rust path of the sibling recursive module that owns the shared
    /// event/error types (e.g. "crate::parser").
    pub event_path: String,
}

impl Default for PdOptions {
    fn default() -> Self {
        PdOptions { event_path: "crate::parser".to_string() }
    }
}

pub fn generate(ir: &ParserIR, opts: &PdOptions) -> String {
    let mut g = Gen::new(ir, opts);
    g.run();
    g.out
}

// ============================================================================
// Expression rendering (frame-addressed variables)
// ============================================================================

/// Render a DSL expression with function variables addressed through the
/// frame binding `f`. Works on the RAW DSL (before COL/PREV expansion) so
/// bare lowercase var names can't collide with the generated `self.col()`.
fn pd_expr(dsl: &str, vars: &BTreeSet<String>) -> String {
    pd_expr_prefixed(dsl, vars, "f.")
}

/// Marked names use a word-char token (`PDVAR_name_RAVDP`) so the per-var
/// word-boundary pass can't re-match an already-marked name; `prefix` is
/// `f.` in state bodies and empty in `enter_` initializer context (params
/// are in scope by name there).
fn pd_expr_prefixed(dsl: &str, vars: &BTreeSet<String>, prefix: &str) -> String {
    let mut s = re(r"(?i):([a-z_]\w*)")
        .replace_all(dsl, "PDVAR_${1}_RAVDP")
        .into_owned();
    for v in vars {
        s = re(&format!(r"\b{}\b", regex::escape(v)))
            .replace_all(&s, format!("PDVAR_{v}_RAVDP"))
            .into_owned();
    }
    let s = re(r"PDVAR_([A-Za-z_0-9]+?)_RAVDP")
        .replace_all(&s, format!("{prefix}$1"))
        .into_owned();
    // hand the rest (COL/PREV, char literals, escapes) to the shared pipeline
    rust_expr(&s)
}

fn esc_char(c: &str) -> String {
    c.chars().next().map(escape_rust_byte).unwrap_or_else(|| "0u8".to_string())
}

fn esc_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ============================================================================
// Sequence-end discipline
// ============================================================================

/// What a command sequence does when it runs off its end without explicit
/// control flow: re-dispatch the same state (grammar semantics of a case
/// body without a transition are "shouldn't happen", but conditionals
/// without an else clause legitimately fall through) or go to a specific
/// continuation state.
#[derive(Clone)]
enum SeqEnd {
    Redispatch,
    Goto(String),
}

// ============================================================================
// Generator
// ============================================================================

struct FnInfo<'i> {
    func: &'i Function,
    kind: &'i str, // "bracket" | "content" | "internal" | ""
    vars: BTreeSet<String>,
}

struct Gen<'i> {
    ir: &'i ParserIR,
    opts: &'i PdOptions,
    out: String,
    /// Continuation states allocated while rendering the current function.
    cont_states: Vec<(String, String)>,
    cont_counter: usize,
}

const IND: usize = 28;

impl<'i> Gen<'i> {
    fn new(ir: &'i ParserIR, opts: &'i PdOptions) -> Self {
        Gen { ir, opts, out: String::new(), cont_states: Vec::new(), cont_counter: 0 }
    }

    fn type_kind(&self, name: &Option<String>) -> &'i str {
        name.as_deref()
            .and_then(|n| self.ir.types.iter().find(|t| t.name == n))
            .map(|t| t.kind.as_str())
            .unwrap_or("")
    }

    fn fn_info(&self, func: &'i Function) -> FnInfo<'i> {
        let mut vars: BTreeSet<String> = func.params.iter().cloned().collect();
        vars.extend(func.locals.iter().cloned());
        FnInfo { func, kind: self.type_kind(&func.return_type), vars }
    }

    fn fresh_cont(&mut self) -> String {
        self.cont_counter += 1;
        format!("PdK{}", self.cont_counter)
    }

    fn run(&mut self) {
        let ep = self.ir.entry_point.clone().unwrap_or_default().replace('/', "");
        let event_path = &self.opts.event_path;
        let _ = write!(
            self.out,
            "//! Generated pushdown (explicit-stack) parser - DO NOT EDIT\n\
             //!\n\
             //! Generated by descent (`--backend pushdown`) from the same grammar as\n\
             //! the sibling recursive parser. Resumable at any byte boundary: feed\n\
             //! with `push_chunk`, close with `finish`. Emits the sibling's\n\
             //! `StreamEvent`s with global spans. Content is borrowed from the\n\
             //! parser's accumulation buffer where safe (`Cow::Borrowed`), owned\n\
             //! only where a buffer drain would invalidate it (PREPEND-combined\n\
             //! content, SAVE-slot re-emission). Delivery contract: a borrowed\n\
             //! event is valid only during the callback that receives it — copy\n\
             //! (`into_owned`) anything that must survive past the callback or\n\
             //! the next `push_chunk`.\n\n\
             use {event_path}::{{ParseErrorCode, ParseResult, StreamEvent}};\n\n"
        );

        for func in &self.ir.functions {
            self.render_frame_struct(func);
        }
        let _ = writeln!(self.out, "#[derive(Debug)]\nenum Frame {{");
        for func in &self.ir.functions {
            let p = pascalcase(&func.name);
            let _ = writeln!(self.out, "    {p}({p}Frame),");
        }
        let _ = writeln!(self.out, "}}\n");

        let _ = write!(self.out, "{}", RUNTIME.replace("__ENTRY__", &ep));

        let mut arms = String::new();
        for func in &self.ir.functions {
            self.render_enter(func);
            let arm = self.render_fn_arm(func);
            arms.push_str(&arm);
        }
        self.render_keyword_lookups();

        let _ = write!(
            self.out,
            "    /// Drive the machine until it needs more bytes or the stack empties.\n\
             \x20   #[allow(unreachable_code, unused_variables)]\n\
             \x20   fn run<F>(&mut self, on_event: &mut F) -> ParseResult\n\
             \x20   where\n\
             \x20       F: for<'e> FnMut(StreamEvent<'e>),\n\
             \x20   {{\n\
             \x20       'run: loop {{\n\
             \x20           // Drain pending mid-sequence advances first.\n\
             \x20           while self.pending_skip > 0 {{\n\
             \x20               if self.pos < self.buf.len() {{\n\
             \x20                   self.advance();\n\
             \x20                   self.pending_skip -= 1;\n\
             \x20               }} else if self.finished {{\n\
             \x20                   self.pending_skip = 0;\n\
             \x20               }} else {{\n\
             \x20                   return ParseResult::NeedMoreData;\n\
             \x20               }}\n\
             \x20           }}\n\
             \x20           let Some(frame) = self.stack.pop() else {{\n\
             \x20               return ParseResult::Complete;\n\
             \x20           }};\n\
             \x20           // The frame is popped for ownership; every non-`return`\n\
             \x20           // path pushes it back before continuing.\n\
             \x20           match frame {{\n{arms}\
             \x20           }}\n\
             \x20       }}\n\
             \x20   }}\n\
             }}\n"
        );
    }

    fn render_frame_struct(&mut self, func: &Function) {
        let p = pascalcase(&func.name);
        let _ = writeln!(self.out, "#[derive(Debug)]\nstruct {p}Frame {{");
        let _ = writeln!(self.out, "    st: {p}St,");
        for (name, ty) in &func.param_types {
            let rty = match ty {
                ParamType::I32 => "i32",
                ParamType::Byte => "u8",
                ParamType::Bytes => "&'static [u8]",
            };
            let _ = writeln!(self.out, "    {name}: {rty},");
        }
        for l in &func.locals {
            if !func.params.contains(l) {
                let _ = writeln!(self.out, "    {l}: i32,");
            }
        }
        let _ = writeln!(self.out, "}}\n");
    }

    // ---- enter_<fn>: Start-emit / mark / frame init, then push ----
    fn render_enter(&mut self, func: &Function) {
        let info = self.fn_info(func);
        let p = pascalcase(&func.name);
        let mut params_sig = String::new();
        for (name, ty) in &func.param_types {
            let rty = match ty {
                ParamType::I32 => "i32",
                ParamType::Byte => "u8",
                ParamType::Bytes => "&'static [u8]",
            };
            let _ = write!(params_sig, "{name}: {rty}, ");
        }
        let _ = writeln!(
            self.out,
            "    fn enter_{name}<F>(&mut self, {params_sig}on_event: &mut F)\n    where\n        F: for<'e> FnMut(StreamEvent<'e>),\n    {{",
            name = func.name
        );
        match info.kind {
            "bracket" => {
                let t = func.return_type.as_deref().unwrap();
                let _ = writeln!(self.out, "        on_event(StreamEvent::{t}Start {{ span: self.gspan() }});");
            }
            "content" => {
                let _ = writeln!(self.out, "        self.mark();");
            }
            _ => {}
        }
        let init_map = pure_entry_inits(func);
        let _ = writeln!(self.out, "        self.stack.push(Frame::{p}({p}Frame {{");
        let _ = writeln!(self.out, "            st: {p}St::PdEntry,");
        for pn in &func.params {
            let _ = writeln!(self.out, "            {pn},");
        }
        for l in &func.locals {
            if func.params.contains(l) {
                continue;
            }
            let init = init_map
                .get(l.as_str())
                .map(|e| init_expr(e, func))
                .unwrap_or_else(|| "0".to_string());
            let _ = writeln!(self.out, "            {l}: {init},");
        }
        let _ = writeln!(self.out, "        }}));");
        let _ = writeln!(self.out, "    }}\n");
    }

    fn render_keyword_lookups(&mut self) {
        for kw in &self.ir.keywords {
            let const_name = format!("{}_KEYWORDS_PD", kw.name.to_uppercase());
            let _ = writeln!(self.out, "}}\n\nstatic {const_name}: &[(&[u8], usize)] = &[");
            for (i, m) in kw.mappings.iter().enumerate() {
                let _ = writeln!(self.out, "    (b\"{}\", {i}),", m.keyword);
            }
            let _ = writeln!(self.out, "];\n\n#[allow(unused_variables, dead_code)]\nimpl PushdownParser {{");
            let _ = writeln!(
                self.out,
                "    fn lookup_{name}<F>(&mut self, on_event: &mut F) -> bool\n    where\n        F: for<'e> FnMut(StreamEvent<'e>),\n    {{\n        let (content, span) = self.take_capture();\n        let Some(&(_, id)) = {const_name}.iter().find(|(k, _)| *k == &content[..]) else {{ return false; }};\n        match id {{",
                name = kw.name
            );
            for (i, m) in kw.mappings.iter().enumerate() {
                let _ = writeln!(
                    self.out,
                    "            {i} => on_event(StreamEvent::{t} {{ content, span }}),",
                    t = m.event_type
                );
            }
            let _ = writeln!(self.out, "            _ => unreachable!(),\n        }}\n        true\n    }}\n");
        }
    }

    // ---- per-function trampoline arm ----
    fn render_fn_arm(&mut self, func: &'i Function) -> String {
        let info = self.fn_info(func);
        let p = pascalcase(&func.name);
        self.cont_states.clear();

        let mut bodies: Vec<(String, String)> = Vec::new();

        // Synthetic PdEntry state: non-init entry actions, then the first
        // grammar state (or the implicit type-driven return for stateless
        // functions like directive_args / verbatim_text).
        let mut seq = non_init_entry_actions(func, info.kind);
        if let Some(first) = func.states.first() {
            seq.push(Command::new(
                "transition",
                serde_json::json!({ "value": first.name.clone() }),
            ));
        } else {
            seq.push(Command::new("return", serde_json::json!({})));
        }
        let mut entry_body = String::new();
        self.render_seq(&mut entry_body, &seq, &info, &p, IND, &SeqEnd::Redispatch, "PdEntry");
        bodies.push(("PdEntry".to_string(), entry_body));

        for state in &func.states {
            let body = self.render_state(state, &info, &p);
            bodies.push((pascalcase(&state.name), body));
        }
        while !self.cont_states.is_empty() {
            bodies.extend(std::mem::take(&mut self.cont_states));
        }

        // Per-function state enum, inserted before the Frame enum.
        let mut st_enum = format!("#[derive(Debug, Clone, Copy, PartialEq)]\nenum {p}St {{ ");
        for (name, _) in &bodies {
            let _ = write!(st_enum, "{name}, ");
        }
        st_enum.push_str("}\n\n");
        self.out = self.out.replacen(
            "#[derive(Debug)]\nenum Frame {",
            &format!("{st_enum}#[derive(Debug)]\nenum Frame {{"),
            1,
        );

        // In-arm state loop: state hops (`continue 'st`) stay inside this
        // frame's arm — no stack pop/push, no Frame-variant re-match. Only
        // calls, returns, and suspensions go back through the trampoline.
        // The pending_skip guard bails to the trampoline top (which owns
        // the drain-or-suspend decision) exactly as the pre-loop code did.
        let mut arm = format!(
            "                Frame::{p}(mut f) => {{\n                    'st: loop {{\n                    if self.pending_skip > 0 {{ self.stack.push(Frame::{p}(f)); continue 'run; }}\n                    match f.st {{\n"
        );
        for (name, body) in &bodies {
            let _ = write!(
                arm,
                "                        {p}St::{name} => {{\n{body}                        }}\n"
            );
        }
        arm.push_str("                    }\n                    }\n                }\n");
        arm
    }

    // ---- a grammar state: suspend/EOF handling + case dispatch ----
    fn render_state(&mut self, state: &State, info: &FnInfo<'i>, p: &str) -> String {
        let mut b = String::new();
        let home = pascalcase(&state.name);

        if state.is_unconditional {
            let cmds = state.cases[0].commands.clone();
            self.render_seq(&mut b, &cmds, info, p, IND, &SeqEnd::Redispatch, &home);
            return b;
        }

        if state.scannable() {
            let mut args_v: Vec<String> = state
                .scan_chars
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|c| esc_char(c))
                .collect();
            // Runtime byte params join the scan set as frame-addressed needles.
            args_v.extend(state.scan_params.iter().map(|p| format!("f.{p}")));
            let n = args_v.len();
            let _ = writeln!(b, "{:IND$}match self.scan_to{n}({args}) {{", "", args = args_v.join(", "));
            for case in &state.cases {
                if case.is_default() {
                    continue;
                }
                self.render_case(&mut b, case, info, p, IND + 4, &home);
            }
            if state.newline_injected {
                let _ = writeln!(
                    b,
                    "{:i$}Some(b'\\n') => {{ self.advance(); continue 'st; }}",
                    "",
                    i = IND + 4
                );
            }
            let _ = writeln!(b, "{:i$}None => {{", "", i = IND + 4);
            let _ = writeln!(
                b,
                "{:i$}if !self.finished {{ self.stack.push(Frame::{p}(f)); return ParseResult::NeedMoreData; }}",
                "",
                i = IND + 8
            );
            self.render_eof(&mut b, state, info, p, IND + 8, &home);
            let _ = writeln!(b, "{:i$}}}", "", i = IND + 4);
            let _ = writeln!(b, "{:i$}_ => unreachable!(),", "", i = IND + 4);
            let _ = writeln!(b, "{:IND$}}}", "");
            return b;
        }

        // Byte-independent state (pure conditionals/default — no character
        // or class cases, no explicit |eof): it cannot consume input, so its
        // guard chain must also run at EOF — skip the exhausted/EOF preamble
        // entirely (mirrors the recursive backend's byte_independent flag).
        let byte_independent = state.eof_handler.is_none()
            && state.cases.iter().all(|c| {
                c.chars.is_none() && c.special_class.is_none() && c.param_ref.is_none()
            });

        // Non-scannable: exhausted check, then dispatch.
        if !byte_independent {
            let _ = writeln!(b, "{:IND$}if self.pos >= self.buf.len() {{", "");
            let _ = writeln!(
                b,
                "{:i$}if !self.finished {{ self.stack.push(Frame::{p}(f)); return ParseResult::NeedMoreData; }}",
                "",
                i = IND + 4
            );
            self.render_eof(&mut b, state, info, p, IND + 4, &home);
            let _ = writeln!(b, "{:IND$}}}", "");
        }

        if state.cases.len() == 1 && state.cases[0].is_default() {
            let cmds = state.cases[0].commands.clone();
            self.render_seq(&mut b, &cmds, info, p, IND, &SeqEnd::Redispatch, &home);
            return b;
        }

        let _ = writeln!(b, "{:IND$}match self.peek() {{", "");
        let mut saw_default = false;
        for case in &state.cases {
            if case.is_default() {
                saw_default = true;
            }
            self.render_case(&mut b, case, info, p, IND + 4, &home);
        }
        if !saw_default {
            let mut ret_b = String::new();
            self.render_seq(
                &mut ret_b,
                &[Command::new("return", serde_json::json!({}))],
                info,
                p,
                IND + 8,
                &SeqEnd::Redispatch,
                &home,
            );
            let _ = writeln!(b, "{:i$}_ => {{\n{ret_b}{:i$}}}", "", "", i = IND + 4);
        }
        let _ = writeln!(b, "{:IND$}}}", "");
        b
    }

    /// EOF behavior once `finish()`ed: explicit handlers, else the same
    /// type-driven inference as the recursive template's None-arm (content
    /// emit; Unclosed error for expects_char functions, End for brackets).
    fn render_eof(&mut self, b: &mut String, state: &State, info: &FnInfo<'i>, p: &str, ind: usize, home: &str) {
        let handler: Option<&Vec<Command>> = state
            .eof_handler
            .as_ref()
            .filter(|h| !h.is_empty())
            .or(info.func.eof_handler.as_ref().filter(|h| !h.is_empty()));
        if let Some(cmds) = handler {
            let cmds = cmds.clone();
            self.render_seq(b, &cmds, info, p, ind, &SeqEnd::Redispatch, home);
            return;
        }
        if info.kind == "content" {
            let t = info.func.return_type.as_deref().unwrap();
            let _ = writeln!(
                b,
                "{:ind$}{{ let (c, sp) = self.take_capture(); on_event(StreamEvent::{t} {{ content: c, span: sp }}); }}",
                ""
            );
        }
        if info.func.expects_char.is_some() {
            // Void expects_char functions produce the bare `Unclosed` code,
            // matching the recursive template's `Unclosed{return_type|dstr}`.
            let t = info.func.return_type.as_deref().unwrap_or("");
            let _ = writeln!(
                b,
                "{:ind$}on_event(StreamEvent::Error {{ code: ParseErrorCode::Unclosed{t}, span: self.gspan() }});",
                ""
            );
        } else if info.kind == "bracket" {
            let t = info.func.return_type.as_deref().unwrap();
            let _ = writeln!(b, "{:ind$}on_event(StreamEvent::{t}End {{ span: self.gspan() }});", "");
        }
        if info.kind == "internal" {
            let _ = writeln!(b, "{:ind$}self.ret = 0;", "");
        }
        let _ = writeln!(b, "{:ind$}continue 'run;", "");
    }

    fn render_case(&mut self, b: &mut String, case: &Case, info: &FnInfo<'i>, p: &str, ind: usize, home: &str) {
        let pat = if case.is_default() {
            "_".to_string()
        } else if let Some(cond) = &case.condition {
            format!("_ if {}", pd_expr(cond, &info.vars))
        } else if let Some(pr) = &case.param_ref {
            format!("Some(b) if b == f.{pr}")
        } else if let Some(class) = &case.special_class {
            let extra: String = case
                .chars
                .iter()
                .flatten()
                .map(|c| format!(" || b == {}", esc_char(c)))
                .collect();
            format!("Some(b) if is_{class}(b){extra}")
        } else if let Some(chars) = &case.chars {
            let pats: Vec<String> = chars.iter().map(|c| esc_char(c)).collect();
            format!("Some({})", pats.join(" | "))
        } else {
            "Some(_)".to_string()
        };
        let _ = writeln!(b, "{:ind$}{pat} => {{", "");
        let cmds = case.commands.clone();
        self.render_seq(b, &cmds, info, p, ind + 4, &SeqEnd::Redispatch, home);
        let _ = writeln!(b, "{:ind$}}}", "");
    }

    /// Apply a sequence end: either re-dispatch the current state or go to
    /// a continuation state.
    fn apply_end(&self, b: &mut String, end: &SeqEnd, p: &str, ind: usize) {
        match end {
            SeqEnd::Redispatch => {
                let _ = writeln!(b, "{:ind$}continue 'st;", "");
            }
            SeqEnd::Goto(st) => {
                let _ = writeln!(b, "{:ind$}f.st = {p}St::{st};", "");
                let _ = writeln!(b, "{:ind$}continue 'st;", "");
            }
        }
    }

    // ---- the heart: render a command sequence with continuation splitting
    fn render_seq(
        &mut self,
        b: &mut String,
        cmds: &[Command],
        info: &FnInfo<'i>,
        p: &str,
        ind: usize,
        end: &SeqEnd,
        home: &str,
    ) {
        for (i, cmd) in cmds.iter().enumerate() {
            let rest = &cmds[i + 1..];
            match cmd.ctype.as_str() {
                "advance" => {
                    let _ = writeln!(b, "{:ind$}self.advance_or_pend();", "");
                }
                "advance_to" => {
                    // Own state for idempotent chunk-boundary resume: the
                    // scan restarts from the current pos with mark intact.
                    let k_scan = self.fresh_cont();
                    let k_rest = self.fresh_cont();
                    let chars: Vec<String> = cmd
                        .arg_str("value")
                        .map(|v| v.chars().map(|c| esc_char(&c.to_string())).collect())
                        .unwrap_or_default();
                    let n = chars.len();
                    let mut kb = String::new();
                    let _ = writeln!(
                        kb,
                        "{:IND$}if self.scan_to{n}({args}).is_none() && !self.finished {{ self.stack.push(Frame::{p}(f)); return ParseResult::NeedMoreData; }}",
                        "",
                        args = chars.join(", ")
                    );
                    self.apply_end(&mut kb, &SeqEnd::Goto(k_rest.clone()), p, IND);
                    self.cont_states.push((k_scan.clone(), kb));
                    let mut rb = String::new();
                    self.render_seq(&mut rb, rest, info, p, IND, end, home);
                    self.cont_states.push((k_rest, rb));
                    self.apply_end(b, &SeqEnd::Goto(k_scan), p, ind);
                    return;
                }
                "mark" => {
                    let _ = writeln!(b, "{:ind$}self.mark();", "");
                }
                "keywords_try" => {
                    let var = cmd.arg_str("var").unwrap_or("");
                    let kw = cmd.arg_str("name").unwrap_or("");
                    let _ = writeln!(
                        b,
                        "{:ind$}f.{var} = if self.lookup_{kw}(on_event) {{ 1 }} else {{ 0 }};",
                        ""
                    );
                }
                "save" => {
                    let slot = cmd.arg_str("slot").unwrap_or("");
                    let _ = writeln!(
                        b,
                        "{:ind$}{{ let cap = self.save_capture(); self.saved.insert(\"{slot}\", cap); }}",
                        ""
                    );
                }
                "term" => {
                    let off = cmd.args.get("offset").and_then(|v| v.as_i64()).unwrap_or(0);
                    let _ = writeln!(b, "{:ind$}self.set_term({off});", "");
                }
                "prepend" => {
                    let lit = cmd.arg_str("literal").unwrap_or("");
                    let _ = writeln!(b, "{:ind$}self.prepend_bytes(b\"{}\");", "", esc_str(lit));
                }
                "prepend_param" => {
                    let pr = cmd.arg_str("param_ref").unwrap_or("");
                    let _ = writeln!(b, "{:ind$}self.prepend_bytes(f.{pr});", "");
                }
                "assign" | "add_assign" | "sub_assign" => {
                    let var = cmd.arg_str("var").unwrap_or("");
                    let expr = cmd.arg_str("expr").unwrap_or("0");
                    let op = match cmd.ctype.as_str() {
                        "add_assign" => "+=",
                        "sub_assign" => "-=",
                        _ => "=",
                    };
                    if let Some(caps) = re(r"^/(\w+)(?:\(([^)]*)\))?\s*$").captures(expr.trim()) {
                        assert_eq!(op, "=", "call-assign must be a plain assign");
                        let callee = caps.get(1).unwrap().as_str().to_string();
                        let args = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
                        self.split_call(b, &callee, &args, rest, info, p, ind, Some(var), end, home);
                        return;
                    }
                    let _ = writeln!(b, "{:ind$}f.{var} {op} {};", "", pd_expr(expr, &info.vars));
                }
                "call" => {
                    if cmd.args.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) {
                        let code = cmd
                            .arg_str("call_args")
                            .filter(|s| !s.is_empty())
                            .map(pascalcase)
                            .unwrap_or_else(|| "UnexpectedChar".to_string());
                        let _ = writeln!(
                            b,
                            "{:ind$}on_event(StreamEvent::Error {{ code: ParseErrorCode::{code}, span: self.gspan() }});",
                            ""
                        );
                        continue;
                    }
                    let callee = cmd.arg_str("name").unwrap_or("").to_string();
                    let args = cmd.arg_str("call_args").unwrap_or("").to_string();
                    self.split_call(b, &callee, &args, rest, info, p, ind, None, end, home);
                    return;
                }
                "keywords_lookup" => {
                    let kw = cmd.arg_str("name").unwrap_or("").to_string();
                    let kwd = self.ir.keywords.iter().find(|k| k.name == kw).cloned();
                    let k = self.fresh_cont();
                    let mut kb = String::new();
                    self.render_seq(&mut kb, rest, info, p, IND, end, home);
                    self.cont_states.push((k.clone(), kb));
                    let _ = writeln!(b, "{:ind$}f.st = {p}St::{k};", "");
                    let _ = writeln!(b, "{:ind$}let matched = self.lookup_{kw}(on_event);", "");
                    let _ = writeln!(b, "{:ind$}self.stack.push(Frame::{p}(f));", "");
                    if let Some(kwd) = kwd {
                        if let Some(fb) = &kwd.fallback_func {
                            // Corpus fallbacks are arg-less (emit_bare_value);
                            // frame-reading args would need pre-binding here.
                            let args = kwd.fallback_args.clone().unwrap_or_default();
                            assert!(args.trim().is_empty(), "keyword fallback args unsupported in pushdown backend");
                            let _ = writeln!(b, "{:ind$}if !matched {{", "");
                            let _ = writeln!(b, "{:i$}self.enter_{fb}(on_event);", "", i = ind + 4);
                            let _ = writeln!(b, "{:ind$}}}", "");
                        }
                    }
                    let _ = writeln!(b, "{:ind$}continue 'run;", "");
                    return;
                }
                "emit" | "inline_emit_bare" | "inline_emit_mark" | "inline_emit_literal" | "inline_emit_saved"
                | "inline_emit_param" => {
                    self.render_emit(b, cmd, ind);
                }
                "error" => {
                    let code = pascalcase(cmd.arg_str("value").unwrap_or("unexpected_char"));
                    let _ = writeln!(
                        b,
                        "{:ind$}on_event(StreamEvent::Error {{ code: ParseErrorCode::{code}, span: self.gspan() }});",
                        ""
                    );
                }
                "transition" => {
                    let target = cmd.arg_str("value").unwrap_or("").replace(':', "");
                    if target.is_empty() {
                        // `|>>` self-loop: the GRAMMAR state, which may not
                        // be the (continuation) state we're rendered into.
                        self.apply_end(b, &SeqEnd::Goto(home.to_string()), p, ind);
                    } else {
                        self.apply_end(b, &SeqEnd::Goto(pascalcase(&target)), p, ind);
                    }
                    return;
                }
                "return" => {
                    self.render_return(b, cmd, info, ind);
                    return;
                }
                "conditional" => {
                    // Rest (if any) becomes the fall-through continuation.
                    let clause_end: SeqEnd = if rest.is_empty() {
                        end.clone()
                    } else {
                        let k = self.fresh_cont();
                        let mut kb = String::new();
                        self.render_seq(&mut kb, rest, info, p, IND, end, home);
                        self.cont_states.push((k.clone(), kb));
                        SeqEnd::Goto(k)
                    };
                    let clauses = cmd.clauses.clone().unwrap_or_default();
                    let has_else = clauses.iter().any(|c| c.condition.is_none());
                    for (ci, clause) in clauses.iter().enumerate() {
                        let head = match (&clause.condition, ci) {
                            (Some(c), 0) => format!("if {} {{", pd_expr(c, &info.vars)),
                            (Some(c), _) => format!("}} else if {} {{", pd_expr(c, &info.vars)),
                            (None, _) => "} else {".to_string(),
                        };
                        let _ = writeln!(b, "{:ind$}{head}", "");
                        self.render_seq(b, &clause.commands, info, p, ind + 4, &clause_end, home);
                    }
                    let _ = writeln!(b, "{:ind$}}}", "");
                    if !has_else {
                        self.apply_end(b, &clause_end, p, ind);
                    }
                    return;
                }
                other => panic!("pushdown backend: unhandled command type '{other}'"),
            }
        }
        self.apply_end(b, end, p, ind);
    }

    #[allow(clippy::too_many_arguments)]
    fn split_call(
        &mut self,
        b: &mut String,
        callee: &str,
        args: &str,
        rest: &[Command],
        info: &FnInfo<'i>,
        p: &str,
        ind: usize,
        assign_ret_to: Option<&str>,
        end: &SeqEnd,
        home: &str,
    ) {
        let k = self.fresh_cont();
        let mut kb = String::new();
        if let Some(var) = assign_ret_to {
            let _ = writeln!(kb, "{:IND$}f.{var} = self.ret;", "");
        }
        self.render_seq(&mut kb, rest, info, p, IND, end, home);
        self.cont_states.push((k.clone(), kb));
        let callee_fn = self.ir.functions.iter().find(|x| x.name == callee);
        let call_args = render_call_args_typed(args, &info.vars, callee_fn);
        let _ = writeln!(b, "{:ind$}f.st = {p}St::{k};", "");
        if call_args.is_empty() {
            let _ = writeln!(b, "{:ind$}self.stack.push(Frame::{p}(f));", "");
            let _ = writeln!(b, "{:ind$}self.enter_{callee}(on_event);", "");
        } else {
            // Args may read frame fields: evaluate before the frame moves.
            let n = callee_fn.map(|c| c.params.len()).unwrap_or(0);
            let names: Vec<String> = (0..n).map(|i| format!("pd_a{i}")).collect();
            let _ = writeln!(
                b,
                "{:ind$}let ({binds},) = ({call_args});",
                "",
                binds = names.join(", ")
            );
            let _ = writeln!(b, "{:ind$}self.stack.push(Frame::{p}(f));", "");
            let _ = writeln!(
                b,
                "{:ind$}self.enter_{callee}({}on_event);",
                "",
                names.iter().map(|x| format!("{x}, ")).collect::<String>()
            );
        }
        let _ = writeln!(b, "{:ind$}continue 'run;", "");
    }

    fn render_emit(&mut self, b: &mut String, cmd: &Command, ind: usize) {
        match cmd.ctype.as_str() {
            "emit" => {
                let t = cmd.arg_str("value").unwrap_or("");
                let _ = writeln!(
                    b,
                    "{:ind$}{{ let (c, sp) = self.take_capture(); on_event(StreamEvent::{t} {{ content: c, span: sp }}); }}",
                    ""
                );
            }
            "inline_emit_bare" => {
                let t = cmd.arg_str("type").unwrap_or("");
                let _ = writeln!(
                    b,
                    "{:ind$}on_event(StreamEvent::{t} {{ content: std::borrow::Cow::Borrowed(&b\"\"[..]), span: self.gspan() }});",
                    ""
                );
            }
            "inline_emit_mark" => {
                let t = cmd.arg_str("type").unwrap_or("");
                let _ = writeln!(
                    b,
                    "{:ind$}{{ let (c, sp) = self.take_capture(); on_event(StreamEvent::{t} {{ content: c, span: sp }}); }}",
                    ""
                );
            }
            "inline_emit_saved" => {
                let t = cmd.arg_str("type").unwrap_or("");
                let slot = cmd.arg_str("slot").unwrap_or("");
                let _ = writeln!(
                    b,
                    "{:ind$}if let Some((c, sp)) = self.saved.get(\"{slot}\") {{ on_event(StreamEvent::{t} {{ content: std::borrow::Cow::Borrowed(&c[..]), span: sp.clone() }}); }}",
                    ""
                );
            }
            "inline_emit_param" => {
                let t = cmd.arg_str("type").unwrap_or("");
                let p = cmd.arg_str("param_ref").unwrap_or("");
                let _ = writeln!(
                    b,
                    "{:ind$}on_event(StreamEvent::{t} {{ content: std::borrow::Cow::Borrowed(f.{p}), span: self.gspan() }});",
                    ""
                );
            }
            "inline_emit_literal" => {
                let t = cmd.arg_str("type").unwrap_or("");
                let raw = cmd.arg_str("literal").unwrap_or("");
                let lit = if raw.starts_with('\'') { raw.trim_matches('\'') } else { raw };
                let _ = writeln!(
                    b,
                    "{:ind$}on_event(StreamEvent::{t} {{ content: std::borrow::Cow::Borrowed(&b\"{}\"[..]), span: self.gspan() }});",
                    "",
                    esc_str(lit)
                );
            }
            _ => unreachable!(),
        }
    }

    /// `return` command: mirrors `_command.j2`'s return semantics with a
    /// pop (the frame is simply not pushed back) instead of a Rust return.
    fn render_return(&mut self, b: &mut String, cmd: &Command, info: &FnInfo<'i>, ind: usize) {
        if let Some(rv) = cmd.arg_str("return_value") {
            let _ = writeln!(b, "{:ind$}self.ret = {};", "", pd_expr(rv, &info.vars));
            let _ = writeln!(b, "{:ind$}continue 'run;", "");
            return;
        }
        if let Some(t) = cmd.arg_str("emit_type") {
            match cmd.arg_str("emit_mode") {
                Some("mark") => {
                    let _ = writeln!(
                        b,
                        "{:ind$}{{ let (c, sp) = self.take_capture(); on_event(StreamEvent::{t} {{ content: c, span: sp }}); }}",
                        ""
                    );
                }
                Some("literal") => {
                    let lit = cmd.arg_str("literal").unwrap_or("");
                    let _ = writeln!(
                        b,
                        "{:ind$}on_event(StreamEvent::{t} {{ content: std::borrow::Cow::Borrowed(&b\"{}\"[..]), span: self.gspan() }});",
                        "",
                        esc_str(lit)
                    );
                }
                _ => {
                    let _ = writeln!(
                        b,
                        "{:ind$}on_event(StreamEvent::{t} {{ content: std::borrow::Cow::Borrowed(&b\"\"[..]), span: self.gspan() }});",
                        ""
                    );
                }
            }
            let _ = writeln!(b, "{:ind$}continue 'run;", "");
            return;
        }
        let suppress = cmd
            .args
            .get("suppress_auto_emit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        match info.kind {
            "internal" => {
                let _ = writeln!(b, "{:ind$}self.ret = 0;", "");
            }
            "content" if !suppress => {
                let t = info.func.return_type.as_deref().unwrap();
                let _ = writeln!(
                    b,
                    "{:ind$}{{ let (c, sp) = self.take_capture(); on_event(StreamEvent::{t} {{ content: c, span: sp }}); }}",
                    ""
                );
            }
            "bracket" => {
                let t = info.func.return_type.as_deref().unwrap();
                let _ = writeln!(b, "{:ind$}on_event(StreamEvent::{t}End {{ span: self.gspan() }});", "");
            }
            _ => {}
        }
        let _ = writeln!(b, "{:ind$}continue 'run;", "");
    }
}

// ---- small helpers ---------------------------------------------------------

fn init_expr(dsl: &str, func: &Function) -> String {
    // Initializers run inside `enter_<fn>` where params are in scope by
    // their own names — no frame prefix.
    let vars: BTreeSet<String> = func.params.iter().cloned().collect();
    pd_expr_prefixed(dsl, &vars, "")
}

fn pure_entry_inits(func: &Function) -> std::collections::BTreeMap<&str, &str> {
    let mut m = std::collections::BTreeMap::new();
    for cmd in &func.entry_actions {
        if cmd.ctype == "assign" {
            if let (Some(v), Some(e)) = (cmd.arg_str("var"), cmd.arg_str("expr")) {
                if !e.contains('/') && func.locals.iter().any(|l| l == v) {
                    m.insert(v, e);
                }
            }
        }
    }
    m
}

fn non_init_entry_actions(func: &Function, kind: &str) -> Vec<Command> {
    let inits = pure_entry_inits(func);
    func.entry_actions
        .iter()
        .filter(|c| {
            // CONTENT functions auto-mark in enter_ (mirror the template's
            // skip); void functions keep their explicit MARK.
            !(c.ctype == "mark" && kind == "content")
                && !(c.ctype == "assign" && c.arg_str("var").is_some_and(|v| inits.contains_key(v)))
        })
        .cloned()
        .collect()
}

/// Render DSL call args with frame addressing, converting each argument by
/// the CALLEE's parameter type (the recursive backend's
/// `transform_call_args_by_type`): Bytes params take byte-string literals
/// (`'``'`/`<>` -> `b"``"`/`b""`), Byte and I32 take the plain pipeline.
/// Trailing ", ".
fn render_call_args_typed(args: &str, vars: &BTreeSet<String>, callee: Option<&Function>) -> String {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = crate::ir_builder::tokenize_call_args(trimmed)
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            let a = raw.trim();
            let pty = callee.and_then(|c| c.param_types.get(i).map(|(_, t)| *t));
            if pty == Some(ParamType::Byte) && a == "<>" {
                // empty class as a Byte param means the zero byte
                return "0u8".to_string();
            }
            if pty == Some(ParamType::Bytes) {
                if let Some(stripped) = a.strip_prefix(':') {
                    return format!("f.{stripped}");
                }
                let parsed = crate::charclass::parse(a);
                let joined: String = parsed.chars.concat();
                return format!("b\"{}\"", esc_str(&joined));
            }
            pd_expr(a, vars)
        })
        .collect();
    format!("{}, ", parts.join(", "))
}

// ---- static runtime --------------------------------------------------------

const RUNTIME: &str = r#"// ---- byte-class matchers (free fns; mirror the recursive backend) ----

#[allow(dead_code)]
#[inline(always)]
fn is_letter(b: u8) -> bool {
    b.is_ascii_alphabetic()
}
#[allow(dead_code)]
#[inline(always)]
fn is_label_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}
#[allow(dead_code)]
#[inline(always)]
fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}
#[allow(dead_code)]
#[inline(always)]
fn is_hex_digit(b: u8) -> bool {
    b.is_ascii_hexdigit()
}
#[allow(dead_code)]
#[inline(always)]
fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t'
}
#[allow(dead_code)]
#[inline(always)]
fn is_nl(b: u8) -> bool {
    b == b'\n'
}
#[allow(dead_code)]
#[inline(always)]
fn is_xid_start(b: u8) -> bool {
    use unicode_xid::UnicodeXID;
    if b < 0x80 {
        (b as char).is_xid_start()
    } else {
        (0xC2..=0xF4).contains(&b)
    }
}
#[allow(dead_code)]
#[inline(always)]
fn is_xid_cont(b: u8) -> bool {
    use unicode_xid::UnicodeXID;
    if b < 0x80 {
        (b as char).is_xid_continue()
    } else {
        b >= 0x80
    }
}
#[allow(dead_code)]
#[inline(always)]
fn is_xlbl_start(b: u8) -> bool {
    is_xid_start(b)
}
#[allow(dead_code)]
#[inline(always)]
fn is_xlbl_cont(b: u8) -> bool {
    b == b'-' || is_xid_cont(b)
}

/// Resumable pushdown parser: feed bytes with `push_chunk`, close with
/// `finish`. Owns an accumulation buffer so capture (`mark..pos`) never
/// spans a seam; consumed bytes before the active mark are drained after
/// every run. Spans are global byte offsets.
pub struct PushdownParser {
    stack: Vec<Frame>,
    buf: Vec<u8>,
    /// Global offset of buf[0].
    base: usize,
    pos: usize,
    mark_pos: usize,
    mark_active: bool,
    term_pos: usize,
    prepend_buf: Vec<u8>,
    term_prepend_len: usize,
    pending_skip: u32,
    ret: i32,
    line: u32,
    column: u32,
    finished: bool,
    started: bool,
    /// SAVE(slot) captures — owned (content, global span) so a drain or
    /// chunk seam can never invalidate them. Re-emitted by
    /// TypeName(USE_SAVED(slot)).
    saved: std::collections::HashMap<&'static str, (Vec<u8>, std::ops::Range<usize>)>,
}

impl Default for PushdownParser {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unused_variables, dead_code)]
impl PushdownParser {
    pub fn new() -> Self {
        PushdownParser {
            stack: Vec::new(),
            buf: Vec::new(),
            base: 0,
            pos: 0,
            mark_pos: 0,
            mark_active: false,
            term_pos: usize::MAX,
            prepend_buf: Vec::new(),
            term_prepend_len: 0,
            saved: std::collections::HashMap::new(),
            pending_skip: 0,
            ret: 0,
            line: 1,
            column: 1,
            finished: false,
            started: false,
        }
    }

    /// Feed a chunk. Events fire for everything decidable so far.
    pub fn push_chunk<F>(&mut self, chunk: &[u8], on_event: &mut F) -> ParseResult
    where
        F: for<'e> FnMut(StreamEvent<'e>),
    {
        self.buf.extend_from_slice(chunk);
        if !self.started {
            self.started = true;
            self.enter___ENTRY__(on_event);
        }
        let r = self.run(on_event);
        self.drain_consumed();
        r
    }

    /// Close the stream: remaining structure runs its EOF behavior.
    pub fn finish<F>(mut self, on_event: &mut F)
    where
        F: for<'e> FnMut(StreamEvent<'e>),
    {
        self.finished = true;
        if !self.started {
            self.started = true;
            self.enter___ENTRY__(on_event);
        }
        let _ = self.run(on_event);
    }

    /// Drop consumed bytes that no capture can still reference.
    fn drain_consumed(&mut self) {
        let keep_from = if self.mark_active { self.mark_pos.min(self.pos) } else { self.pos };
        if keep_from == 0 {
            return;
        }
        self.buf.drain(..keep_from);
        self.base += keep_from;
        self.pos -= keep_from;
        if self.mark_active {
            self.mark_pos -= keep_from;
            if self.term_pos != usize::MAX {
                self.term_pos -= keep_from;
            }
        } else {
            self.mark_pos = 0;
            self.term_pos = usize::MAX;
        }
    }

    // ---- capture / position helpers (global-span variants) ----

    #[inline(always)]
    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    #[inline(always)]
    fn advance(&mut self) {
        if self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            if b == b'\n' {
                self.line += 1;
                self.column = 1;
            } else if b & 0xC0 != 0x80 {
                self.column += 1;
            }
            self.pos += 1;
        }
    }

    /// Mid-sequence advance that may land past the buffer end: the missing
    /// byte is consumed when it arrives (drained at the run-loop top).
    #[inline(always)]
    fn advance_or_pend(&mut self) {
        if self.pos < self.buf.len() {
            self.advance();
        } else if !self.finished {
            self.pending_skip += 1;
        }
    }

    #[inline(always)]
    fn mark(&mut self) {
        self.mark_pos = self.pos;
        self.mark_active = true;
        self.term_pos = usize::MAX;
        self.term_prepend_len = 0;
    }

    #[inline(always)]
    fn set_term(&mut self, offset: i32) {
        let new_pos = self.pos as i64 + offset as i64;
        self.term_pos = new_pos.clamp(0, self.buf.len() as i64) as usize;
    }

    #[inline(always)]
    fn prepend_bytes(&mut self, bytes: &[u8]) {
        self.prepend_buf.extend_from_slice(bytes);
    }

    /// Take the accumulated capture (prepend + mark..term) with its global
    /// span. Content is `Cow::Borrowed` straight out of the buffer when no
    /// PREPEND bytes are pending (the common case — zero-copy), and
    /// `Cow::Owned` only when prepend bytes must be combined. The borrow is
    /// safe against drains because `drain_consumed` runs only after `run`
    /// returns, and the borrow cannot outlive the event callback. The mark
    /// stays active for drain purposes until the next `mark()` — keyword
    /// fallback re-terms the same region, and a suspension between the two
    /// must not drain it.
    fn take_capture(&mut self) -> (std::borrow::Cow<'_, [u8]>, std::ops::Range<usize>) {
        let end = if self.term_pos != usize::MAX { self.term_pos } else { self.pos };
        let start = self.mark_pos.min(end);
        self.term_prepend_len = self.prepend_buf.len();
        // Span extends back over prepend-restored bytes in GLOBAL
        // coordinates: they may lie before the drained base.
        let span = ((self.base + self.mark_pos).saturating_sub(self.term_prepend_len))
            ..(self.base + end.max(self.mark_pos));
        if self.prepend_buf.is_empty() {
            (std::borrow::Cow::Borrowed(&self.buf[start..end]), span)
        } else {
            let mut combined = std::mem::take(&mut self.prepend_buf);
            combined.extend_from_slice(&self.buf[start..end]);
            (std::borrow::Cow::Owned(combined), span)
        }
    }

    /// Snapshot the current MARK..TERM capture for SAVE(slot) — owned copy,
    /// non-destructive (prepend buffer untouched).
    fn save_capture(&self) -> (Vec<u8>, std::ops::Range<usize>) {
        let end = if self.term_pos != usize::MAX { self.term_pos } else { self.pos };
        let content = self.buf[self.mark_pos.min(end)..end].to_vec();
        let span = (self.base + self.mark_pos)..(self.base + end.max(self.mark_pos));
        (content, span)
    }

    #[inline(always)]
    fn gspan(&self) -> std::ops::Range<usize> {
        (self.base + self.pos)..(self.base + self.pos)
    }

    #[inline(always)]
    fn col(&self) -> i32 {
        self.column as i32
    }

    #[inline(always)]
    fn prev(&self) -> u8 {
        if self.pos > 0 {
            self.buf[self.pos - 1]
        } else {
            0
        }
    }

    // ---- scan helpers (memchr over the live buffer) ----

    #[inline(always)]
    fn char_count(bytes: &[u8]) -> u32 {
        bytes.iter().filter(|&&b| b & 0xC0 != 0x80).count() as u32
    }

    fn scan_advance(&mut self, offset: Option<usize>) -> Option<u8> {
        match offset {
            Some(off) => {
                self.column += Self::char_count(&self.buf[self.pos..self.pos + off]);
                self.pos += off;
                Some(self.buf[self.pos])
            }
            None => {
                self.column += Self::char_count(&self.buf[self.pos..]);
                self.pos = self.buf.len();
                None
            }
        }
    }

    fn scan_to1(&mut self, b1: u8) -> Option<u8> {
        let r = memchr::memchr(b1, &self.buf[self.pos..]);
        self.scan_advance(r)
    }
    fn scan_to2(&mut self, b1: u8, b2: u8) -> Option<u8> {
        let r = memchr::memchr2(b1, b2, &self.buf[self.pos..]);
        self.scan_advance(r)
    }
    fn scan_to3(&mut self, b1: u8, b2: u8, b3: u8) -> Option<u8> {
        let r = memchr::memchr3(b1, b2, b3, &self.buf[self.pos..]);
        self.scan_advance(r)
    }
    fn scan_to4(&mut self, b1: u8, b2: u8, b3: u8, b4: u8) -> Option<u8> {
        let h = &self.buf[self.pos..];
        let p1 = memchr::memchr3(b1, b2, b3, h);
        let p2 = match p1 {
            Some(limit) => memchr::memchr(b4, &h[..limit]),
            None => memchr::memchr(b4, h),
        };
        let r = match (p1, p2) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        };
        self.scan_advance(r)
    }
    fn scan_to5(&mut self, b1: u8, b2: u8, b3: u8, b4: u8, b5: u8) -> Option<u8> {
        let h = &self.buf[self.pos..];
        let p1 = memchr::memchr3(b1, b2, b3, h);
        let p2 = match p1 {
            Some(limit) => memchr::memchr2(b4, b5, &h[..limit]),
            None => memchr::memchr2(b4, b5, h),
        };
        let r = match (p1, p2) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        };
        self.scan_advance(r)
    }
    fn scan_to6(&mut self, b1: u8, b2: u8, b3: u8, b4: u8, b5: u8, b6: u8) -> Option<u8> {
        let h = &self.buf[self.pos..];
        let p1 = memchr::memchr3(b1, b2, b3, h);
        let p2 = match p1 {
            Some(limit) => memchr::memchr3(b4, b5, b6, &h[..limit]),
            None => memchr::memchr3(b4, b5, b6, h),
        };
        let r = match (p1, p2) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        };
        self.scan_advance(r)
    }

"#;
