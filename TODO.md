# descent TODO

## CLI Implementation (using devex/core)

Entry point: `exe/descent`
Tools directory: `lib/descent/tools/`

### Commands

| Command | Description |
|---------|-------------|
| `descent generate <file>` | Generate parser from .desc file |
| `descent debug <file>` | Output tokens/AST/IR for debugging |
| `descent validate <file>` | Validate .desc file without generating |

### Flags

**generate:**
- `-o, --output=FILE` - Output to file (default: stdout)
- `-t, --target=TARGET` - Target language: rust, c (default: rust)
- `--trace` - Enable trace output in generated parser

**debug:**
- `--tokens` - Show tokens only
- `--ast` - Show AST only
- `--ir` - Show IR only (default: all stages)

**validate:**
- (no special flags)

### Implementation

```
exe/descent                    # CLI entry point using devex/core
lib/descent/tools/
  generate.rb                  # Generate parser command
  debug.rb                     # Debug/inspect command
  validate.rb                  # Validate command
```

## Current Status

### Working
- [x] Lexer: tokenizes .desc files
- [x] Parser: builds AST with functions, states, cases, commands
- [x] IR Builder: transforms AST with SCAN inference, type resolution
- [x] Character classes: `letter`, `label_cont` parsed correctly
- [x] Conditional cases: `|if[condition]` parsed with commands
- [x] Rust template: generates basic callback-based parser
- [x] Entry point: strips leading `/`
- [x] SCAN optimization: generates memchr calls (implicit from self-looping default)
- [x] Explicit advance-to: `->[chars]` generates memchr calls
- [x] Type-driven emit: BRACKET emits Start/End, CONTENT emits on return
- [x] EOF inference: basic type-based emit on EOF
- [x] Debug script: `bin/debug` dumps tokens/AST/IR
- [x] CLI: `descent generate/debug/validate` commands via devex/core
- [x] Function parameters: `:col` params passed through, COL → self.col()

### Template Issues
- [x] Clean up excessive whitespace in generated code (post-processing gsub)
- [x] Liquid deprecation warnings (use Environment instead of Template class methods)
- [x] Handle function parameters in state machine (e.g., `col` parameter)
- [x] COL keyword transforms to `self.col()` in conditions and call args

### Parser/Lexer Issues
- [x] `->` recognized as advance command (FIXED)
- [x] Character classes like `letter`, `label_cont` (FIXED)
- [x] Conditional cases with commands (FIXED)
- [x] `->[chars]` advance-to with escape processing (FIXED)
- [x] Function call arguments preserve case (COL vs col) (FIXED)

## EXPECTS Inference (IMPLEMENTED)

The DSL does NOT have explicit `EXPECTS(x)` annotations. Instead, the expected closing
delimiter is INFERRED from the structure of return cases.

### Inference Algorithm

1. For each function, find ALL cases that contain a `return` command
2. Check if ALL such cases match a SINGLE character (same char across all return cases)
3. If yes, that character is the inferred `expects_char`
4. Also check if TERM appears before return in those cases (`emits_content_on_close`)

### Example: String Parsing

```
|function[string:StringValue]   ; CONTENT type
  |state[:main]
    |c["]      | TERM  |return   ; ← return on single char ", with TERM
    |c[\\]     | -> | -> |>>     ; escape handling, loops
    |default   | ->     |>>      ; collect, loops
```

**Inference:**
- All returns are on `|c["]` → `expects_char = '"'`
- TERM is present before return → `emits_content_on_close = true`

### Example: Nested Brackets

```
|function[brace_comment:Comment] | depth = 1
  |state[:main]
    |c[{]    | depth += 1        |>>
    |c[}]    | depth -= 1
      |if[depth == 0]            |return   ; ← return still on }, guarded by condition
    |default | ->                |>>
```

**Inference:**
- Returns are on `|c[}]` (with condition) → `expects_char = '}'`
- No TERM → `emits_content_on_close = false` (or check if CONTENT type needs auto-TERM)

### EOF Behavior with Inferred EXPECTS

When EOF is reached and `expects_char` is set:

**For CONTENT types (with `emits_content_on_close`):**
1. Emit the accumulated content: `on_event(Event::Type { content: self.term(), ... })`
2. Emit unclosed error: `on_event(Event::Error { code: UnclosedX, ... })`
3. Return

