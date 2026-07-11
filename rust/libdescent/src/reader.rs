//! Production front-end: udon-core events -> descent `Token`s.
//!
//! Promoted from rust/spikes/udon-reader (session 4) after 10/10
//! token-identity vs the oracle lexer across the fixture corpus; the
//! hand-ported `lexer` module remains the differential oracle and fallback
//! (`Frontend::OracleLexer`). Spike metrics/probes stay in the spike dir.
//!
//! Strategy (PROGRESS.md "Front-end plan"): reconstruct descent *parts* (the
//! strings Ruby's pipe-splitter produces) from UDON events, then feed each
//! part through `libdescent::lexer::parse_part` — the shared layer-3
//! decomposition — so all tag-casing/id/rest quirks live in one place. The
//! reader replaces only layers 1-2 (comment strip + pipe split).
//!
//! Reconstruction rules (empirical, from probes on the fixture corpus):
//! - ElementStart begins a new part; Name appends the element name.
//! - The positional id attr (`Attr("id")` with zero-width span) + its value
//!   becomes `[<content>]` (udon preserves quotes inside bracket-ids).
//! - A class attr on a nameless element is descent's substate: `.<content>`.
//! - Any other sameline attr is re-serialized from the RAW SOURCE SLICE
//!   spanning `:` through the value end (never value-interpreted).
//! - Text runs are split with the same quote/bracket-aware pipe splitter the
//!   oracle lexer uses: piece 0 continues the last open part (Ruby semantics:
//!   a pipe *terminates* the previous part), later pieces open new parts.
//! - Comments are dropped; BlankLine ignored.
//!
//! Known udon-core irregularity (ledgered): Text spans on pipe-leading runs
//! are one byte short of `content.len()`; we therefore trust `span.start` +
//! `content` and treat span ends as estimates.

use crate::lexer::{parse_part, LexerError, Token};
use udon_core::Event;

/// Sentinels substituted for .desc micro-syntax bytes before udon-core sees
/// them (BRIDGE, ledgered): .desc semantics quote-protect `|` and `;` in
/// command tails and use single-quote rules inside bracket-ids, none of which
/// UDON content rules honor — a quoted pipe breaks the text run, a quoted
/// `;` starts a UDON comment, and quotes inside `[...]` open UDON strings
/// that swallow the rest of the line. Valve proposals: UDON-native tail
/// syntax / `<P>`-style escape aliases / UDON attr-id quoting remove the
/// need for each. Sentinels are restored at part-assembly time; bracket-id
/// text is re-extracted from the ORIGINAL bytes so ids never need restoring.
const PIPE_SENTINEL: u8 = 0x01;
const SEMI_SENTINEL: u8 = 0x02;
const SQUOTE_SENTINEL: u8 = 0x03;
const DQUOTE_SENTINEL: u8 = 0x04;

fn restore_sentinels(s: &str) -> String {
    s.chars()
        .map(|c| match c as u32 {
            0x01 => '|',
            0x02 => ';',
            0x03 => '\'',
            0x04 => '"',
            _ => c,
        })
        .collect()
}

/// A part under reconstruction.
struct Part {
    buf: String,
    /// Byte offset in source where the part's text begins (for lineno).
    start: usize,
    /// Estimated byte offset just past the last appended piece (for gap
    /// reconstruction between pieces).
    prev_end: usize,
}

pub struct Reader<'a> {
    /// Sentinel-protected bytes (what udon-core parsed; spans refer to this).
    source: &'a [u8],
    /// Original bytes (identical offsets — substitution is 1:1); bracket-ids
    /// are extracted from here so they never need sentinel restoration.
    original: &'a [u8],
    parts: Vec<Part>,
    /// Attr name awaiting its value event ("class" | generic).
    pending_attr: Option<(String, std::ops::Range<usize>)>,
    /// Events with span.start below this are inside a raw-extracted bracket-id
    /// region and must be ignored (udon fragments/garbles such ids).
    skip_until: usize,
    comment_depth: usize,
    /// Max source offset accounted for by structural events — used to detect
    /// pipes udon consumed without emitting structure (see `on_text`).
    consumed: usize,
    pub warnings: Vec<String>,
    /// Count of quote-protected pipes bridged via sentinel (spike metric).
    pub bridged_pipes: usize,
}

