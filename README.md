# `descent` Recursive Descent Parser Generator

A recursive descent parser generator that produces high-performance callback-based
parsers from declarative `.desc` specifications.

**See also:**
- [SYNTAX.md](SYNTAX.md) - Complete `.desc` DSL syntax reference
- [characters.md](characters.md) - Character, String, and Class literal specification

## Philosophy

**The DSL describes *what* to parse. The generator figures out *how*.**

- **Type-driven emit**: Return types determine events, no explicit `emit()` needed
- **Inferred EOF**: Default EOF behavior derived from context (`|eof` available for explicit control)
- **Auto SCAN**: SIMD-accelerated scanning inferred from state structure
- **True recursion**: Call stack IS the element stack
- **Minimal state**: Small functions compose into complex parsers

## Installation

```bash
gem install descent
```

Or in your Gemfile:

```ruby
gem 'descent'
```

## Usage

```bash
# Generate Rust parser (to stdout)
descent generate parser.desc

# Generate with output file
descent generate parser.desc -o parser.rs

# Generate with debug tracing enabled
descent generate parser.desc --trace

# Validate .desc file without generating
descent validate parser.desc

# Debug: inspect tokens, AST, or IR
descent debug parser.desc
descent debug --tokens parser.desc
descent debug --ast parser.desc

# Show help
descent --help
descent generate --help
```

## The `.desc` DSL

`.desc` files are valid UDON documents (enabling future bootstrapping). The format
uses pipe-delimited declarations.

### Document Structure

```
|parser myparser                    ; Parser name (required)

|type[Element] BRACKET              ; Type declarations
|type[Text]    CONTENT

|entry-point /document              ; Where parsing begins

|function[document]                 ; Function definitions
  |state[:main]
    ...
```

### Type System

Types declare what happens when a function returns:

| Category   | On Entry          | On Return                    |
|------------|-------------------|------------------------------|
| `BRACKET`  | Emit `TypeStart`  | Emit `TypeEnd`               |
| `CONTENT`  | `MARK` position   | Emit `Type` with content     |
| `INTERNAL` | Nothing           | Nothing (internal use only)  |

```
|type[Element]   BRACKET    ; ElementStart on entry, ElementEnd on return
|type[Name]      CONTENT    ; MARK on entry, emit Name with content on return
|type[INT]       INTERNAL   ; No emit - internal computation only
```

### Functions

```
|function[name]                     ; Void function (no auto-emit)
|function[name:ReturnType]          ; Returns/emits ReturnType
|function[name:Type] :param         ; With parameter
|function[name:Type] :p1 :p2        ; Multiple parameters
```

Functions returning `BRACKET` types automatically emit Start on entry, End on return.
Functions returning `CONTENT` types automatically MARK on entry, emit content on return.

**Parameterized byte matching:**

Parameters used in `|c[:param]|` become byte (`u8`) type, enabling functions to work
with different terminators:

```
; A single function handles [], {}, () by parameterizing the close char
|function[bracketed:Bracketed] :close
  |c[:close]   | ->  |return              ; Match against param value
  |default     | /content(:close)  |>>    ; Pass param to nested calls
```

Called as: `/bracketed(']')` for `]`, `/bracketed('}')` for `}`, `/bracketed(')')` for `)`.
(Legacy syntax `/bracketed(<R>)` etc. also works.)

This eliminates duplicate functions that differ only in their terminator character.

### States

```
|function[element:Element] :col
  |state[:identity]
    |LETTER     |.name     | /name           |>> :after
    |default    |.anon     |                 |>> :content

  |state[:after]
    |c['\n']      |.eol      | ->              |>> :children
    |default    |.inline   | /text(col)      |>> :children
```

### Cases (Character Matching)

Each case has: `match | actions | transition`

```
|c[x]           | actions    |>> :state   ; Match single char
|c['\n']        | actions    |>>          ; Match newline (self-loop)
|c[' \t']       | actions    |return      ; Match space or tab (quoted)
|c[abc]         | actions    |>> :next    ; Match any of a, b, c
|c[<0-9>]       | actions    |>> :digit   ; Match digit (using class syntax)
|c[<LETTER '_'>]| actions    |>> :ident   ; Match letter or underscore
|LETTER         | actions    |>> :name    ; Match ASCII letter (a-z, A-Z)
|LABEL_CONT     | actions    |>>          ; Match letter/digit/_/-
|DIGIT          | actions    |>> :num     ; Match ASCII digit (0-9)
|HEX_DIGIT      | actions    |>> :hex     ; Match hex digit (0-9, a-f, A-F)
|default        | actions    |>> :other   ; Fallback case
```

