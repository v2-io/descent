//! Builds AST from the token stream. Port of `lib/descent/parser.rb`.

use crate::ast::*;
use crate::lexer::{re, Token};

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ParseError {}

const STRUCTURAL: &[&str] = &["function", "type", "state", "keywords"];
const CASE_KEYWORDS: &[&str] = &["c", "default", "eof", "if"];
const CHAR_CLASSES: &[&str] = &[
    "letter", "label_cont", "digit", "hex_digit", "ws", "nl", "xid_start", "xid_cont",
    "xlbl_start", "xlbl_cont",
];

fn is_case_starter(tag: &str) -> bool {
    STRUCTURAL.contains(&tag) || CASE_KEYWORDS.contains(&tag) || CHAR_CLASSES.contains(&tag)
}

/// Detect if a token tag looks like a command (not a case starter).
fn command_like(tag: &str) -> bool {
    if tag.is_empty() {
        return false;
    }
    if tag.starts_with('/') || tag.starts_with("->") || tag.starts_with(">>") {
        return true;
    }
    if tag.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return true;
    }
    let base_tag = tag.to_lowercase();
    let base_tag = base_tag.split('(').next().unwrap_or("");
    ["return", "err", "mark", "term"].contains(&base_tag)
}

fn inline_command_token(token: &Token) -> bool {
    if command_like(&token.tag) {
        return true;
    }
    // Assignment: rest starts with =, += or -=
    re(r"^\s*[+-]?=").is_match(&token.rest)
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    pub fn parse(&mut self) -> Result<Machine, ParseError> {
        let mut name = None;
        let mut entry_point = None;
        let mut types = Vec::new();
        let mut functions = Vec::new();
        let mut keywords = Vec::new();

        while let Some(token) = self.current() {
            match token.tag.as_str() {
                "parser" => {
                    name = Some(token.rest.trim().to_lowercase());
                    self.advance();
                }
                "entry-point" => {
                    entry_point = Some(token.rest.trim().to_string());
                    self.advance();
                }
                "type" => types.push(self.parse_type()),
                "function" => functions.push(self.parse_function()?),
                "keywords" => keywords.push(self.parse_keywords()?),
                _ => {
                    return Err(ParseError(format!(
                        "Line {}: Unknown top-level declaration '{}'. Expected: parser, entry-point, type, function, or keywords",
                        token.lineno, token.tag
                    )))
                }
            }
        }

        Ok(Machine { name, entry_point, types, functions, keywords })
    }

    fn parse_type(&mut self) -> TypeDecl {
        let token = self.current().unwrap().clone();
        let kind = token
            .rest
            .split_whitespace()
            .next()
            .map(|w| w.to_uppercase())
            .unwrap_or_else(|| "UNKNOWN".to_string());
        self.advance();
        TypeDecl { name: token.id, kind, lineno: token.lineno }
    }

    fn parse_keywords(&mut self) -> Result<Keywords, ParseError> {
        let token = self.current().unwrap().clone();
        let name = token.id;
        let rest = token.rest;
        let lineno = token.lineno;
        self.advance();

        let mut fallback: Option<String> = None;
        if let Some(c) = re(r"(?s):fallback\s+(/\w+(?:\([^)]*\))?)").captures(&rest) {
            fallback = Some(c[1].to_string());
        } else if let Some(c) = re(r"^(/\w+(?:\([^)]*\))?)").captures(&rest) {
            fallback = Some(c[1].to_string());
        }

        let mut mappings = Vec::new();
        while let Some(t) = self.current() {
            if STRUCTURAL.contains(&t.tag.as_str()) || t.tag.starts_with('/') {
                break;
            }
            let t = t.clone();
            if t.tag.is_empty() && t.rest.contains("=>") {
                let mut it = t.rest.splitn(2, "=>");
                let keyword = it.next().unwrap_or("").trim().to_string();
                let event_type = it.next().unwrap_or("").trim().to_string();
                if !keyword.is_empty() && !event_type.is_empty() {
                    mappings.push(KeywordMapping { keyword, event_type });
                }
                self.advance();
            } else if let Some(c) = re(r"^=>\s*(\w+)").captures(&t.rest) {
                mappings.push(KeywordMapping {
                    keyword: t.tag.trim().to_string(),
                    event_type: c[1].to_string(),
                });
                self.advance();
            } else {
                return Err(ParseError(format!(
                    "Line {}: Unknown keyword mapping format: '{}' rest='{}'",
                    t.lineno, t.tag, t.rest
                )));
            }
        }

        Ok(Keywords { name, fallback, mappings, lineno })
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let token = self.current().unwrap().clone();
        // Quirk mirror: Ruby `name, rtype = id.split(':')` silently DROPS any
        // third-and-later colon segment ("a:b:c" -> rtype "b"). Ledgered.
        let mut split = token.id.split(':');
        let name = split.next().unwrap_or("").to_string();
        let rtype = split.next().map(|s| s.to_string());
        let params: Vec<String> = re(r":(\w+)")
            .captures_iter(&token.rest)
            .map(|c| c[1].to_string())
            .collect();
        let lineno = token.lineno;
        self.advance();

        let mut states = Vec::new();
        let mut eof_handler = None;
        let mut entry_actions = Vec::new();

        while let Some(t) = self.current() {
            if ["function", "type", "keywords"].contains(&t.tag.as_str()) {
                break;
            }
            match t.tag.as_str() {
                "state" => states.push(self.parse_state()?),
                "eof" => eof_handler = Some(self.parse_eof_handler()?),
                "if" => entry_actions.push(self.parse_conditional()?),
                _ => {
                    let t = t.clone();
                    if inline_command_token(&t) {
                        entry_actions.push(parse_command(&t)?);
                        self.advance();
                    } else {
                        return Err(ParseError(format!(
                            "Line {}: Unexpected token '{}' inside function. Expected: state, eof, if, or inline command (like 'var = expr' or 'MARK')",
                            t.lineno, t.tag
                        )));
                    }
                }
            }
        }

        Ok(Function {
            name: name.replace('-', "_"),
            return_type: rtype,
            params,
            states,
            eof_handler,
            entry_actions,
            lineno,
        })
    }

    fn parse_state(&mut self) -> Result<State, ParseError> {
        let token = self.current().unwrap().clone();
        let name = token.id.replace('-', "_").replace(':', "");
        let lineno = token.lineno;
        self.advance();

        let mut cases = Vec::new();
        let mut eof_handler = None;

        while let Some(t) = self.current() {
            if STRUCTURAL.contains(&t.tag.as_str()) {
                break;
            }
            let tag = t.tag.clone();
            match tag.as_str() {
                "c" => {
                    let id = t.id.clone();
                    cases.push(self.parse_case(Some(id))?);
                }
                "default" => cases.push(self.parse_case(None)?),
                "eof" => eof_handler = Some(self.parse_eof_handler()?),
                "if" => cases.push(self.parse_if_case()?),
                _ => {
                    if CHAR_CLASSES.contains(&tag.as_str()) {
                        cases.push(self.parse_case(Some(tag.to_uppercase()))?);
                    } else if command_like(&tag) {
                        cases.push(self.parse_bare_action_case()?);
                    } else {
                        return Err(ParseError(format!(
                            "Line {}: Unknown token in state: '{}' (not a case starter or command)",
                            t.lineno, tag
                        )));
                    }
                }
            }
        }

        Ok(State { name, cases, eof_handler, lineno })
    }

    fn parse_case(&mut self, chars_str: Option<String>) -> Result<Case, ParseError> {
        let token = self.current().unwrap().clone();
        let lineno = token.lineno;
        self.advance();

        let mut substate = None;
        let mut commands = Vec::new();

        while let Some(t) = self.current() {
            if is_case_starter(&t.tag) {
                break;
            }
            let t = t.clone();
            if t.tag == "." {
                substate = Some(t.rest.trim().to_string());
            } else {
                commands.push(parse_command(&t)?);
            }
            self.advance();
        }

        Ok(Case { chars: chars_str, condition: None, substate, commands, lineno })
    }

    /// A bare action case starts with a command; the current token IS the first command.
    fn parse_bare_action_case(&mut self) -> Result<Case, ParseError> {
        let lineno = self.current().unwrap().lineno;

        let mut substate = None;
        let mut commands = Vec::new();

        while let Some(t) = self.current() {
            if is_case_starter(&t.tag) {
                break;
            }
            let t = t.clone();
            if t.tag == "." {
                substate = Some(t.rest.trim().to_string());
            } else {
                commands.push(parse_command(&t)?);
            }
            self.advance();
        }

        Ok(Case { chars: None, condition: None, substate, commands, lineno })
    }

    fn parse_if_case(&mut self) -> Result<Case, ParseError> {
        let token = self.current().unwrap().clone();
        let lineno = token.lineno;
        let condition = token.id;
        self.advance();

        let mut commands: Vec<Command> = Vec::new();
        let mut last_was_return = false;

        while let Some(t) = self.current() {
            if is_case_starter(&t.tag) {
                break;
            }
            // After return, any command-like token starts a new (bare action) case.
            if last_was_return && command_like(&t.tag) {
                break;
            }
            let t = t.clone();
            if t.tag != "." {
                let cmd = parse_command(&t)?;
                last_was_return = cmd.is_return();
                commands.push(cmd);
            }
            self.advance();
        }

        Ok(Case { chars: None, condition: Some(condition), substate: None, commands, lineno })
    }

    fn parse_eof_handler(&mut self) -> Result<EOFHandler, ParseError> {
        let lineno = self.current().unwrap().lineno;
        self.advance();

        let mut commands = Vec::new();
        while let Some(t) = self.current() {
            if is_case_starter(&t.tag) {
                break;
            }
            let t = t.clone();
            if t.tag != "." {
                commands.push(parse_command(&t)?);
            }
            self.advance();
        }

        Ok(EOFHandler { commands, lineno })
    }

    fn parse_conditional(&mut self) -> Result<Command, ParseError> {
        let token = self.current().unwrap().clone();
        let lineno = token.lineno;
        let mut clauses: Vec<Clause> = Vec::new();

        let mut current_condition: Option<String> = Some(token.id);
        let mut current_commands: Vec<Command> = Vec::new();
        self.advance();

        while let Some(t) = self.current() {
            match t.tag.as_str() {
                "elsif" => {
                    let id = t.id.clone();
                    clauses.push(Clause { condition: current_condition.take(), commands: std::mem::take(&mut current_commands) });
                    current_condition = Some(id);
                    self.advance();
                }
                "else" => {
                    clauses.push(Clause { condition: current_condition.take(), commands: std::mem::take(&mut current_commands) });
                    current_condition = None;
                    self.advance();
                }
                "endif" => {
                    clauses.push(Clause { condition: current_condition.take(), commands: std::mem::take(&mut current_commands) });
                    self.advance();
                    return Ok(Command::Conditional { clauses, lineno });
                }
                "function" | "type" | "state" | "c" | "default" | "eof" => {
                    // Implicit endif
                    clauses.push(Clause { condition: current_condition.take(), commands: std::mem::take(&mut current_commands) });
                    return Ok(Command::Conditional { clauses, lineno });
                }
                _ => {
                    let t = t.clone();
                    current_commands.push(parse_command(&t)?);
                    self.advance();
                }
            }
        }
        // Token stream ended without endif (Ruby's loop just exits; the last
        // clause is lost there too — Ruby only pushes on explicit tags. Mirror
        // that: fall out with what we have.)
        Ok(Command::Conditional { clauses, lineno })
    }
}

