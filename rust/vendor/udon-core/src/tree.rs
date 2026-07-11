//! Tree/AST representation for UDON documents.
//!
//! This module provides a tree-based API layered over the streaming parser.
//! The tree uses an index-based arena pattern for efficient allocation and
//! to enable parent pointers without reference cycles.
//!
//! # Example
//!
//! ```
//! use udon_core::tree::Document;
//!
//! let input = b"|article :author Joseph\n  |heading Hello\n  Some text here.\n";
//! let doc = Document::parse(input).unwrap();
//!
//! for node in doc.root().children() {
//!     if let Some(el) = node.as_element() {
//!         println!("Element: {}", el.name());
//!     }
//! }
//! ```

use std::borrow::Cow;
use crate::parser::{Event, ParseErrorCode, Parser};
use crate::span::Span;

// ============================================================================
// Core Types
// ============================================================================

/// Index into the document's node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    fn new(index: usize) -> Self {
        NodeId(index as u32)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Internal node storage.
#[derive(Debug)]
struct NodeData<'a> {
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    kind: NodeKind<'a>,
    /// Source byte range covered by this node, derived from parser event
    /// spans (Start-event start .. End-event end for container nodes; the
    /// event's own span for leaf nodes).
    span: Span,
}

/// The kind of node in the tree.
#[derive(Debug)]
pub enum NodeKind<'a> {
    /// Root document container.
    Document,

    /// Element node: `|element` or `|{embedded}`.
    Element {
        name: Cow<'a, str>,
        id: Option<Cow<'a, str>>,
        classes: Vec<Cow<'a, str>>,
        attrs: Vec<Attribute<'a>>,
        /// True for embedded elements `|{...}`.
        embedded: bool,
    },

    /// Text content.
    Text(Cow<'a, str>),

    /// Comment: `;` line or `;{inline}`.
    Comment(Cow<'a, str>),

    /// Directive: `!if`, `!for`, `!include`, etc.
    Directive {
        name: Cow<'a, str>,
        attrs: Vec<Attribute<'a>>,
    },

    /// Interpolation: `!{{expr}}`.
    Interpolation(Cow<'a, str>),

    /// Reference: `@[id]` or `:[ref]`.
    Reference(Cow<'a, str>),

    /// Raw or freeform content block.
    Raw {
        lang: Option<Cow<'a, str>>,
        content: Cow<'a, str>,
    },
}

/// An attribute on an element or directive.
#[derive(Debug)]
pub struct Attribute<'a> {
    pub name: Cow<'a, str>,
    pub value: Value<'a>,
}

/// A typed value preserving original representation.
#[derive(Debug, Clone)]
pub enum Value<'a> {
    /// Quoted string: `"hello"`.
    String(Cow<'a, str>),
    /// Bare/unquoted value.
    Bare(Cow<'a, str>),
    /// Integer (decimal, hex, octal, binary).
    Integer(Cow<'a, str>),
    /// Floating point.
    Float(Cow<'a, str>),
    /// Rational: `1/3r`.
    Rational(Cow<'a, str>),
    /// Complex: `3+4i`.
    Complex(Cow<'a, str>),
    /// Boolean true.
    BoolTrue,
    /// Boolean false.
    BoolFalse,
    /// Nil/null.
    Nil,
    /// Array of values.
    Array(Vec<Value<'a>>),
    /// Date: YYYY-MM-DD or YYYY-MM.
    Date(Cow<'a, str>),
    /// Time: HH:MM:SS or HH:MM with optional fractional seconds.
    Time(Cow<'a, str>),
    /// DateTime: Date T Time with optional offset.
    DateTime(Cow<'a, str>),
    /// Duration: ISO (P...) or shorthand (30s, 5m, 1h).
    Duration(Cow<'a, str>),
    /// Relative time: +/- followed by duration.
    RelativeTime(Cow<'a, str>),
}

// ============================================================================
// Document
// ============================================================================

/// A parsed UDON document as a tree.
#[derive(Debug)]
pub struct Document<'a> {
    nodes: Vec<NodeData<'a>>,
    root: NodeId,
}

/// A single parse error with a human-readable message.
#[derive(Debug)]
pub struct ParseError {
    pub code: ParseErrorCode,
    pub message: String,
    pub span: std::ops::Range<usize>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}..{}", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for ParseError {}

/// All errors encountered while parsing a document.
///
/// The parser recovers and continues after errors, so a single parse can
/// surface every error in the input, not just the first.
#[derive(Debug)]
pub struct ParseErrors {
    pub errors: Vec<ParseError>,
}

impl ParseErrors {
    /// The first error (always present).
    pub fn first(&self) -> &ParseError {
        &self.errors[0]
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ParseError> {
        self.errors.iter()
    }
}

impl std::fmt::Display for ParseErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.errors.len() == 1 {
            return write!(f, "{}", self.errors[0]);
        }
        writeln!(f, "{} parse errors:", self.errors.len())?;
        for err in &self.errors {
            writeln!(f, "  {}", err)?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseErrors {}

/// Human-readable message for an error code, e.g. `UnclosedArray` ->
/// "unclosed array". Derived from the code name so new codes stay covered.
fn describe_code(code: &ParseErrorCode) -> String {
    let name = format!("{:?}", code);
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push(' ');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

impl<'a> Document<'a> {
    /// Parse input bytes into a document tree.
    ///
    /// Collects *all* parse errors (the parser recovers and continues);
    /// returns `Err` if any occurred.
    pub fn parse(input: &'a [u8]) -> Result<Self, ParseErrors> {
        let mut builder = TreeBuilder::new();
        let mut errors: Vec<ParseError> = Vec::new();

        Parser::new(input).parse(|event| {
            if let Event::Error { code, span } = &event {
                errors.push(ParseError {
                    code: *code,
                    message: describe_code(code),
                    span: span.clone(),
                });
            } else {
                builder.handle_event(event);
            }
        });

        if !errors.is_empty() {
            return Err(ParseErrors { errors });
        }

        Ok(builder.finish(input.len()))
    }

    /// Get the root node.
    pub fn root(&self) -> Node<'_, 'a> {
        Node { doc: self, id: self.root }
    }

    /// Get a node by ID.
    pub fn get(&self, id: NodeId) -> Option<Node<'_, 'a>> {
        if (id.index()) < self.nodes.len() {
            Some(Node { doc: self, id })
        } else {
            None
        }
    }

    fn node_data(&self, id: NodeId) -> &NodeData<'a> {
        &self.nodes[id.index()]
    }
}

// ============================================================================
// Node (navigation handle)
// ============================================================================

/// A handle for navigating the document tree.
///
/// This is a lightweight reference that borrows from the document.
/// The `'doc: 'a` bound ensures the document outlives the input.
#[derive(Clone, Copy)]
pub struct Node<'doc, 'a: 'doc> {
    doc: &'doc Document<'a>,
    id: NodeId,
}

impl<'doc, 'a: 'doc> Node<'doc, 'a> {
    /// Get the node's ID.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Get the node's kind.
    pub fn kind(&self) -> &NodeKind<'a> {
        &self.doc.node_data(self.id).kind
    }

    /// Source byte range covered by this node.
    ///
    /// Derived from parser event spans: container nodes cover Start-event
    /// start through End-event end; leaf nodes use their event's span.
    /// (Sigil prefixes such as `|` are currently not included in the Start
    /// event's span — refining this is part of the node-span/error work.)
    pub fn span(&self) -> Span {
        self.doc.node_data(self.id).span
    }

    /// Get the parent node, if any.
    pub fn parent(&self) -> Option<Node<'doc, 'a>> {
        self.doc.node_data(self.id).parent.map(|id| Node { doc: self.doc, id })
    }

    /// Iterate over child nodes.
    pub fn children(&self) -> impl Iterator<Item = Node<'doc, 'a>> + 'doc {
        let doc = self.doc;
        self.doc.node_data(self.id).children.iter().map(move |&id| Node { doc, id })
    }

    /// Get the first child node.
    pub fn first_child(&self) -> Option<Node<'doc, 'a>> {
        self.doc.node_data(self.id).children.first().map(|&id| Node { doc: self.doc, id })
    }

    /// Get the last child node.
    pub fn last_child(&self) -> Option<Node<'doc, 'a>> {
        self.doc.node_data(self.id).children.last().map(|&id| Node { doc: self.doc, id })
    }

    /// Get the next sibling node.
    pub fn next_sibling(&self) -> Option<Node<'doc, 'a>> {
        let parent_id = self.doc.node_data(self.id).parent?;
        let siblings = &self.doc.node_data(parent_id).children;
        let pos = siblings.iter().position(|&id| id == self.id)?;
        siblings.get(pos + 1).map(|&id| Node { doc: self.doc, id })
    }

    /// Get the previous sibling node.
    pub fn prev_sibling(&self) -> Option<Node<'doc, 'a>> {
        let parent_id = self.doc.node_data(self.id).parent?;
        let siblings = &self.doc.node_data(parent_id).children;
        let pos = siblings.iter().position(|&id| id == self.id)?;
        if pos > 0 {
            Some(Node { doc: self.doc, id: siblings[pos - 1] })
        } else {
            None
        }
    }

    /// Check if this is an element node.
    pub fn is_element(&self) -> bool {
        matches!(self.kind(), NodeKind::Element { .. })
    }

    /// Check if this is a text node.
    pub fn is_text(&self) -> bool {
        matches!(self.kind(), NodeKind::Text(_))
    }

    /// Get element view if this is an element.
    pub fn as_element(&self) -> Option<ElementView<'doc, 'a>> {
        if let NodeKind::Element { .. } = self.kind() {
            Some(ElementView { node: *self })
        } else {
            None
        }
    }

    /// Get text content if this is a text node.
    pub fn text_content(&self) -> Option<&str> {
        if let NodeKind::Text(s) = self.kind() {
            Some(s.as_ref())
        } else {
            None
        }
    }

    /// Recursively collect all text content under this node.
    ///
    /// Includes `Text` nodes and `Raw` (freeform/raw block) content;
    /// excludes comments. Adjacent chunks are separated by a single space
    /// unless one side already ends/starts with whitespace, so prose lines
    /// don't run together ("Hello there" + "second line" ->
    /// "Hello there second line").
    pub fn all_text(&self) -> String {
        let mut result = String::new();
        self.collect_text(&mut result);
        result
    }

    fn collect_text(&self, buf: &mut String) {
        match self.kind() {
            NodeKind::Text(s) => push_text_chunk(buf, s),
            NodeKind::Raw { content, .. } => push_text_chunk(buf, content),
            NodeKind::Comment(_) => {}
            _ => {
                for child in self.children() {
                    child.collect_text(buf);
                }
            }
        }
    }
}

impl<'doc, 'a> std::fmt::Debug for Node<'doc, 'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("kind", self.kind())
            .finish()
    }
}

// ============================================================================
// ElementView (typed access to elements)
// ============================================================================

/// A typed view for element nodes.
#[derive(Clone, Copy)]
pub struct ElementView<'doc, 'a: 'doc> {
    node: Node<'doc, 'a>,
}

impl<'doc, 'a: 'doc> ElementView<'doc, 'a> {
    /// Get the underlying node.
    pub fn node(&self) -> Node<'doc, 'a> {
        self.node
    }

    /// Get the element name.
    pub fn name(&self) -> &str {
        if let NodeKind::Element { name, .. } = self.node.kind() {
            name.as_ref()
        } else {
            unreachable!()
        }
    }

    /// Get the element ID, if any.
    pub fn id(&self) -> Option<&str> {
        if let NodeKind::Element { id, .. } = self.node.kind() {
            id.as_ref().map(|s| s.as_ref())
        } else {
            None
        }
    }

    /// Get the element classes.
    pub fn classes(&self) -> &[Cow<'a, str>] {
        if let NodeKind::Element { classes, .. } = self.node.kind() {
            classes
        } else {
            &[]
        }
    }

    /// Check if the element has a specific class.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes().iter().any(|c| c.as_ref() == class)
    }

    /// Check if this is an embedded element (`|{...}`).
    pub fn is_embedded(&self) -> bool {
        if let NodeKind::Element { embedded, .. } = self.node.kind() {
            *embedded
        } else {
            false
        }
    }

    /// Get an attribute value by name.
    pub fn attr(&self, name: &str) -> Option<&Value<'a>> {
        if let NodeKind::Element { attrs, .. } = self.node.kind() {
            attrs.iter().find(|a| a.name.as_ref() == name).map(|a| &a.value)
        } else {
            None
        }
    }

    /// Iterate over all attributes.
    pub fn attrs(&self) -> impl Iterator<Item = (&str, &Value<'a>)> {
        let attrs = if let NodeKind::Element { attrs, .. } = self.node.kind() {
            attrs.as_slice()
        } else {
            &[]
        };
        attrs.iter().map(|a| (a.name.as_ref(), &a.value))
    }

    /// Iterate over child nodes.
    pub fn children(&self) -> impl Iterator<Item = Node<'doc, 'a>> + 'doc {
        self.node.children()
    }
}

impl<'doc, 'a> std::fmt::Debug for ElementView<'doc, 'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementView")
            .field("name", &self.name())
            .field("id", &self.id())
            .field("classes", &self.classes())
            .finish()
    }
}

// ============================================================================
// TreeBuilder (event consumer)
// ============================================================================

/// Builds a document tree from parser events.
struct TreeBuilder<'a> {
    nodes: Vec<NodeData<'a>>,
    /// Stack of open node IDs.
    stack: Vec<NodeId>,
    /// Current attribute being built.
    current_attr: Option<Cow<'a, str>>,
    /// Array nesting for values.
    array_stack: Vec<Vec<Value<'a>>>,
    /// Node IDs (raw u32) that have received at least one content line
    /// (Comment/Raw content accumulation).
    content_started: std::collections::HashSet<u32>,
}