**Note:** Characters outside `/A-Za-z0-9_-/` must be quoted in `c[...]`:
- `c['"']` for double quote, `c['#']` for hash, `c['.']` for dot
- `c[<P>]` or `c['|']` for pipe (DSL delimiter)
- `c[<L>]` or `c['[']` for brackets (DSL delimiters)

### Character, String, and Class Literals

The DSL supports three literal types. See [characters.md](characters.md) for complete specification.

| Type | Syntax | Semantics |
|------|--------|-----------|
| **Character** | `'x'` | Single byte or Unicode codepoint |
| **String** | `'hello'` | Ordered sequence (decomposed to chars in classes) |
| **Class** | `<...>` | Unordered set of characters |

**Character class syntax** (`<...>`):

```
<abc>                 ; Bare lowercase decomposed: a, b, c
<'|'>                 ; Quoted character (for special chars)
<LETTER>              ; Predefined class
<0-9>                 ; Predefined range (digits)
<LETTER 0-9 '_'>      ; Combined: letters, digits, underscore
<:var>                ; Include variable's chars
```

**Predefined ranges:**

| Name          | Characters      |
| ------------- | --------------- |
| `0-9`         | Decimal digits  |
| `0-7`         | Octal digits    |
| `0-1`         | Binary digits   |
| `a-z`         | Lowercase ASCII |
| `A-Z`         | Uppercase ASCII |
| `a-f` / `A-F` | Hex letters     |

**Predefined classes:**

| Name         | Description                    |
| ------------ | ------------------------------ |
| `LETTER`     | `a-z` + `A-Z`                  |
| `DIGIT`      | `0-9`                          |
| `HEX_DIGIT`  | `0-9` + `a-f` + `A-F`          |
| `LABEL_CONT` | `LETTER` + `DIGIT` + `_` + `-` |
| `WS`         | Space + tab                    |
| `NL`         | Newline                        |

**Unicode classes** (requires `unicode-xid` crate):

| Name         | Description                   |
| ------------ | ----------------------------- |
| `XID_START`  | Unicode identifier start      |
| `XID_CONT`   | Unicode identifier continue   |
| `XLBL_START` | = `XID_START` (label start)   |
| `XLBL_CONT`  | `XID_CONT` + `-` (kebab-case) |

**DSL-reserved single-char classes:**

| Name | Char | Name | Char |
| ---- | ---- | ---- | ---- |
| `P`  | `\|` | `SQ` | `'`  |
| `L`  | `[`  | `DQ` | `"`  |
| `R`  | `]`  | `BS` | `\`  |
| `LB` | `{`  | `LP` | `(`  |
| `RB` | `}`  | `RP` | `)`  |

**Escape sequences** (in quoted strings):

| Syntax   | Result            |
| -------- | ----------------- |
| `\n`     | Newline           |
| `\t`     | Tab               |
| `\r`     | Carriage return   |
| `\\`     | Backslash         |
| `\'`     | Single quote      |
| `\xHH`   | Hex byte          |
| `\uXXXX` | Unicode codepoint |

### Actions

Actions are pipe-separated, execute left-to-right:

```
| ->                    ; Advance one character
| ->['\n']              ; Advance TO newline (SIMD scan, 1-6 chars)
| ->['"\'']             ; Advance TO first " or ' (multi-char scan)
| MARK                  ; Mark position for accumulation
| TERM                  ; Terminate slice (MARK to current)
| TERM(-1)              ; Terminate slice excluding last N bytes
| /function             ; Call function
| /function(args)       ; Call with arguments
| /error                ; Emit error event and return
| /error(CustomCode)    ; Emit error with custom code and return
| var = value           ; Assignment
| var += 1              ; Increment
| PREPEND('|')          ; Prepend literal to next accumulated content
| PREPEND('**')         ; Prepend multi-byte literal
| PREPEND(:param)       ; Prepend parameter bytes (empty = no-op)
```

**Advance-to (`->[...]`)**: SIMD-accelerated scan using memchr. Supports 1-6 literal
bytes only. Does NOT support character classes (LETTER, DIGIT) or parameter refs (:param).
Use quoted chars for special bytes: `->['\n\t']`, `->['|']`.

**Error handling (`/error`)**: Emits an Error event and returns. Custom error codes
are converted to PascalCase:

```
|c['\t']    | /error(no_tabs)   |return    ; Emits [Error, "NoTabs"]
|c[' ']     | /error(no_spaces) |return    ; Emits [Error, "NoSpaces"]
|default    | /error            |return    ; Emits [Error, "UnexpectedChar"]
```