impl<'a> Reader<'a> {
    pub fn new(source: &'a [u8], original: &'a [u8]) -> Self {
        Reader {
            source,
            original,
            parts: Vec::new(),
            pending_attr: None,
            skip_until: 0,
            comment_depth: 0,
            consumed: 0,
            warnings: Vec::new(),
            bridged_pipes: 0,
        }
    }

    pub fn tokens(raw: &[u8], source_file: &str) -> Result<(Vec<Token>, Vec<String>), LexerError> {
        let (source, bridged) = protect_desc_microsyntax(raw);
        let mut r = Reader::new(&source, raw);
        r.bridged_pipes = bridged;
        // udon-core's callback parse; events borrow from `source`.
        udon_core::Parser::new(&source).parse(|ev| r.on_event(&ev));
        if bridged > 0 {
            r.warnings.push(format!("bridged {bridged} micro-syntax byte(s) via sentinels"));
        }
        let mut tokens = Vec::new();
        for part in &r.parts {
            let lineno = lineno_at(&source, part.start);
            let text = restore_sentinels(part.buf.trim_end());
            if let Some(tok) = parse_part(&text, lineno, source_file)? {
                tokens.push(tok);
            }
        }
        Ok((tokens, r.warnings))
    }

    fn on_event(&mut self, ev: &Event<'_>) {
        use Event::*;
        match ev {
            CommentStart { .. } => self.comment_depth += 1,
            CommentEnd { .. } => self.comment_depth = self.comment_depth.saturating_sub(1),
            _ if self.comment_depth > 0 => {}

            ElementStart { span } => {
                self.pending_attr = None;
                self.consumed = self.consumed.max(span.start);
                self.parts.push(Part { buf: String::new(), start: span.start, prev_end: span.start });
            }
            // NOTE: ElementEnd spans can point PAST the next line's pipe
            // (observed: End @87 while the orphaned '|' sits at 86), so End
            // events must not advance `consumed`.
            ElementEnd { .. } | EmbeddedEnd { .. } => {
                self.pending_attr = None;
            }
            EmbeddedStart { span } => {
                // |{...} inline element — treat like an element part.
                self.pending_attr = None;
                self.consumed = self.consumed.max(span.start);
                self.parts.push(Part { buf: String::new(), start: span.start, prev_end: span.start });
            }
            Name { content, span } => {
                let name = String::from_utf8_lossy(content).into_owned();
                self.consumed = self.consumed.max(span.start + content.len());
                self.append_to_last(&name, span.start, span.start + content.len());
            }
            Attr { content, span } => {
                if span.start < self.skip_until {
                    return;
                }
                self.consumed = self.consumed.max(span.end);
                let name = String::from_utf8_lossy(content).into_owned();
                if name == "id" {
                    // Positional bracket-id: udon fragments ids containing
                    // spaces and garbles quoted whitespace, so re-extract from
                    // RAW source with the oracle's exact bracket rules and
                    // skip every event inside the region.
                    if let Some(bp) = span.start.checked_sub(1) {
                        if self.original.get(bp) == Some(&b'[') {
                            let tail = String::from_utf8_lossy(&self.original[bp..]);
                            let chars: Vec<char> = tail.chars().collect();
                            let (_, end) = crate::lexer::extract_bracketed_id(&chars);
                            if let Some(endc) = end {
                                let blen: usize = chars[..endc].iter().map(|c| c.len_utf8()).sum();
                                let raw = String::from_utf8_lossy(&self.source[bp..bp + blen]).into_owned();
                                self.append_raw(&raw, bp + blen);
                                self.skip_until = bp + blen;
                                self.consumed = self.consumed.max(bp + blen);
                                return;
                            }
                        }
                    }
                }
                self.pending_attr = Some((name, span.clone()));
            }
            BareValue { content, span }
            | StringValue { content, span }
            | BoolTrue { content, span }
            | BoolFalse { content, span }
            | Nil { content, span }
            | Integer { content, span }
            | Float { content, span }
            | Rational { content, span }
            | Complex { content, span }
            | Date { content, span }
            | Time { content, span }
            | DateTime { content, span }
            | Duration { content, span }
            | RelativeTime { content, span }
            | Reference { content, span } => {
                if span.start < self.skip_until {
                    return;
                }
                let val = String::from_utf8_lossy(content).into_owned();
                self.consumed = self.consumed.max(span.start + content.len());
                match self.pending_attr.take() {
                    Some((a, _)) if a == "id" => {
                        // Positional bracket-id: content preserves quotes.
                        let piece = format!("[{val}]");
                        self.append_raw(&piece, span.end + 1);
                    }
                    Some((a, _)) if a == "class" => {
                        let piece = format!(".{val}");
                        self.append_raw(&piece, span.end);
                    }
                    Some((attr, aspan)) => {
                        // Generic sameline attr: raw slice from ':' to value end
                        // when recoverable, else re-serialize.
                        let start = aspan.start.saturating_sub(1);
                        let end = span.end.max(span.start + content.len());
                        let piece = match self.source.get(start..end) {
                            Some(raw) if raw.first() == Some(&b':') => {
                                String::from_utf8_lossy(raw).into_owned()
                            }
                            _ => format!(":{attr} {val}"),
                        };
                        self.append_to_last(&piece, start, end);
                    }
                    None => {
                        // Bare value in content position — treat as text.
                        self.append_to_last(&val, span.start, span.start + content.len());
                    }
                }
            }
            Text { content, span } | RawContent { content, span } | Raw { content, span } => {
                // A Text event can straddle the end of a raw-extracted
                // bracket-id region (udon doesn't scope `[...]`: it emits the
                // id remainder + the live tail as ONE run). Clip the part
                // inside the region rather than skipping the whole event.
                let text = String::from_utf8_lossy(content).into_owned();
                self.on_text(&text, span.start);
            }
            BlankLine { .. } => {}
            Warning { content, span } => {
                self.warnings.push(format!(
                    "udon warning @{}: {}",
                    span.start,
                    String::from_utf8_lossy(content)
                ));
            }
            Error { code, span } => {
                self.warnings.push(format!("udon PARSE ERROR @{}: {:?}", span.start, code));
            }
            DirectiveStart { span } => {
                self.warnings.push(format!("unhandled DirectiveStart @{}", span.start));
            }
            DirectiveEnd { .. } => {}
            ArrayStart { span } | FreeformStart { span } => {
                self.warnings.push(format!("unhandled Array/FreeformStart @{}", span.start));
            }
            ArrayEnd { .. } | FreeformEnd { .. } => {}
            Interpolation { content, span } => {
                self.warnings.push(format!(
                    "unhandled Interpolation @{}: {}",
                    span.start,
                    String::from_utf8_lossy(content)
                ));
            }
        }
    }

