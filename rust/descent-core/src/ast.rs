//! Abstract Syntax Tree - direct parse result before semantic analysis.
//! Port of Ruby descent's `lib/descent/ast.rb`.

#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub name: Option<String>,
    pub entry_point: Option<String>,
    pub types: Vec<TypeDecl>,
    pub consts: Vec<ConstDecl>,
    pub functions: Vec<Function>,
    pub keywords: Vec<Keywords>,
}

/// Named integer constant: `|const[NAME] <int>`. NAME is SCREAMING_CASE and
/// usable wherever an integer expression is (assignments, conditions, call
/// args, `|return NAME`); substituted textually before IR building so every
/// downstream analysis just sees the number.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub name: String,
    pub value: i64,
    pub lineno: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    /// Upcased kind word (e.g. "BRACKET", "CONTENT", "INTERNAL", "UNKNOWN").
    pub kind: String,
    pub lineno: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub return_type: Option<String>,
    pub params: Vec<String>,
    pub states: Vec<State>,
    pub eof_handler: Option<EOFHandler>,
    pub entry_actions: Vec<Command>,
    pub lineno: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub name: String,
    pub cases: Vec<Case>,
    pub eof_handler: Option<EOFHandler>,
    pub lineno: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub chars: Option<String>,
    pub condition: Option<String>,
    pub substate: Option<String>,
    pub commands: Vec<Command>,
    pub lineno: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EOFHandler {
    pub commands: Vec<Command>,
    pub lineno: usize,
}

/// A command is either a simple typed command or a function-level conditional
/// (Ruby models the latter as a distinct AST::Conditional node that can appear
/// anywhere a command can).
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Cmd { kind: CmdKind, lineno: usize },
    Conditional { clauses: Vec<Clause>, lineno: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub condition: Option<String>,
    pub commands: Vec<Command>,
}

/// Command types with their raw (string-level) values, mirroring Ruby's
/// `[type, value]` pairs from `classify_command`.
#[derive(Debug, Clone, PartialEq)]
pub enum CmdKind {
    Advance,
    AdvanceTo(String),
    Transition(String),
    Return(String),
    Error(String),
    Mark,
    /// TERM with offset (Ruby: value nil => handled as 0 by IR builder)
    Term(Option<i64>),
    Emit(Option<String>),
    Call(String),
    CallMethod(String),
    Scan(String),
    KeywordsLookup(String),
    Prepend(String),
    PrependParam(String),
    Assign { var: String, expr: String },
    AddAssign { var: String, expr: String },
    SubAssign { var: String, expr: String },
    InlineEmitMark(String),
    /// SAVE(slot): snapshot the current MARK..TERM capture into a named
    /// parser-global slot, re-emittable later via TypeName(USE_SAVED(slot)).
    /// Motivating use: UDON's flat attribute wire re-emits the attribute
    /// key for every value segment (multi-line text, warn+stack ingestion).
    Save(String),
    /// `var = KEYWORDS(map)`: try the keyword lookup (emit on match), store
    /// 1/0 in var, never call a fallback — lets the grammar branch itself.
    KeywordsTry { var: String, name: String },
    /// TypeName(USE_SAVED(slot)): emit an event whose payload is the saved
    /// capture — content and span both come from the slot.
    InlineEmitSaved { ty: String, slot: String },
    InlineEmitLiteral { ty: String, literal: String },
    InlineEmitBare(String),
    Noop,
}

impl Command {
    pub fn lineno(&self) -> usize {
        match self {
            Command::Cmd { lineno, .. } => *lineno,
            Command::Conditional { lineno, .. } => *lineno,
        }
    }

    pub fn is_return(&self) -> bool {
        matches!(self, Command::Cmd { kind: CmdKind::Return(_), .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keywords {
    pub name: String,
    pub fallback: Option<String>,
    pub mappings: Vec<KeywordMapping>,
    pub lineno: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeywordMapping {
    pub keyword: String,
    pub event_type: String,
}