`PREPEND` adds bytes to the accumulation buffer that will be combined with the
next `TERM` result. This is useful for restoring consumed characters during
lookahead. Parameters used in `PREPEND(:param)` become `&'static [u8]` type,
allowing empty (`<>`), single byte (`'x'`), or multi-byte (`'**'`) values.
Empty content is naturally a no-op.

### Inline Literal Events

For emitting events directly with literal or accumulated content:

```
| TypeName              ; Emit event with no payload (BoolTrue, Nil)
| TypeName('value')     ; Emit with literal value (e.g., Attr('$id'), Attr('?'))
| TypeName(USE_MARK)    ; Emit using current MARK/TERM content
```

Examples:
```
|c['?']      | Attr(?) | BoolTrue | ->   |return   ; Emit Attr "?", BoolTrue
|c[<L>]      | Attr($id) | /value        |>> :next ; Emit Attr "$id", parse value
|default     | TERM | Text(USE_MARK)     |return   ; Emit Text with accumulated
```

### Transitions

```
|>>                     ; Self-loop (stay in current state)
|>> :state              ; Go to named state
|return                 ; Return from function
```

### Conditionals

Single-line guards only (no block structure):

```
|if[COL <= col]         |return         ; If true, return
|                       |>> :continue   ; Else, continue
```

### Special Variables

| Variable | Meaning                              |
|----------|--------------------------------------|
| `COL`    | Current column (1-indexed)           |
| `LINE`   | Current line (1-indexed)             |
| `PREV`   | Previous byte (0 at start of input)  |

### Keywords (phf Perfect Hash)

For keyword matching (like `true`/`false`/`null`), use phf perfect hash for O(1) lookup:

```
|keywords[bare] :fallback /identifier
  | true   => BoolTrue
  | false  => BoolFalse
  | null   => Nil
  | nil    => Nil

|function[value]
  |state[:main]
    |LABEL_CONT |.cont  | ->                    |>>
    |default    |.done  | TERM | KEYWORDS(bare) |return
```

- `|keywords[name]` defines a keyword map
- `:fallback /function` specifies what to call if no match
- `| keyword => EventType` maps keywords to event types
- `KEYWORDS(name)` in actions triggers the lookup

Generates efficient phf_map! with O(1) compile-time perfect hash lookup.

## Automatic Optimizations

### SCAN Inference

If a state has a self-looping default case (`|default | -> |>>`), the explicit
character cases become SCAN targets for SIMD-accelerated bulk scanning.

```
|state[:prose]
  |c['\n']      |.eol      | ...           |>> :next
  |c[<P>]     |.pipe     | ...           |>> :check
  |default    |.collect  | ->            |>>      ; ← triggers auto-SCAN
```

The generator detects this and uses `memchr` to scan for `\n` and `|` in bulk.

### EOF Handling

By default, the generator infers EOF behavior based on return type:

- `BRACKET` → emit End event
- `CONTENT` → emit content event (finalizing any active MARK)
- `INTERNAL` / void → just return

The generator also infers unclosed-delimiter errors from the structure of return
cases (e.g., a string function that only returns on `"` will emit `UnclosedStringValue`
if EOF is reached).

For explicit control, use the `|eof` directive:

```
|function[number:Number]
  |state[:main]
    |DIGIT      |.digit   | ->                           |>>
    |default    |.done    | TERM | Integer(USE_MARK)     |return
    |eof        |.eof     | TERM | Integer(USE_MARK)     |return
```

This is useful when EOF should emit a different type than the function's return type,
or when you need specific actions at EOF (like inline emits).

## Example: Line Parser

```
|parser lines

|type[Text] CONTENT

|entry-point /document

|function[document]
  |state[:main]
    |c['\n']      |.blank    | ->           |>>
    |default    |.start    | /line        |>>

|function[line:Text]
  |state[:main]
    |c['\n']      |.eol      | ->           |return
    |default    |.collect  | ->           |>>
```

What the generator infers:
- `line` returns `CONTENT` type → MARK on entry, emit Text on return
- `line:main` has self-looping default → SCAN for `\n`
- EOF in `line` → emit accumulated Text, return
- EOF in `document` → just return (void function)

## Example: Element Parser