    /// Split a text run on pipes (same rules as the oracle splitter); piece 0
    /// continues the last open part, later pieces open new parts.
    ///
    /// Dropped-pipe detection (BRIDGE, ledgered): when a line ends with a
    /// sameline child *element*, udon-core emits the NEXT line's `|name` as
    /// Text with the pipe consumed, no ElementStart, and span pointing at the
    /// line start. We detect the orphaned '|' between the last
    /// structurally-consumed offset and the text's true content position and
    /// open a new part instead of appending.
    fn on_text(&mut self, text: &str, span_start: usize) {
        if text.is_empty() {
            return;
        }
        let mut text = text;
        let mut true_start = self.find_content(text, span_start);
        // Clip any prefix that lies inside a raw-extracted bracket-id region.
        if true_start < self.skip_until {
            let cut = self.skip_until - true_start;
            match text.get(cut..) {
                Some(rest) if !rest.is_empty() => {
                    text = rest;
                    true_start = self.skip_until;
                }
                _ => return,
            }
        }
        let scan_from = self.consumed.min(true_start);
        let dropped_pipe = self.source[scan_from..true_start].contains(&b'|');
        self.consumed = self.consumed.max(true_start + text.len());

        let pieces = split_with_offsets(text);
        for (i, (piece, off, terminated_prev)) in pieces.into_iter().enumerate() {
            let pos = true_start + off;
            if i == 0 && !terminated_prev && !dropped_pipe {
                self.append_to_last(piece.trim_end_matches('\n'), pos, pos + piece.len());
            } else {
                let trimmed = piece.trim_end_matches('\n');
                if trimmed.trim().is_empty() {
                    continue;
                }
                self.parts.push(Part {
                    buf: trimmed.to_string(),
                    start: pos,
                    prev_end: pos + piece.len(),
                });
            }
        }
    }