fn parse_command(token: &Token) -> Result<Command, ParseError> {
    let (kind, lineno) = classify_command(token)?;
    Ok(Command::Cmd { kind, lineno })
}

fn classify_command(token: &Token) -> Result<(CmdKind, usize), ParseError> {
    let tag = token.tag.as_str();
    let rest = token.rest.as_str();
    let lineno = token.lineno;

    let kind = if tag.is_empty() {
        parse_inline_command(rest)?
    } else if tag == "->" {
        if token.id.is_empty() {
            CmdKind::Advance
        } else {
            CmdKind::AdvanceTo(token.id.clone())
        }
    } else if tag == ">>" {
        CmdKind::Transition(rest.trim().to_string())
    } else if tag == "return" {
        CmdKind::Return(rest.trim().to_string())
    } else if tag == "err" {
        CmdKind::Error(rest.trim().to_string())
    } else if tag == "mark" {
        CmdKind::Mark
    } else if tag == "term" {
        CmdKind::Term(None)
    } else if re(r"(?i)^emit\(").is_match(tag) {
        let v = re(r"(?i)emit\(([^)]+)\)").captures(tag).map(|c| c[1].to_string());
        CmdKind::Emit(v)
    } else if re(r"^/\w").is_match(tag) {
        let value = if rest.is_empty() {
            tag[1..].to_string()
        } else {
            format!("{}({})", &tag[1..], rest)
        };
        CmdKind::Call(value)
    } else if let Some(c) = re(r"(?i)^TERM\((-?\d+)\)$").captures(tag) {
        CmdKind::Term(Some(c[1].parse().unwrap()))
    } else if re(r"(?i)^TERM$").is_match(tag) {
        CmdKind::Term(Some(0))
    } else if re(r"(?i)^MARK$").is_match(tag) {
        CmdKind::Mark
    } else if let Some(c) = re(r"(?i)^KEYWORDS\((\w+)\)$").captures(tag) {
        CmdKind::KeywordsLookup(c[1].to_string())
    } else if let Some(c) = re(r"(?i)^PREPEND\(([^)]*)\)$").captures(tag) {
        prepend_kind(&c[1])
    } else if let Some(c) = re(r"^([A-Z]\w*)\(USE_MARK\)$").captures(tag) {
        CmdKind::InlineEmitMark(c[1].to_string())
    } else if let Some(c) = re(r"^([A-Z]\w*)\(([^)]+)\)$").captures(tag) {
        CmdKind::InlineEmitLiteral { ty: c[1].to_string(), literal: c[2].to_string() }
    } else if let Some(c) = re(r"^([A-Z]\w*)$").captures(tag) {
        CmdKind::InlineEmitBare(c[1].to_string())
    } else {
        // Maybe tag + rest forms an assignment (e.g. tag="depth", rest="= 1")
        let full_cmd = format!("{} {}", tag, rest);
        parse_inline_command(full_cmd.trim())?
    };

    Ok((kind, lineno))
}

