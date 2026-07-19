//! Intermediate Representation - semantic model after analysis.
//! Port of Ruby descent's `lib/descent/ir.rb`, with one deliberate
//! architectural divergence (the January fix): the IR is **target-neutral**.
//! Ruby's IRBuilder bakes Rust byte literals into call args
//! (`transform_call_args_by_type`) and pre-escapes prepend values; here those
//! renderings happen in the emitter's context builder (`emit::rust`), and the
//! IR keeps DSL-level facts: chars as chars, conditions/exprs/call-args as
//! raw DSL strings, prepend values as unescaped bytes.

use serde_json::Value;

/// Top-level parser definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ParserIR {
    pub name: Option<String>,
    pub entry_point: Option<String>,
    pub types: Vec<TypeInfo>,
    pub functions: Vec<Function>,
    pub keywords: Vec<Keywords>,
    pub custom_error_codes: Vec<String>,
}

/// Keywords for perfect hash (phf) lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct Keywords {
    pub name: String,
    pub fallback_func: Option<String>,
    pub fallback_args: Option<String>,
    pub mappings: Vec<crate::ast::KeywordMapping>,
    pub lineno: usize,
}

/// Resolved type information. `kind` is the downcased kind word
/// ("bracket" | "content" | "internal" | "unknown").
#[derive(Debug, Clone, PartialEq)]
pub struct TypeInfo {
    pub name: String,
    pub kind: String,
    pub emits_start: bool,
    pub emits_end: bool,
    pub lineno: usize,
}

impl TypeInfo {
    pub fn is_bracket(&self) -> bool {
        self.kind == "bracket"
    }
    pub fn is_content(&self) -> bool {
        self.kind == "content"
    }
}

/// Inferred parameter type. Target-neutral: the emitter renders these into
/// concrete types (u8 / &'static [u8] / i32 for Rust).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    I32,
    Byte,
    Bytes,
}

impl ParamType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ParamType::I32 => "i32",
            ParamType::Byte => "byte",
            ParamType::Bytes => "bytes",
        }
    }
}

/// Function with resolved semantics.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub return_type: Option<String>,
    pub params: Vec<String>,
    /// Param name -> inferred type, in `params` order (Ruby: insertion-ordered Hash).
    pub param_types: Vec<(String, ParamType)>,
    /// Local variable names in first-seen order (Ruby maps every local to :i32).
    pub locals: Vec<String>,
    pub states: Vec<State>,
    pub eof_handler: Option<Vec<Command>>,
    pub entry_actions: Vec<Command>,
    /// `None` mirrors a Ruby nil (return type missing from the type table);
    /// serialized as JSON null, not false.
    pub emits_events: Option<bool>,
    /// Single char (as string) that must be seen to return.
    pub expects_char: Option<String>,
    /// Full `Unclosed*` warning code when this function is classified
    /// **delimited** (positional/delimited classification pass) and carries a
    /// return type. Drives the generated force-unwind warning at EOF —
    /// distinct from `expects_char`, which only sees single-literal closers.
    /// `None` for positional functions and for delimited ones whose closer
    /// `expects_char` already handles.
    pub delimited_code: Option<String>,
    pub emits_content_on_close: bool,
    /// Param name -> sorted byte values passed at call sites (neutral,
    /// unescaped — Ruby stores these pre-Rust-escaped; see module docs).
    pub prepend_values: Vec<(String, Vec<String>)>,
    pub lineno: usize,
}

impl Function {
    pub fn param_type(&self, param: &str) -> Option<ParamType> {
        self.param_types
            .iter()
            .find(|(p, _)| p == param)
            .map(|(_, t)| *t)
    }
}

/// State with inferred optimizations.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub name: String,
    pub cases: Vec<Case>,
    pub eof_handler: Option<Vec<Command>>,
    /// Chars (each one character) for SIMD memchr scan, or None.
    pub scan_chars: Option<Vec<String>>,
    /// Byte-parameter names that join the scan set at runtime (memchr takes
    /// runtime needles for free) — from `|c[:param]` cases in a scannable
    /// state.
    pub scan_params: Vec<String>,
    pub is_self_looping: bool,
    pub has_default: bool,
    pub is_unconditional: bool,
    /// True if '\n' was injected into scan_chars by the builder.
    pub newline_injected: bool,
    pub lineno: usize,
}

