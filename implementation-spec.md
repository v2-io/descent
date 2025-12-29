# descent Implementation Specification

A recursive descent parser generator that produces high-performance callback-based
parsers from `.desc` specification files.

## Background: Lessons from Previous Generators

### genmachine (C-era) - 960 lines

**The Good:**
- Clean AST class hierarchy (GM, GMFunction, GMState, GMCase, GMCommand)
- Uses Liquid templates
- Some separation of concerns

**The Spaghetti:**
1. **`to_str()` methods on every AST class** - Each class knows how to render C code
2. **GMCommand does parsing AND C codegen** - `outv()`, `specials_map()`, `parse_side()` all know C syntax
3. **Template is vestigial** - Just `{% for f in functions %}{{f}}{% endfor %}` - real work in Ruby
4. **Type mapping hardcoded** - `type_map()` returns C types directly

### genmachine-rs (Rust-era) - 1262 lines

**Same pattern, worse coupling:**
1. **`to_rust()` methods everywhere** - Every class renders itself to Rust
2. **`emit_rust()` is 100 lines of Rust-specific event code** (lines 999-1101)
3. **Template is 838 lines** but mostly boilerplate - `{{ functions }}` placeholder for pre-rendered code
4. **State machine architecture baked in** - Generator assumes output is always a state machine with enum

### The Core Problem

Both generators conflate **three distinct concerns**:

```
┌─────────────────────────────────────────────────────────────────┐
│  1. DSL Parsing        →  What did the user write?              │
│  2. Semantic Analysis  →  What does it mean? (types, scopes)    │
│  3. Code Generation    →  How to express it in target language? │
└─────────────────────────────────────────────────────────────────┘
```

The existing generators do all three in a single pass through AST classes.
Each class knows how to parse itself AND how to render itself to the target language.

---

## Architecture

```
                    ┌──────────────┐
   udon.desc   ───▶ │   Lexer      │ ───▶  Tokens
                    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │   Parser     │ ───▶  AST (pure Data structs)
                    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │  IR Builder  │ ───▶  IR (semantic model)
                    │              │       - Types resolved
                    │              │       - SCAN chars inferred
                    │              │       - EOF handling inferred
                    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐       ┌─────────────────────┐
                    │   Generator  │◀──────│ rust/parser.liquid  │
                    │   (Liquid)   │       │ c/parser.liquid     │
                    └──────────────┘       │ (target-specific)   │
                           │               └─────────────────────┘
                           ▼
                       parser.rs
```

### Key Principles

| Principle | What It Means |
|-----------|---------------|
| **AST is pure data** | No `to_rust()` methods. Just Data structs with fields. |
| **IR is the semantic model** | All analysis done here: type inference, SCAN inference, validation |
| **Templates own ALL target knowledge** | Type mapping, syntax, idioms - all in Liquid |
| **Generator core ~400 lines** | Lexer + Parser + IR Builder + Generator |

---

## Module Responsibilities

### `Descent::Lexer` (~50 lines)
- Tokenize pipe-delimited `.desc` files
- Track line numbers for error reporting
- Output: Array of `Token` Data structs

### `Descent::Parser` (~150 lines)
- Build AST from token stream
- Pure recursive descent
- Output: `AST::Machine` containing functions, states, cases, commands

### `Descent::AST` (~60 lines)
- Pure Data structs, no behavior
- Represents direct parse result before semantic analysis
- Types: `Machine`, `Function`, `State`, `Case`, `Command`, `Conditional`

### `Descent::IRBuilder` (~100 lines)
- Transform AST → IR with semantic analysis
- Infer SCAN optimization characters from state structure
- Infer EOF handling requirements
- Collect local variable declarations with types
- Validate consistency

### `Descent::IR` (~80 lines)
- Semantic model Data structs
- Enriched with inferred information
- Types: `Parser`, `Function`, `State`, `Case`, `Command`
- States know their `scan_chars`, `is_self_looping`
- Functions know their `locals`, `emits_events`

### `Descent::Generator` (~50 lines)
- Render IR to target code using Liquid templates
- NO target-specific logic - just template loading and context building
- All type mapping, syntax, idioms in templates

---

## Template Structure

Templates live in `lib/descent/templates/{target}/`:

```
templates/
├── rust/
│   ├── parser.liquid      # Main template
│   └── _helpers.liquid    # Shared macros (optional)
└── c/
    ├── parser.liquid
    └── header.liquid
```

### What Templates Handle

**Type mapping:**
```liquid
{%- case type.kind -%}
  {%- when "string" -%}ChunkSlice
  {%- when "bracket" -%}{%- comment -%}void - emits events{%- endcomment -%}
{%- endcase -%}
```