impl<'a> TreeBuilder<'a> {
    fn new() -> Self {
        // Create root document node
        let root = NodeData {
            parent: None,
            children: Vec::new(),
            kind: NodeKind::Document,
            span: Span::empty(),
        };
        TreeBuilder {
            nodes: vec![root],
            stack: vec![NodeId::new(0)],
            current_attr: None,
            array_stack: Vec::new(),
            content_started: std::collections::HashSet::new(),
        }
    }

    fn current(&self) -> NodeId {
        *self.stack.last().unwrap()
    }

    fn push_node(&mut self, kind: NodeKind<'a>, span: &std::ops::Range<usize>) -> NodeId {
        let parent = self.current();
        let id = NodeId::new(self.nodes.len());
        self.nodes.push(NodeData {
            parent: Some(parent),
            children: Vec::new(),
            kind,
            span: Span::new(span.start, span.end),
        });
        self.nodes[parent.index()].children.push(id);
        id
    }

    /// Extend the current node's span to cover through `end`.
    fn extend_current_span(&mut self, end: usize) {
        let current = self.current();
        let node = &mut self.nodes[current.index()];
        if (end as u32) > node.span.end {
            node.span.end = end as u32;
        }
    }

    /// Append a line of content to a `Comment` or `Raw` node's content
    /// field, joining lines with '\n'. Returns true if the node accepts
    /// line content.
    ///
    /// The first appended line replaces the initial empty content wholesale
    /// (tracked via `content_started`) so that a leading blank line is
    /// preserved rather than confused with "no content yet".
    fn append_line_content(&mut self, id: NodeId, line: Cow<'a, str>) -> bool {
        let started = self.content_started.contains(&id.0);
        let node = &mut self.nodes[id.index()];
        let target = match &mut node.kind {
            NodeKind::Comment(content) => content,
            NodeKind::Raw { content, .. } => content,
            _ => return false,
        };
        if !started {
            // First line: keep it borrowed when possible (zero-copy).
            *target = line;
            self.content_started.insert(id.0);
        } else {
            let owned = target.to_mut();
            owned.push('\n');
            owned.push_str(&line);
        }
        true
    }