    /// udon-core Text spans can point at the line start rather than the
    /// content, and their ends can be one byte short; locate the content's
    /// actual byte offset by searching near span.start.
    fn find_content(&self, text: &str, span_start: usize) -> usize {
        let needle = text.as_bytes();
        if needle.is_empty() {
            return span_start;
        }
        let from = span_start.saturating_sub(4);
        let to = (span_start + needle.len() + 64).min(self.source.len());
        if from < to {
            let win = &self.source[from..to];
            if needle.len() <= win.len() {
                if let Some(rel) = win.windows(needle.len()).position(|w| w == needle) {
                    return from + rel;
                }
            }
        }
        span_start
    }

    /// Append with the raw inter-piece gap (preserves alignment spacing and
    /// newlines) when the gap is recoverable whitespace; else a single space.
    fn append_to_last(&mut self, piece: &str, src_start: usize, src_end: usize) {
        if piece.is_empty() {
            return;
        }
        let Some(part) = self.parts.last_mut() else {
            self.parts.push(Part { buf: piece.to_string(), start: src_start, prev_end: src_end });
            return;
        };
        if !part.buf.is_empty() {
            let sep = match self.source.get(part.prev_end..src_start) {
                Some(gap) if !gap.is_empty() && gap.iter().all(|b| b.is_ascii_whitespace()) => {
                    String::from_utf8_lossy(gap).into_owned()
                }
                Some(gap) if gap.is_empty() => String::new(),
                _ => " ".to_string(),
            };
            part.buf.push_str(&sep);
        } else {
            part.start = part.start.min(src_start);
        }
        part.buf.push_str(piece);
        part.prev_end = src_end;
    }

    /// Append literal reconstruction (id brackets / substate dot) with no gap.
    fn append_raw(&mut self, piece: &str, src_end: usize) {
        if let Some(part) = self.parts.last_mut() {
            part.buf.push_str(piece);
            part.prev_end = src_end;
        }
    }
}

/// Quote/bracket-aware pipe split returning `(piece, byte_offset,
/// terminated_prev)` — same state rules as `libdescent::lexer::split_on_pipes`
/// (sticky single-level bracket, both quote kinds, escape-aware), plus
/// offsets. `terminated_prev` is true for every piece after a pipe, INCLUDING
/// an empty piece 0 when the text begins with a pipe.
fn split_with_offsets(text: &str) -> Vec<(String, usize, bool)> {
    let mut out: Vec<(String, usize, bool)> = Vec::new();
    let mut current = String::new();
    let mut cur_off = 0usize;
    let mut in_bracket = false;
    let mut in_quote: Option<char> = None;
    let mut prev_char: Option<char> = None;
    let mut after_pipe = false;

    for (i, c) in text.char_indices() {
        match c {
            '\'' | '"' => {
                current.push(c);
                if in_quote == Some(c) && prev_char != Some('\\') {
                    in_quote = None;
                } else if in_quote.is_none() {
                    in_quote = Some(c);
                }
            }
            '[' => {
                if in_quote.is_none() {
                    in_bracket = true;
                }
                current.push(c);
            }
            ']' => {
                current.push(c);
                if in_quote.is_none() {
                    in_bracket = false;
                }
            }
            '|' => {
                if in_bracket || in_quote.is_some() {
                    current.push(c);
                } else {
                    if !current.is_empty() || !after_pipe {
                        out.push((std::mem::take(&mut current), cur_off, after_pipe));
                    } else {
                        current.clear();
                    }
                    cur_off = i + c.len_utf8();
                    after_pipe = true;
                }
            }
            _ => current.push(c),
        }
        prev_char = Some(c);
    }
    if !current.is_empty() || !after_pipe {
        out.push((current, cur_off, after_pipe));
    }
    out
}