**For BRACKET types:**
1. Emit unclosed error ONLY (no End event)
2. Return
3. The missing End event signals to consumer what wasn't closed

**For functions WITHOUT inferred expects_char:**
- Current behavior: emit based on type and return (no error)

### IR Changes Needed

Add to `IR::Function`:
```ruby
Function = Data.define(:name, :return_type, :params, :locals, :states,
                       :eof_handler, :emits_events, :expects_char,
                       :emits_content_on_close, :lineno)
```

### IR Builder Changes

In `build_function`:
```ruby
def infer_expects(func, states)
  return_cases = []

  states.each do |state|
    state.cases.each do |kase|
      if kase.commands.any? { |cmd| cmd.type == :return }
        return_cases << kase
      end
    end
  end

  return [nil, false] if return_cases.empty?

  # Check if all return cases match same single char
  chars = return_cases.map { |c| c.chars }.compact
  return [nil, false] unless chars.all? { |c| c.length == 1 }
  return [nil, false] unless chars.map(&:first).uniq.length == 1

  expects_char = chars.first.first

  # Check if TERM appears before return
  emits_content = return_cases.any? do |kase|
    kase.commands.any? { |cmd| cmd.type == :term }
  end

  [expects_char, emits_content]
end
```

### Template Changes

In EOF handling sections, check for `expects_char`:

```liquid
{% if func.expects_char %}
  {% comment %} Unclosed delimiter EOF {% endcomment %}
  {% if return_type_info.kind == "content" and func.emits_content_on_close %}
  on_event(Event::{{ func.return_type }} { content: self.term(), span: self.span_from_mark() });
  {% endif %}
  on_event(Event::Error { code: ParseErrorCode::Unclosed{{ func.return_type }}, span: self.span() });
  return;
{% else %}
  {% comment %} Normal EOF - current behavior {% endcomment %}
  ...
{% endif %}
```

### Error Code Generation

Need to generate error codes for each type that can have unclosed errors:
```rust
pub enum ParseErrorCode {
    UnexpectedEof,
    UnexpectedChar,
    UnclosedStringValue,  // Generated from types with expects_char
    UnclosedComment,
    // etc.
}
```

## DSL Feature Coverage

### Implemented
- Basic functions: `|function[name:Type]`
- Parameters: `:param1 :param2`
- States: `|state[:name]`
- Character matching: `|c[chars]`, `|default`
- Character classes: `letter`, `label_cont`
- Transitions: `|>>`, `|>> :state`, `|return`
- Function calls: `/function`, `/function(args)`
- Conditionals: `|if[condition] |return`
- SCAN inference from self-looping default
- Explicit advance-to: `->[chars]`
- Type declarations: `|type[Name] BRACKET/CONTENT/INTERNAL`
- EOF inference (basic)

### Partially Implemented
- MARK/TERM: parsed, auto-MARK for CONTENT works, explicit MARK/TERM working
- Character ranges: `|c[0-9]`, `|c[a-z]` not yet supported (use DIGIT, LETTER classes)

### Not Yet Implemented
- Character ranges: `|c[0-9]` → `b'0'..=b'9'`
- C template

### Recently Implemented
- [x] Return with value: `|return TypeName`, `|return TypeName(USE_MARK)`
- [x] Built-in /error: `/error`, `/error(CustomError)`
- [x] Combined char classes: `|c[LETTER'[.?!*+]` - match class OR literal chars
- [x] TERM adjustments: `TERM(-1)` - terminate slice before current position
- [x] PREPEND: `PREPEND(|)` - emit literal as text event
- [x] Inline literals: `TypeName`, `TypeName(literal)`, `TypeName(USE_MARK)`
- [x] PREV variable: Previous byte for context-sensitive parsing
- [x] Test harness: End-to-end Rust compilation and testing (12 tests)

## Testing Harness

End-to-end testing for generated parsers requires two levels:

### Level 1: Generator Correctness (Ruby-driven)
- Does descent produce correct Rust code for various `.desc` inputs?
- Basic "parse this input, get these events" verification