fn prepend_kind(content: &str) -> CmdKind {
    let content = content.trim();
    if content.is_empty() {
        CmdKind::Noop
    } else if let Some(stripped) = content.strip_prefix(':') {
        CmdKind::PrependParam(stripped.to_string())
    } else {
        CmdKind::Prepend(content.to_string())
    }
}

fn parse_inline_command(cmd: &str) -> Result<CmdKind, ParseError> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Ok(CmdKind::Noop);
    }

    let kind = if re(r"(?i)^MARK\b").is_match(cmd) {
        CmdKind::Mark
    } else if let Some(c) = re(r"(?i)^TERM\((-?\d+)\)").captures(cmd) {
        CmdKind::Term(Some(c[1].parse().unwrap()))
    } else if re(r"(?i)^TERM\b").is_match(cmd) {
        CmdKind::Term(Some(0))
    } else if let Some(c) = re(r"(?i)^KEYWORDS\((\w+)\)").captures(cmd) {
        CmdKind::KeywordsLookup(c[1].to_string())
    } else if let Some(c) = re(r"(?i)^PREPEND\(([^)]*)\)").captures(cmd) {
        prepend_kind(&c[1])
    } else if let Some(c) = re(r"(?is)^return\b\s*(.*)$").captures(cmd) {
        CmdKind::Return(c[1].trim().to_string())
    } else if re(r"^->\s*$").is_match(cmd) {
        CmdKind::Advance
    } else if let Some(c) = re(r"^->\s*\[([^\]]+)\]$").captures(cmd) {
        CmdKind::AdvanceTo(c[1].to_string())
    } else if let Some(c) = re(r"(?i)^emit\(([^)]+)\)").captures(cmd) {
        CmdKind::Emit(Some(c[1].to_string()))
    } else if let Some(c) = re(r"(?i)^CALL:(\w+)").captures(cmd) {
        CmdKind::CallMethod(c[1].to_string())
    } else if let Some(c) = re(r"(?i)^SCAN\(([^)]+)\)").captures(cmd) {
        CmdKind::Scan(c[1].to_string())
    } else if let Some(c) = re(r"^/(\w+)").captures(cmd) {
        // NOTE (quirk, mirrored from Ruby): inline /call captures the name
        // only — any arguments are silently dropped.
        CmdKind::Call(c[1].to_string())
    } else if let Some(c) = re(r"(?s)^(\w+)\s*\+=\s*(.+)$").captures(cmd) {
        CmdKind::AddAssign { var: c[1].to_string(), expr: c[2].to_string() }
    } else if let Some(c) = re(r"(?s)^(\w+)\s*-=\s*(.+)$").captures(cmd) {
        CmdKind::SubAssign { var: c[1].to_string(), expr: c[2].to_string() }
    } else if let Some(c) = re(r"(?s)^(\w+)\s*=\s*(.+)$").captures(cmd) {
        CmdKind::Assign { var: c[1].to_string(), expr: c[2].to_string() }
    } else if let Some(c) = re(r"^([A-Z]\w*)\(USE_MARK\)$").captures(cmd) {
        CmdKind::InlineEmitMark(c[1].to_string())
    } else if let Some(c) = re(r"^([A-Z]\w*)\(([^)]+)\)$").captures(cmd) {
        CmdKind::InlineEmitLiteral { ty: c[1].to_string(), literal: c[2].to_string() }
    } else if let Some(c) = re(r"^([A-Z]\w*)$").captures(cmd) {
        CmdKind::InlineEmitBare(c[1].to_string())
    } else {
        return Err(ParseError(format!(
            "Unrecognized command: '{}'. Expected: MARK, TERM, PREPEND, return, ->, /call, assignment, or TypeName",
            cmd
        )));
    };
    Ok(kind)
}