fn lineno_at(source: &[u8], pos: usize) -> usize {
    source[..pos.min(source.len())].iter().filter(|&&b| b == b'\n').count() + 1
}

/// BRIDGE pre-pass: sentinel-substitute the .desc micro-syntax bytes that
/// UDON content rules would misinterpret, using the ORACLE's own state rules:
/// - comment regions per strip_comments (per-line depth + both quotes) are
///   BLANKED to spaces — udon does not recognize `;` comments in
///   continuation-text mode (observed after its "Inconsistent indentation"
///   degradation), so we don't rely on its comment rules at all;
/// - over non-comment bytes, split_on_pipes' GLOBAL quote state + sticky
///   single-level bracket flag drive the substitutions:
///   `|` in quotes -> PIPE_SENTINEL, quotes inside brackets -> *_QUOTE
///   sentinels (so udon can't open a string inside `[...]`), and every
///   non-comment `;` -> SEMI_SENTINEL (Ruby keeps exactly these).
/// Returns (modified bytes, substitution count). Substitution is 1:1 so all
/// offsets are preserved.
fn protect_desc_microsyntax(raw: &[u8]) -> (Vec<u8>, usize) {
    let mut out = raw.to_vec();

    // Pass 1: comment mask (strip_comments rules — state resets per line).
    let mut comment = vec![false; raw.len()];
    let mut line_start = 0;
    while line_start < raw.len() {
        let line_end = raw[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(raw.len(), |p| line_start + p);
        let mut depth: i64 = 0;
        let mut in_quote: Option<u8> = None;
        let mut prev: Option<u8> = None;
        for i in line_start..line_end {
            let b = raw[i];
            if b == b'\'' && prev != Some(b'\\') && in_quote != Some(b'"') {
                in_quote = if in_quote == Some(b'\'') { None } else { Some(b'\'') };
            } else if b == b'"' && prev != Some(b'\\') && in_quote != Some(b'\'') {
                in_quote = if in_quote == Some(b'"') { None } else { Some(b'"') };
            } else if in_quote.is_none() {
                match b {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b';' if depth == 0 => {
                        for j in i..line_end {
                            comment[j] = true;
                            out[j] = b' ';
                        }
                        break;
                    }
                    _ => {}
                }
            }
            prev = Some(b);
        }
        line_start = line_end + 1;
    }

    // Pass 2: global quote state + sticky bracket flag over non-comment bytes
    // (split_on_pipes rules).
    let mut count = 0;
    let mut in_quote: Option<u8> = None;
    let mut in_bracket = false;
    let mut prev: Option<u8> = None;
    for i in 0..raw.len() {
        if comment[i] {
            continue;
        }
        let b = raw[i];
        match b {
            b'\'' | b'"' => {
                if in_quote == Some(b) && prev != Some(b'\\') {
                    in_quote = None;
                } else if in_quote.is_none() {
                    in_quote = Some(b);
                }
                if in_bracket {
                    out[i] = if b == b'\'' { SQUOTE_SENTINEL } else { DQUOTE_SENTINEL };
                    count += 1;
                }
            }
            b'[' => {
                if in_quote.is_none() {
                    in_bracket = true;
                }
            }
            b']' => {
                if in_quote.is_none() {
                    in_bracket = false;
                }
            }
            b'|' if in_quote.is_some() => {
                out[i] = PIPE_SENTINEL;
                count += 1;
            }
            b';' => {
                out[i] = SEMI_SENTINEL;
                count += 1;
            }
            _ => {}
        }
        prev = Some(b);
    }
    (out, count)
}
