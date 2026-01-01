# descent TODO

## Priority Queue

### 1. Multi-Chunk Streaming - VERIFY WORKING

Resumable state machine for streaming input (design decided in session 3).
**This should already be implemented** - verify it works before other features.

If not working, this is the highest priority item.

See "Multi-Chunk Streaming" section below for full design.

### 2. Byte Literal Syntax Cleanup

See "Design Discussions Needed" section below. Needs resolution before adding
more syntax features - the current ad-hoc accretion is confusing and error-prone.

### 3. Unicode Identifiers

Use `unicode-ident` crate for XID_Start/XID_Continue. See section below for
decided approach with XID_START, XID_CONT, XLBL_START, XLBL_CONT classes.

### 4. Character Ranges

`|c[0-9]` → `b'0'..=b'9'`, `|c[a-f]` → `b'a'..=b'f'`
Currently parses as literal chars (broken). `values.desc` depends on this.

---

## Design Discussions Needed

### Byte Literal Syntax Inconsistency

**Problem:** The DSL has accumulated multiple ways to express byte/character literals
without a coherent grammar. This creates confusion and potential bugs.

**Current state (ad-hoc accretion):**

| Context | Syntax | Result |
|---------|--------|--------|
| Escape sequences | `<P>`, `<R>`, `<RB>` | Required for DSL-reserved chars |
| Quoted char | `'!'` | Works in call args |
| Double quoted | `"!"` | Works in call args |
| Bare punctuation | `!` | Now auto-converts in call args (0.2.8) |
| Magic zero | `0` | Means "no value" for PREPEND |
| In PREPEND | `PREPEND(!)` | Works (inside parens) |
| In PREPEND | `PREPEND(|)` | BROKEN - pipe terminates command |

**Risks:**
- `/func(x)` could mean variable `x` or byte literal `b'x'` depending on context
- No clear rule for when quoting is required
- Escape sequences only exist for some reserved chars
- Different contexts have different parsing rules

**Potential solutions:**

1. **Single canonical syntax**: Require `'x'` everywhere for byte literals
   - Pro: Unambiguous, familiar from other languages
   - Con: More verbose, breaking change

2. **Escape sequences for everything**: `<BANG>`, `<SEMI>`, `<STAR>`, etc.
   - Pro: Consistent with existing `<P>`, `<R>`
   - Con: Very verbose, many to memorize

3. **Document current behavior**: Accept the inconsistency, document clearly
   - Pro: No breaking changes
   - Con: Confusing, error-prone