```
|parser elements

|type[Element] BRACKET
|type[Name]    CONTENT
|type[Text]    CONTENT

|entry-point /document

|function[document]
  |state[:main]
    |c['\n']      |.blank    | ->              |>>
    |c[<P>]     |.pipe     | -> | /element(0)|>>
    |default    |.text     | /text(0)        |>>

|function[element:Element] :col
  |state[:identity]
    |LETTER     |.name     | /name           |>> :after
    |default    |.anon     |                 |>> :content

  |state[:after]
    |c['\n']      |.eol      | ->              |>> :children
    |default    |.text     | /text(col)      |>> :children

  |state[:children]
    |c['\n']      |.blank    | ->              |>>
    |c[' \t']     |.ws       | ->              |>> :check
    |default    |.dedent   |return

  |state[:check]
    |if[COL <= col]        |return
    |c[<P>]     |.child    | -> | /element(COL) |>> :children
    |default    |.text     | /text(COL)        |>> :children

|function[name:Name]
  |state[:main]
    |LABEL_CONT |.cont     | ->              |>>
    |default    |.done     |return

|function[text:Text] :col
  |state[:main]
    |c['\n']      |.eol      |return
    |default    |.collect  | ->              |>>
```

What the generator produces:
- `element` returns `BRACKET` → emit `ElementStart` on entry, `ElementEnd` on return
- `name` returns `CONTENT` → MARK on entry, emit `Name` on return
- Recursive `/element(COL)` calls naturally handle nesting
- Column-based dedent via `|if[COL <= col]` unwinds the call stack

## Generated Code

descent generates callback-based parsers (2-7x faster than ring-buffer alternatives):

```rust
impl<'a> Parser<'a> {
    pub fn parse<F>(self, on_event: F)
    where
        F: FnMut(Event<'a>)
    {
        self.parse_document(&mut on_event);
    }

    fn parse_element<F>(&mut self, col: i32, on_event: &mut F) {
        on_event(Event::ElementStart { .. });
        // ... parse content ...
        // ... recursive calls for children ...
        on_event(Event::ElementEnd { .. });
    }
}
```

### Debug Tracing

Generate parsers with `--trace` to output detailed execution traces to stderr:

```bash
descent generate --trace parser.desc > parser.rs
```

Trace output shows the exact execution path through the parser:

```
TRACE: L14 ENTER document | byte='H' pos=0
TRACE: L16 document:main.collect | byte='H' term=[] pos=0
TRACE: L16 document:main.collect | byte='e' term=["H"] pos=1
TRACE: L15 document:main EOF | term=["Hello"] pos=5
```

Each line shows:
- **L14**: Source line number from the `.desc` file
- **document:main.collect**: Function name, state name, and case label (substate)
- **byte='H'**: Current byte being processed
- **term=["H"]**: Accumulated content in the term buffer
- **pos=0**: Current position in input

This is invaluable for debugging parser behavior and understanding how the
generated state machine processes input.

## Architecture

```
                    ┌──────────────┐
   parser.desc ───▶ │    Lexer     │ ───▶  Tokens
                    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │    Parser    │ ───▶  AST
                    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │  IR Builder  │ ───▶  IR (with inferred SCAN, etc.)
                    └──────────────┘
                           │
                           ▼
                    ┌──────────────┐       ┌─────────────────────┐
                    │  Generator   │◀──────│ templates/rust/     │
                    │   (Liquid)   │       │ templates/c/        │
                    └──────────────┘       └─────────────────────┘
                           │
                           ▼
                       parser.rs
```

- **Lexer**: Tokenizes pipe-delimited `.desc` format
- **Parser**: Builds AST from tokens (pure Data structs)
- **IR Builder**: Semantic analysis, SCAN inference, validation
- **Generator**: Renders IR through Liquid templates

**Adding new commands**: The parser uses `command_like?()` to detect command tokens
generically (uppercase words, `/function`, `->`, `>>`). To add a new command like `RESET`:

1. `parser.rb`: Add to `classify_command()` - what type is it?
2. `ir_builder.rb`: Add to command transformation - what args does it have?
3. `_command.liquid`: Add rendering - what Rust code does it generate?

No changes needed to case detection or structural parsing.

## Targets

| Target | Status | Output |
|--------|--------|--------|
| Rust   | Working | Single `.rs` file with callback API |
| C      | Not implemented | `.c` + `.h` files (planned) |

## Bootstrapping

The `.desc` format is valid UDON. When the UDON parser (generated by descent)
is mature, descent can use it to parse its own input format - the tool generates
the parser that parses its own specifications.

## Development

```bash
# Install dependencies
bundle install

# Run tests
dx test

# Lint
dx lint
dx lint --fix

# Build and install locally
dx gem install
```

## Related

- [libudon](https://github.com/josephwecker/libudon) - The UDON parser library (uses descent)
- [UDON Specification](https://github.com/josephwecker/udon) - The UDON markup language

## License

MIT