    fn handle_event(&mut self, event: Event<'a>) {
        use Event::*;

        match event {
            // ---- Elements ----
            ElementStart { span } => {
                let id = self.push_node(NodeKind::Element {
                    name: Cow::Borrowed(""),
                    id: None,
                    classes: Vec::new(),
                    attrs: Vec::new(),
                    embedded: false,
                }, &span);
                self.stack.push(id);
            }
            ElementEnd { span } => {
                self.extend_current_span(span.end);
                self.stack.pop();
            }

            EmbeddedStart { span } => {
                let id = self.push_node(NodeKind::Element {
                    name: Cow::Borrowed(""),
                    id: None,
                    classes: Vec::new(),
                    attrs: Vec::new(),
                    embedded: true,
                }, &span);
                self.stack.push(id);
            }
            EmbeddedEnd { span } => {
                self.extend_current_span(span.end);
                self.stack.pop();
            }

            Name { content, .. } => {
                let current = self.current();
                match &mut self.nodes[current.index()].kind {
                    NodeKind::Element { name, .. } | NodeKind::Directive { name, .. } => {
                        *name = bytes_to_cow(&content);
                    }
                    // Freeform fence info string (```lang) — wire it to the
                    // Raw node's lang field.
                    NodeKind::Raw { lang, .. } => {
                        *lang = Some(bytes_to_cow(&content));
                    }
                    _ => {}
                }
            }

            // ---- Attributes ----
            Attr { content, .. } => {
                self.current_attr = Some(bytes_to_cow(&content));
            }

            // ---- Values ----
            StringValue { content, .. } => {
                self.add_value(Value::String(bytes_to_cow(&content)));
            }
            BareValue { content, .. } => {
                let s = bytes_to_cow(&content);
                // Check if this is for id or class
                if let Some(attr_name) = &self.current_attr {
                    if attr_name == "id" {
                        let current = self.current();
                        if let NodeKind::Element { id, .. } = &mut self.nodes[current.index()].kind {
                            *id = Some(s.clone());
                        }
                        self.current_attr = None;
                        return;
                    } else if attr_name == "class" {
                        let current = self.current();
                        if let NodeKind::Element { classes, .. } = &mut self.nodes[current.index()].kind {
                            classes.push(s.clone());
                        }
                        self.current_attr = None;
                        return;
                    }
                }
                self.add_value(Value::Bare(s));
            }
            Integer { content, .. } => {
                self.add_value(Value::Integer(bytes_to_cow(&content)));
            }
            Float { content, .. } => {
                self.add_value(Value::Float(bytes_to_cow(&content)));
            }
            Rational { content, .. } => {
                self.add_value(Value::Rational(bytes_to_cow(&content)));
            }
            Complex { content, .. } => {
                self.add_value(Value::Complex(bytes_to_cow(&content)));
            }
            Date { content, .. } => {
                self.add_value(Value::Date(bytes_to_cow(&content)));
            }
            Time { content, .. } => {
                self.add_value(Value::Time(bytes_to_cow(&content)));
            }
            DateTime { content, .. } => {
                self.add_value(Value::DateTime(bytes_to_cow(&content)));
            }
            Duration { content, .. } => {
                self.add_value(Value::Duration(bytes_to_cow(&content)));
            }
            RelativeTime { content, .. } => {
                self.add_value(Value::RelativeTime(bytes_to_cow(&content)));
            }
            BoolTrue { .. } => {
                self.add_value(Value::BoolTrue);
            }
            BoolFalse { .. } => {
                self.add_value(Value::BoolFalse);
            }
            Nil { .. } => {
                self.add_value(Value::Nil);
            }

            // ---- Arrays ----
            ArrayStart { .. } => {
                self.array_stack.push(Vec::new());
            }
            ArrayEnd { .. } => {
                if let Some(items) = self.array_stack.pop() {
                    self.add_value(Value::Array(items));
                }
            }

            // ---- Text ----
            // Inside Comment/Raw nodes, text lines accumulate into the
            // node's content field (single representation). Elsewhere they
            // become Text child nodes.
            Text { content, span } => {
                let current = self.current();
                if matches!(
                    self.nodes[current.index()].kind,
                    NodeKind::Comment(_) | NodeKind::Raw { .. }
                ) {
                    let line = bytes_to_cow(&content);
                    self.append_line_content(current, line);
                    self.extend_current_span(span.end);
                } else {
                    self.push_node(NodeKind::Text(bytes_to_cow(&content)), &span);
                }
            }

            // ---- Comments ----
            // Comment text lines accumulate into the Comment node's content
            // (see Text handling above).
            CommentStart { span } => {
                let id = self.push_node(NodeKind::Comment(Cow::Borrowed("")), &span);
                self.stack.push(id);
            }
            CommentEnd { span } => {
                self.extend_current_span(span.end);
                self.stack.pop();
            }

            // ---- Directives ----
            DirectiveStart { span } => {
                let id = self.push_node(NodeKind::Directive {
                    name: Cow::Borrowed(""),
                    attrs: Vec::new(),
                }, &span);
                self.stack.push(id);
            }
            DirectiveEnd { span } => {
                self.extend_current_span(span.end);
                self.stack.pop();
            }

            // ---- Interpolation ----
            Interpolation { content, span } => {
                self.push_node(NodeKind::Interpolation(bytes_to_cow(&content)), &span);
            }

            // ---- References ----
            Reference { content, span } => {
                self.push_node(NodeKind::Reference(bytes_to_cow(&content)), &span);
            }

            // ---- Raw/Freeform ----
            FreeformStart { span } => {
                let id = self.push_node(NodeKind::Raw {
                    lang: None,
                    content: Cow::Borrowed(""),
                }, &span);
                self.stack.push(id);
            }
            FreeformEnd { span } => {
                self.extend_current_span(span.end);
                self.stack.pop();
            }
            // `Raw` is the raw-kind marker emitted by `!:kind:` directives
            // (empty content); `RawContent` carries the actual lines.
            Raw { content, span } => {
                if !content.is_empty() {
                    self.handle_raw_content(content, span);
                }
            }
            RawContent { content, span } => {
                self.handle_raw_content(content, span);
            }

            // ---- Blank lines ----
            // Freeform blocks preserve blank lines as empty content lines.
            // Elsewhere blank lines are not represented in the tree (yet).
            BlankLine { span, .. } => {
                let current = self.current();
                if matches!(self.nodes[current.index()].kind, NodeKind::Raw { .. }) {
                    self.append_line_content(current, Cow::Borrowed(""));
                    self.extend_current_span(span.end);
                }
            }

            // ---- Ignored ----
            Error { .. } | Warning { .. } => {}
        }
    }