/// True if this command sequence returns (recursing into conditional clauses).
/// Shared EOF-derivation helper — used by BOTH backends so they stay in lockstep.
pub fn commands_return(cmds: &[Command]) -> bool {
    cmds.iter().any(|c| {
        c.ctype == "return"
            || c.clauses.as_ref().is_some_and(|cl| cl.iter().any(|cl| commands_return(&cl.commands)))
    })
}

impl State {
    pub fn scannable(&self) -> bool {
        self.scan_chars.as_ref().is_some_and(|c| !c.is_empty()) || !self.scan_params.is_empty()
    }

    /// Total scan arity: static chars + runtime byte params.
    pub fn scan_arity(&self) -> usize {
        self.scan_chars.as_ref().map_or(0, |c| c.len()) + self.scan_params.len()
    }

    fn has_newline_case(&self) -> bool {
        self.cases.iter().any(|c| c.matches_newline())
    }

    /// The `\n` case that RETURNS (a leaf terminal) — the safe target for
    /// "EOF reuses its newline arm". `None` for loop states (whose `\n`
    /// continues) so they keep the clean dedent-return at EOF.
    pub fn newline_return_case(&self) -> Option<&Case> {
        self.cases.iter().find(|c| c.is_newline_return())
    }

    pub fn default_case(&self) -> Option<&Case> {
        self.cases.iter().find(|c| c.is_default())
    }

    /// EOF ≡ newline: a state with no explicit `|eof` whose `\n` arm returns
    /// runs that arm at EOF. (Both backends.)
    pub fn eof_run_newline(&self) -> bool {
        self.eof_handler.is_none() && self.newline_return_case().is_some()
    }

    /// EOF ≡ newline + dedent for a lookahead state: no `|eof`, no `\n` case,
    /// a non-self-looping `default` — run that fall-through `default` at EOF
    /// (chains to the state that emits) instead of the type-default that would
    /// drop accumulated content. (Both backends.)
    pub fn eof_run_default(&self) -> bool {
        self.eof_handler.is_none()
            && !self.is_self_looping
            && !self.has_newline_case()
            && self.has_default
    }
}

/// Case with resolved actions.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// Literal chars to match (each one character), or None for default.
    pub chars: Option<Vec<String>>,
    /// Special class name like "letter", "xid_start" for runtime matchers.
    pub special_class: Option<String>,
    /// Parameter name to match against (for `|c[:param]|`), or None.
    pub param_ref: Option<String>,
    /// Raw DSL condition string for if-cases, or None.
    pub condition: Option<String>,
    pub substate: Option<String>,
    pub commands: Vec<Command>,
    pub lineno: usize,
}

impl Case {
    pub fn is_default(&self) -> bool {
        self.chars.is_none()
            && self.special_class.is_none()
            && self.param_ref.is_none()
            && self.condition.is_none()
    }
    pub fn is_conditional(&self) -> bool {
        self.condition.is_some()
    }
    /// Matches a `\n` (possibly among other chars).
    pub fn matches_newline(&self) -> bool {
        self.chars.as_deref().is_some_and(|cs| cs.iter().any(|x| x == "\n"))
    }
    /// A `\n` case whose commands RETURN — the EOF-reuse target (see State).
    pub fn is_newline_return(&self) -> bool {
        self.matches_newline() && commands_return(&self.commands)
    }
}

/// Resolved command. `ctype` matches Ruby's command-type symbol name
/// ("mark", "term", "call", "conditional", ...). `args` is the Ruby
/// args-hash as a JSON object with **raw DSL values** (no target literals);
/// conditional clauses live in `clauses` (Ruby nests real Command objects
/// inside the args hash — we hoist them for typed traversal and re-nest at
/// serialization time).
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub ctype: String,
    pub args: Value,
    pub clauses: Option<Vec<Clause>>,
}

impl Command {
    pub fn new(ctype: &str, args: Value) -> Self {
        Command { ctype: ctype.to_string(), args, clauses: None }
    }

    pub fn conditional(clauses: Vec<Clause>) -> Self {
        Command {
            ctype: "conditional".to_string(),
            args: serde_json::json!({}),
            clauses: Some(clauses),
        }
    }

    /// String-valued arg accessor.
    pub fn arg_str(&self, key: &str) -> Option<&str> {
        self.args.get(key).and_then(|v| v.as_str())
    }
}

/// Conditional clause (condition None = else).
#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub condition: Option<String>,
    pub commands: Vec<Command>,
}
