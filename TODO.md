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

### Template Issues
- [ ] Clean up excessive whitespace in generated code
- [ ] Liquid deprecation warnings (use Environment instead of Template class methods)
- [ ] Handle function parameters in state machine (e.g., `col` parameter)

### Parser/Lexer Issues
- [x] `->` recognized as advance command (FIXED)
- [x] Character classes like `letter`, `label_cont` (FIXED)
- [x] Conditional cases with commands (FIXED)
- [x] `->[chars]` advance-to with escape processing (FIXED)

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
- MARK/TERM: parsed, auto-MARK for CONTENT works, explicit MARK/TERM in progress

### Not Yet Implemented
- Inline literals: `TypeName(literal)`, `TypeName(USE_MARK)`
- Combined char classes: `|c[LETTER'[.?!*+]`
- TERM adjustments: `TERM(-1)`
- PREPEND: `PREPEND(|)`
- Return with value: `|return value`
- Built-in /error
- C template

## Future Enhancements

### Static Analysis
The IR provides enough structure for useful static analysis:

- **Infinite loop detection:** States where all cases self-loop with no exit path
- **Unreachable state detection:** States with no incoming transitions
- **Type consistency:** CONTENT functions that never MARK, etc.

### Bootstrap
The `.desc` format is valid UDON. When libudon is mature, descent can use the
UDON parser (that it generated!) to parse its own input format.