### Level 2: Runtime Behavior (Rust-native tests)
- Streaming semantics: `feed()` partial chunks, `finish()` for EOF
- Backpressure: `buffer_full` flag, consumer reading to unblock
- Buffer boundaries: token split across chunks
- Performance: criterion benchmarks

### Architecture

```
test/
  fixtures/
    minimal.desc / .input / .expected
    lines.desc / ...
  rust_harness/
    Cargo.toml
    src/
      lib.rs              # Re-exports generated parser module
      generated.rs        # Ruby writes generated parser here
      main.rs             # CLI: stdin → JSON events to stdout
    tests/
      streaming.rs        # Backpressure, partial feed, EOF
      boundaries.rs       # Tokens split across chunks
    benches/
      parse.rs            # Criterion benchmarks
```

### Ruby Tests
1. Generate parser → write to `generated.rs`
2. `cargo run < input.txt` → compare JSON to `.expected`

### Rust Tests (via `cargo test` in harness)
- Explicit tests for streaming edge cases
- Tests the *template's runtime code*, not individual grammars

## Future Enhancements

### Static Analysis
The IR provides enough structure for useful static analysis:

- **Infinite loop detection:** States where all cases self-loop with no exit path
- **Unreachable state detection:** States with no incoming transitions
- **Type consistency:** CONTENT functions that never MARK, etc.

### Bootstrap
The `.desc` format is valid UDON. When libudon is mature, descent can use the
UDON parser (that it generated!) to parse its own input format.

## Template Issues

### TERM span calculation - FIXED

~~When explicit TERM is used, `span_from_mark()` returns incorrect span because it uses
`self.pos` after advancing past the delimiter.~~

**Fixed:** `span_from_mark()` now uses the same logic as `term()` - respects `term_pos` when set.

## SCAN Optimization

### Chained memchr for 4-6 Chars - IMPLEMENTED

SCAN optimization now supports up to 6 exit characters via chained memchr calls:

| Chars | Implementation |
|-------|----------------|
| 1-3 | Single memchr/memchr2/memchr3 call |
| 4 | memchr3 + memchr, take min |
| 5 | memchr3 + memchr2, take min |
| 6 | memchr3 + memchr3, take min |

Two SIMD passes is still much faster than byte-by-byte for large text blocks.

**Example:** The markdown parser's `text/main` now gets SCAN with 5 exit chars (`` ` ``, `*`, `_`, `~`, `\n`).

### Beyond 6 Chars

For states with >6 exit chars, SCAN is not applied. Options if needed:
1. Restructure grammar to use fewer exit chars
2. Use specialized text functions per container (current markdown.desc approach)
3. Investigate aho-corasick for multi-pattern matching

## Performance Optimizations (from Codex review of libudon)

These optimizations may apply to descent-generated parsers:

### Zero-Copy Improvements
- **Zero-copy feed for single-chunk usage**: Accept `Bytes`/`Arc<[u8]>` or borrow mode
  to avoid `chunk.to_vec()` copies. The callback-based approach with `&'a [u8]` slices
  already achieves this for single-chunk, but streaming multi-chunk needs care.

### Allocation Reduction
- **Pre-intern static strings**: For known keys like `"$id"`, `"$class"`, suffix keys,
  and single-char literals, use static references rather than allocating per-event.
  Could generate a static lookup table in the template.

- **Eliminate String allocations in value parsing**: Use `lexical-core` or `fast_float`
  for float/complex number parsing instead of String intermediates.

### Unicode Handling
- **Cheaper unicode label detection**: Current approach may use `from_utf8` on remainder
  for every non-ASCII byte. Consider single-char decode (e.g., decode one codepoint,
  check if XID_Start/XID_Continue) rather than validating entire remainder.

### Indentation Handling
- **SPEC-INDENTS for multi-chunk feeds**: When input is streamed in chunks, indentation
  detection needs to handle chunk boundaries correctly. Document the invariants and
  ensure generated parsers handle partial lines at chunk boundaries.

### Not Applicable to Callback-Based
- **MaybeUninit ring slots**: This optimization is for ring-buffer architectures.
  The callback-based approach eliminates ring buffers entirely, so this doesn't apply.
  (Callback approach is already 2-7x faster than ring-buffer alternatives.)