    /// Raw line content from a `!:kind:` raw directive (or any RawContent
    /// outside a freeform block): accumulate all lines into a single Raw
    /// child node of the current node.
    fn handle_raw_content(&mut self, content: Cow<'a, [u8]>, span: std::ops::Range<usize>) {
        let current = self.current();
        // Freeform/Raw node open on the stack: append directly.
        if matches!(self.nodes[current.index()].kind, NodeKind::Raw { .. }) {
            let line = bytes_to_cow(&content);
            self.append_line_content(current, line);
            self.extend_current_span(span.end);
            return;
        }
        // Continue an existing Raw child if it's the most recent child.
        if let Some(&last) = self.nodes[current.index()].children.last() {
            if matches!(self.nodes[last.index()].kind, NodeKind::Raw { .. }) {
                let line = bytes_to_cow(&content);
                self.append_line_content(last, line);
                let node = &mut self.nodes[last.index()];
                if (span.end as u32) > node.span.end {
                    node.span.end = span.end as u32;
                }
                return;
            }
        }
        // First raw line: create the Raw child.
        let id = self.push_node(NodeKind::Raw {
            lang: None,
            content: Cow::Borrowed(""),
        }, &span);
        let line = bytes_to_cow(&content);
        self.append_line_content(id, line);
    }