**State machine generation:**
```liquid
{% for state in func.states %}
State::{{ state.name | pascalcase }} => {
    {% if state.scannable %}
    match self.scan_to{{ state.scan_chars | size }}({{ state.scan_chars | map: "rust_char" | join: ", " }}) {
    {% else %}
    match self.peek() {
    {% endif %}
    ...
}
{% endfor %}
```

**Emit patterns:**
```liquid
{% case cmd.args.value %}
{% when "ElementStart" %}
{ let name = Some(self.term()); self.emit(StreamingEvent::ElementStart { name, span: self.span_from_mark() }); }
{% when "Text" %}
{ let content = self.term(); self.emit(StreamingEvent::Text { content, span: self.span_from_mark() }); }
{% endcase %}
```

---

## SCAN Optimization Inference

The IR builder automatically infers SCAN optimization from state structure:

**Rule:** If a state has a default case that self-loops (`|default | -> |>>`),
the explicit character cases become SCAN targets.

**Example DSL:**
```
|state[:prose]
  |c[\n]     |.newline   | emit(Text)  |>> :line
  |c[<P>]    |.pipe      | emit(Text)  |>> :check_element
  |default   |.collect   | ->          |>>
```

**Inferred:** `scan_chars = ["\n", "|"]`

**Generated code uses `memchr2` instead of per-character loop.**

---

## EOF Handling Inference

Three sources of EOF behavior:

1. **Explicit `|eof` handler** - DSL specifies exactly what to do
2. **EXPECTS(x) annotation** - Implies error if EOF before x
3. **Return type** - CONTENT types emit on EOF, BRACKET types may error

The IR builder validates that every reachable state has EOF handling
(explicit or inferred) to prevent infinite loops.

---

## Bootstrapping Potential

The `.desc` format is valid UDON. This enables:

1. **Now:** descent parses `.desc` using hand-coded Ruby lexer/parser
2. **Later:** When libudon matures, descent can use UDON parser
3. **Full circle:** descent generates the parser that parses its own input

To enable this:
- Keep `.desc` syntax strictly UDON-compatible
- No extensions that would break UDON parsing
- Document any UDON features we rely on

---

## CLI Interface

```bash
# Basic usage
descent udon.desc                    # Output to stdout, Rust target

# Specify target
descent --target rust udon.desc
descent --target c udon.desc

# Output to file
descent -o parser.rs udon.desc

# Enable trace output in generated parser
descent --trace udon.desc

# Help
descent --help
descent --version
```

---

## Testing Strategy

### Unit Tests
- `test/lexer_test.rb` - Tokenization edge cases
- `test/parser_test.rb` - AST construction
- `test/ir_builder_test.rb` - Inference and validation

### Integration Tests
- `test/fixtures/minimal.desc` - Smallest valid parser
- `test/fixtures/udon.desc` - Full UDON parser (symlink to libudon)
- Round-trip: parse → IR → generate → compile → run

### Property Tests (future)
- Any valid `.desc` should produce compilable output
- Generated parser should accept what spec says it accepts

---

## Development Workflow

```bash
# Install dependencies
bundle install

# Run tests
dx test

# Lint
dx lint
dx lint --fix

# Build and install gem locally
dx gem install

# Generate parser from .desc
descent --target rust udon.desc > parser.rs
```

---

## File Structure

```
descent/
├── descent.gemspec
├── Gemfile
├── mise.toml
├── .rubocop.yml
├── implementation-spec.md     # This file
├── exe/
│   └── descent                # CLI entry point
├── lib/
│   ├── descent.rb             # Main module
│   └── descent/
│       ├── version.rb
│       ├── ast.rb             # Pure Data AST nodes
│       ├── ir.rb              # Semantic IR nodes
│       ├── lexer.rb           # Tokenizer
│       ├── parser.rb          # AST builder
│       ├── ir_builder.rb      # AST → IR transformation
│       ├── generator.rb       # Template rendering
│       └── templates/
│           ├── rust/
│           │   └── parser.liquid
│           └── c/
│               ├── parser.liquid
│               └── header.liquid
└── test/
    ├── test_helper.rb
    ├── descent_test.rb
    ├── lexer_test.rb
    ├── parser_test.rb
    └── fixtures/
        └── minimal.desc
```

---

## Success Criteria

1. **Separation of concerns:** No target-specific code outside templates
2. **Template-driven:** Adding a new target = new template directory
3. **Correctness:** Generated parser passes existing libudon test suite
4. **Performance:** Generated Rust parser achieves ≥2 GiB/s
5. **Maintainability:** Core Ruby code under 500 lines total
6. **Bootstrappable:** `.desc` format remains valid UDON