4. **Byte literal prefix**: `b!`, `b|`, `b;` (like Rust's `b'x'` but terser)
   - Pro: Explicit, not too verbose
   - Con: New syntax to learn

**Discussion needed:** What's the right balance of explicitness vs terseness for a
DSL meant to be concise? Should we accept some ambiguity for brevity?

---

## libudon Integration Issues (BLOCKING)

Issues discovered during libudon integration.

---

## Session 3 Issues (values.desc feedback)

### 11. CONTENT + Inline Emit = Double Emit - **FIXED**

When a CONTENT-type function uses inline emit like `Float(USE_MARK)`, both events are emitted:

```rust
on_event(Event::Float { ... });    // inline emit
on_event(Event::Integer { ... });  // CONTENT auto-emit - shouldn't happen!
```

**Fix:** Added `mark_returns_after_inline_emits` in IR builder that detects inline emits
preceding bare returns and marks them with `suppress_auto_emit: true`. Template checks
this flag and skips CONTENT auto-emit.

### 12. CONTENT EOF Bypasses Inline Emits - **FIXED**

At EOF, the `|default` case actions (including inline emits) are bypassed entirely.

**Fix:** Use explicit `|eof` directive to specify EOF behavior with inline emits.
Example: `|eof |.end | TERM | Integer(USE_MARK) |return`

### 13. |eof Directive Generates Action Code - **FIXED**

The `|eof` directive now generates the specified action code:

```
|eof |.end | Integer(USE_MARK) |return
```

**Fix:**
1. IR builder now transforms eof_handler commands from AST to IR (was passing through untransformed)
2. Generator converts eof_handler commands to hashes (was passing IR objects directly)
3. Template checks `state.eof_handler` and `func.eof_handler` before using default EOF behavior
4. Same `suppress_auto_emit` fix applied to eof_handler commands

### 14. MARK on State Line Doesn't Work - **NOT IMPLEMENTED**

```
|state[:main] MARK    ; Expected to call self.mark() on state entry
```

**Expected:** MARK should be called when entering the state.

**Actual:** No `self.mark()` generated.

**Workaround:** Add `| MARK` in the action column of each entry row.

---

## Session 1 Issues (Items 1-5 FIXED)

### 1. Duplicate Error Codes - DONE
Multiple functions returning the same type with `expects_char` generate duplicate enum variants.
Example: `dquote_string:StringValue` and `squote_string:StringValue` both generate
`UnclosedStringValue`. Need deduplication at template level - collect unique
`(type_name, expects_char)` pairs when generating error enum.

**Status:** FIXED - Template now uses comma-delimited pattern matching to avoid partial string
matches (e.g., "Code" vs "CodeBlock"). See `parser.liquid` lines 55-66.

### 2. Local Variable Scoping Across States - DONE
Variables assigned in one state (e.g., `col = /count_indent` in `:children`) aren't visible in
subsequent states (`:check_child`, `:child_dispatch`). Multi-state functions use a `State` enum
and loop, but locals need to be declared at function scope, not inside state match arms.

**Status:** FIXED - Two issues resolved:
1. Parser was creating `{var, val}` but template expected `{var, expr}` - fixed in parser.rb
2. Function call assignments (`col = /count_indent`) now properly transformed via `rust_expr`
   filter in generator.rb (converts `/func(args)` to `self.parse_func(args, on_event)`)

### 3. Functions with No States - DONE
A function with just `| |return` (no explicit `|state[:]`) generates invalid Rust:
`enum State {}` and `State::;`. Should either:
- Validator rejects (require at least one state)
- Template handles gracefully (stateless function = simple sequential code, no loop/enum)

Graceful handling is preferred - immediate-return functions don't need state machines.

**Status:** FIXED - Template now handles `func.states.size == 0` by emitting appropriate
event (for BRACKET/CONTENT) or `return 0` (for INTERNAL) without any state machine.

### 5. `|return result` (Return with Value) - DONE
udon.desc uses `|return result` in `count_indent` for INTERNAL type functions that compute
and return values. TODO says "Return with value" is implemented, but needs verification
given other issues.

**Status:** FIXED - Three changes made:
1. IR builder's `parse_return_value` now recognizes lowercase variable names as return values
2. Template generates `-> i32` return type for INTERNAL functions
3. Command template generates `return varname;` for INTERNAL type returns
4. EOF handling returns `0` for INTERNAL types

---

## Session 2 Fixes (Codex Feedback + libudon Continued)

### Assignment Parsing - FIXED
Assignments like `depth = 1` were being parsed as `:raw` commands because the lexer
splits them so `depth` becomes the tag and `= 1` the rest. Fixed `classify_command`
to fall through to `parse_inline_command` for the combined `tag + rest`.

### parse_case Stopping Conditions - FIXED
Added `if`, `letter`, `label_cont`, `digit`, `hex_digit` to case_starters in `parse_case`
so conditional cases and character class cases aren't swallowed by the previous case.

### TERM(-n) Underflow - FIXED
Fixed `set_term` to use clamped i64 arithmetic instead of unsafe cast that could
panic when offset is negative at start of input.

### Lexer Pipe-in-Comments - FIXED
Added `strip_comments` method that runs BEFORE splitting on `|` to prevent
comments containing pipe characters from corrupting tokenization.

### Uppercase Character Classes - FIXED
Added regex check for SCREAMING_SNAKE_CASE in lexer to lowercase character class
names like `LETTER`, `LABEL_CONT` so they work as documented in README.

### Validator Bugs - FIXED
- Extract function name before `(` when validating calls (`element(COL)` → `element`)
- Fixed transition validation to warn on malformed targets

### /error(code) Custom Error Codes - FIXED
Custom error codes from `/error(NoTabs)` weren't being added to the ParseErrorCode enum.

Fix:
1. Added `custom_error_codes` field to IR::Parser
2. IR builder now collects error codes from `/error(code)` calls across all functions
3. Template includes custom error codes in ParseErrorCode enum

### Some() Pattern Matching Bug - FIXED
Empty chars array produced invalid `Some()` instead of valid pattern.

Fix: Added `{% elsif kase.chars.size > 1 %}` check before the for loop,
with fallback to `Some(_)` for edge cases with no chars.

### Unreachable Code After Return
Could not reproduce with current test examples. The structure of generated code
uses `return;` inside match arms which should not produce unreachable code.
May be specific to patterns in udon.desc - needs investigation with actual file.

---

### 7. Unicode Identifiers - **DECIDED** (Priority Queue #3)
`is_letter()` in generated code uses `b.is_ascii_alphabetic()`. UDON spec allows Unicode
XID_Start/XID_Continue for element names.

**Decision:** Use `unicode-ident` crate with these character class names:

| Class | Meaning |
|-------|---------|
| `XID_START` | Unicode XID_Start (can start identifier) |
| `XID_CONT` | Unicode XID_Continue (can continue identifier) |
| `XLBL_START` | = XID_START (same start rules for labels) |
| `XLBL_CONT` | XID_Continue + hyphen (for kebab-case labels) |

Implementation notes:
- Requires UTF-8 decoding on-demand (single codepoint, not full validation)
- Use `unicode_ident::is_xid_start(ch)` / `is_xid_continue(ch)`
- `XLBL_CONT` adds: `|| ch == '-'`
- Existing ASCII classes (`LETTER`, `DIGIT`, etc.) remain for byte-level matching

### 8. Parameterized Byte Terminators - **IMPLEMENTED**

Many functions are duplicated with only the terminator character differing
(e.g., `value` vs `value_inline`, different close brackets). Byte parameters
eliminate this duplication.

**Syntax:**
```
|function[bracketed] :close
  |c[:close]   | ->  |return        ; :close references the param
  |default     | /value(:close)  |>> :wait
```

Called as: `/bracketed(<R>)` or `/bracketed(<RB>)` or `/bracketed(<RP>)`

**Rules:**
- `:param` inside `|c[...]` references a byte parameter
- Type inferred from usage:
  - Used in `|c[:x]|` → `u8`
  - Used in arithmetic/conditions → `i32`
- If you need literal `:` in a character class, don't put it first
  (e.g., `|c[a:]|` not `|c[:a]|`)

**Implementation:** DONE
- IR::Case has `param_ref` field for parameter references
- IR::Function has `param_types` hash to track `u8` vs `i32` params
- IRBuilder detects `:param` in `|c[:close]|`, infers type from usage
- Generator transforms `:param` → `param`, `<RP>` → `b')'` in calls
- Template generates `Some(b) if b == param =>` for param_ref cases
- Added `<LP>` and `<RP>` escape sequences for `(` and `)`
- Example: `examples/param_test.desc`

### 9. Value Type Parsing - **DECIDED**

Numeric type detection is handled in `.desc` files (see `libudon/generator/values.desc`).
Keywords (true, false, null, nil) await phf integration (see "Keyword Matching with
Perfect Hash" in Future Enhancements).

**Approach:**
- Numeric parsing: descent handles via state machine in `.desc`
- Keyword lookup: phf perfect hash (O(1)) - pending implementation
- Complex post-processing (if needed): libudon with `lexical-core`

### 10. Multi-Chunk Streaming - **DECIDED** (Priority Queue #1 - VERIFY)

Current parser is single-buffer only (`&'a [u8]`). UDON needs streaming for LLM use case.
**This should already be implemented** - verify before other work.

**Decision:** Resumable State Machine (Option 2)

**API:**
```rust
loop {
    match parser.parse(chunk, on_event) {
        ParseResult::Complete => break,
        ParseResult::NeedMoreData => {
            chunk = get_next_chunk();  // Caller controls flow
        }
    }
}
```

**Design:**
- Zero-copy for 99% of input (tokens within chunks)
- Small internal buffer (~256 bytes) for cross-boundary tokens only
- Parser state already captured in `State` enum
- Add `mark_pos` / `term_pos` to saved state for resume
- When hitting end of chunk mid-token, return `NeedMoreData`
- Resume by prepending buffered bytes to next chunk

**Backpressure:**
- Blocking callback model (sufficient for LLM streaming)
- Intra-chunk: synchronous callback blocks → parser waits
- Inter-chunk: caller controls when to feed next chunk
- No explicit pause signal needed

**Implementation steps:**
1. Add `ParseResult` enum with `Complete` / `NeedMoreData` variants
2. Add small buffer for partial tokens at chunk boundary
3. Save/restore parser position state across chunks
4. Handle MARK/TERM spans that cross boundaries

---

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

### Not Yet Implemented
- Character ranges: See Priority Queue #4
- Unicode identifiers: See Priority Queue #3
- LINE variable: Current line number (1-indexed), like COL
  - Parser already tracks `self.line` internally
  - Just needs `LINE -> self.line as i32` in `rust_expr` filter (like COL/PREV)
- C template

### Recently Implemented
- [x] Character classes: `DIGIT`, `HEX_DIGIT` added (using Rust's is_ascii_digit/is_ascii_hexdigit)
- [x] |eof directive: Generates specified action code at EOF
- [x] Inline emit + return: No longer double-emits for CONTENT types
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

## Implemented Features

### Keyword Matching with Perfect Hash (phf) - IMPLEMENTED (0.2.9)

For keyword sets like `true`/`false`/`null`/`nil`, use phf perfect hash for O(1
compile-time lookup instead of verbose state-per-character functions.

**Syntax:**
```
|keywords[name] :fallback /fallback_function
  | true   => BoolTrue
  | false  => BoolFalse
  | null   => Nil
  | nil    => Nil
```

**Usage in parsing:**
```
|function[value]
  |state[:main]
    |LABEL_CONT |.cont  | ->                         |>>
    |default    |.done  | TERM | KEYWORDS(name)      |return
```

**Generates:**
```rust
static NAME_KEYWORDS: phf::Map<&'static [u8], u8> = phf_map! {
    b"true" => 0u8,
    b"false" => 1u8,
    b"null" => 2u8,
    b"nil" => 3u8,
};

fn lookup_name<F>(&mut self, on_event: &mut F) -> bool { ... }
fn lookup_name_or_fallback<F>(&mut self, on_event: &mut F) { ... }
```

**Example:** See `examples/keywords.desc`

**Benefits:**
- Single probe lookup, no state machine
- Works on `&[u8]` directly
- Scales to large keyword sets (gperf-style)
- Eliminates `kw_true`, `kw_false`, etc. boilerplate

---

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