    fn add_value(&mut self, value: Value<'a>) {
        // If we're in an array context, add to the array
        if let Some(arr) = self.array_stack.last_mut() {
            arr.push(value);
            return;
        }

        // Otherwise add as attribute
        if let Some(attr_name) = self.current_attr.take() {
            let current = self.current();
            match &mut self.nodes[current.index()].kind {
                NodeKind::Element { attrs, .. } | NodeKind::Directive { attrs, .. } => {
                    attrs.push(Attribute { name: attr_name, value });
                }
                _ => {}
            }
        }
    }

    fn finish(mut self, input_len: usize) -> Document<'a> {
        self.nodes[0].span = Span::new(0, input_len);
        Document {
            nodes: self.nodes,
            root: NodeId::new(0),
        }
    }
}

/// Append a text chunk to `buf`, inserting a single space separator when
/// neither side supplies whitespace (used by `Node::all_text`).
fn push_text_chunk(buf: &mut String, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    let needs_sep = !buf.is_empty()
        && !buf.ends_with(|c: char| c.is_whitespace())
        && !chunk.starts_with(|c: char| c.is_whitespace());
    if needs_sep {
        buf.push(' ');
    }
    buf.push_str(chunk);
}

/// Convert bytes to Cow<str>, using borrowed if valid UTF-8.
fn bytes_to_cow<'a>(bytes: &std::borrow::Cow<'a, [u8]>) -> Cow<'a, str> {
    match bytes {
        std::borrow::Cow::Borrowed(b) => {
            match std::str::from_utf8(b) {
                Ok(s) => Cow::Borrowed(s),
                Err(_) => Cow::Owned(String::from_utf8_lossy(b).into_owned()),
            }
        }
        std::borrow::Cow::Owned(b) => {
            match String::from_utf8(b.clone()) {
                Ok(s) => Cow::Owned(s),
                Err(e) => Cow::Owned(String::from_utf8_lossy(&e.into_bytes()).into_owned()),
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_element() {
        let doc = Document::parse(b"|div\n").unwrap();
        let root = doc.root();

        assert!(matches!(root.kind(), NodeKind::Document));

        let child = root.first_child().unwrap();
        let el = child.as_element().unwrap();
        assert_eq!(el.name(), "div");
    }

    #[test]
    fn test_element_with_id_and_class() {
        let doc = Document::parse(b"|div[myid].class1.class2\n").unwrap();
        let el = doc.root().first_child().unwrap().as_element().unwrap();

        assert_eq!(el.name(), "div");
        assert_eq!(el.id(), Some("myid"));
        assert_eq!(el.classes().len(), 2);
        assert!(el.has_class("class1"));
        assert!(el.has_class("class2"));
    }

    #[test]
    fn test_nested_elements() {
        let doc = Document::parse(b"|parent\n  |child\n").unwrap();
        let parent = doc.root().first_child().unwrap();
        let child = parent.first_child().unwrap();

        assert_eq!(parent.as_element().unwrap().name(), "parent");
        assert_eq!(child.as_element().unwrap().name(), "child");

        // Test parent pointer
        assert_eq!(child.parent().unwrap().id(), parent.id());
    }

    #[test]
    fn test_text_content() {
        let doc = Document::parse(b"|p Hello world\n").unwrap();
        let p = doc.root().first_child().unwrap();
        let text = p.first_child().unwrap();

        assert!(text.is_text());
        assert!(text.text_content().unwrap().contains("Hello world"));
    }

    #[test]
    fn test_attributes() {
        let doc = Document::parse(b"|el :foo bar :count 42\n").unwrap();
        let el = doc.root().first_child().unwrap().as_element().unwrap();

        assert!(matches!(el.attr("foo"), Some(Value::Bare(_))));
        assert!(matches!(el.attr("count"), Some(Value::Integer(_))));
    }

    #[test]
    fn test_array_attribute() {
        let doc = Document::parse(b"|el :tags [a b c]\n").unwrap();
        let el = doc.root().first_child().unwrap().as_element().unwrap();

        if let Some(Value::Array(arr)) = el.attr("tags") {
            assert_eq!(arr.len(), 3);
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_siblings() {
        let doc = Document::parse(b"|a\n|b\n|c\n").unwrap();
        let a = doc.root().first_child().unwrap();
        let b = a.next_sibling().unwrap();
        let c = b.next_sibling().unwrap();

        assert_eq!(a.as_element().unwrap().name(), "a");
        assert_eq!(b.as_element().unwrap().name(), "b");
        assert_eq!(c.as_element().unwrap().name(), "c");

        assert!(c.next_sibling().is_none());
        assert_eq!(b.prev_sibling().unwrap().id(), a.id());
    }

    #[test]
    fn test_all_text() {
        let doc = Document::parse(b"|p Hello |{em world}\n").unwrap();
        let p = doc.root().first_child().unwrap();

        let text = p.all_text();
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn test_all_text_separates_lines() {
        let doc = Document::parse(b"|p\n  Hello there\n  second line\n").unwrap();
        let p = doc.root().first_child().unwrap();
        assert_eq!(p.all_text(), "Hello there second line");
    }

    #[test]
    fn test_all_text_no_double_space_around_inline() {
        // "Hello " already ends with whitespace; no extra separator.
        let doc = Document::parse(b"|p Hello |{em world}\n").unwrap();
        let p = doc.root().first_child().unwrap();
        assert_eq!(p.all_text(), "Hello world");
    }

    #[test]
    fn test_comment_content_populated() {
        let doc = Document::parse(b"; first\n  second\n  third\n").unwrap();
        let comment = doc.root().first_child().unwrap();
        match comment.kind() {
            NodeKind::Comment(content) => {
                assert_eq!(content.as_ref(), " first\nsecond\nthird");
            }
            other => panic!("expected Comment, got {:?}", other),
        }
        // Single representation: no Text children under the comment.
        assert!(comment.first_child().is_none());
        // And comments are excluded from all_text.
        assert_eq!(doc.root().all_text(), "");
    }

    #[test]
    fn test_freeform_lang_and_content() {
        let doc = Document::parse(b"```python\nx = 1\ny = 2\n```\n").unwrap();
        let raw = doc.root().first_child().unwrap();
        match raw.kind() {
            NodeKind::Raw { lang, content } => {
                assert_eq!(lang.as_deref(), Some("python"));
                assert_eq!(content.as_ref(), "x = 1\ny = 2");
            }
            other => panic!("expected Raw, got {:?}", other),
        }
        // Single representation: content lives on the node, not Text children.
        assert!(raw.first_child().is_none());
    }

    #[test]
    fn test_raw_directive_single_node() {
        let doc = Document::parse(b"!:sql:\n  SELECT 1\n  FROM t\n").unwrap();
        let dir = doc.root().first_child().unwrap();
        match dir.kind() {
            NodeKind::Directive { name, .. } => assert_eq!(name.as_ref(), "sql"),
            other => panic!("expected Directive, got {:?}", other),
        }
        // All raw lines consolidated into ONE Raw child.
        let children: Vec<_> = dir.children().collect();
        assert_eq!(children.len(), 1);
        match children[0].kind() {
            NodeKind::Raw { content, .. } => assert_eq!(content.as_ref(), "SELECT 1\nFROM t"),
            other => panic!("expected Raw, got {:?}", other),
        }
    }

    #[test]
    fn test_node_spans_wired() {
        let input = b"|p Hello world\n";
        let doc = Document::parse(input).unwrap();
        assert_eq!(doc.root().span().start, 0);
        assert_eq!(doc.root().span().end as usize, input.len());

        let p = doc.root().first_child().unwrap();
        let text = p.first_child().unwrap();
        // Text span covers its content bytes ("Hello world" at 3..14).
        assert_eq!(text.span().start, 3);
        assert_eq!(text.span().end, 14);
        // Element span covers at least through its text content.
        assert!(p.span().end >= text.span().end);
        assert!(p.span().start <= text.span().start);
    }

    #[test]
    fn test_parse_collects_all_errors_with_readable_messages() {
        // Two tab-indented lines -> two NoTabs errors.
        let err = Document::parse(b"\tx\n\ty\n").unwrap_err();
        assert_eq!(err.len(), 2);
        assert_eq!(err.first().message, "no tabs");
        assert!(err.to_string().contains("no tabs"));
    }
}
