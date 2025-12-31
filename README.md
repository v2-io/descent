# descent

A recursive descent parser generator that produces high-performance callback-based
parsers from declarative `.desc` specifications.

## Philosophy

**The DSL describes *what* to parse. The generator figures out *how*.**

- **Type-driven emit**: Return types determine events, no explicit `emit()` needed
- **Inferred EOF**: No explicit `|eof` cases - behavior derived from context
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
# Generate Rust parser
descent --target rust parser.desc > parser.rs

# Generate C parser
descent --target c parser.desc -o parser

# Show help
descent --help
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

Called as: `/bracketed(<R>)` for `]`, `/bracketed(<RB>)` for `}`, `/bracketed(<RP>)` for `)`.

This eliminates duplicate functions that differ only in their terminator character.

### States

```
|function[element:Element] :col
  |state[:identity]
    |c[a-z]     |.name     | /name           |>> :after
    |default    |.anon     |                 |>> :content

  |state[:after]
    |c[\n]      |.eol      | ->              |>> :children
    |default    |.inline   | /text(col)      |>> :children
```

### Cases (Character Matching)

Each case has: `match | actions | transition`

```
|c[x]           | actions    |>> :state   ; Match single char
|c[\n]          | actions    |>>          ; Match newline (self-loop)
|c[ \t]         | actions    |return      ; Match space or tab
|c[abc]         | actions    |>> :next    ; Match any of a, b, c
|LETTER         | actions    |>> :name    ; Match ASCII letter (a-z, A-Z)
|LABEL_CONT     | actions    |>>          ; Match letter/digit/_/-
|DIGIT          | actions    |>> :num     ; Match ASCII digit (0-9)
|HEX_DIGIT      | actions    |>> :hex     ; Match hex digit (0-9, a-f, A-F)
|default        | actions    |>> :other   ; Fallback case
```

**Character escapes:**

| Syntax | Character |
|--------|-----------|
| `\n`   | Newline   |
| `\t`   | Tab       |
| `\\`   | Backslash `\` |
| `<BS>` | Backslash `\` (alternate) |
| `<P>`  | Pipe `\|` |
| `<L>`  | `[`       |
| `<R>`  | `]`       |
| `<LB>` | `{`       |
| `<RB>` | `}`       |
| `<LP>` | `(`       |
| `<RP>` | `)`       |

### Actions

Actions are pipe-separated, execute left-to-right:

```
| ->                    ; Advance one character
| ->[\n]                ; Advance TO newline (SIMD scan)
| MARK                  ; Mark position for accumulation
| TERM                  ; Terminate slice (MARK to current)
| /function             ; Call function
| /function(args)       ; Call with arguments
| var = value           ; Assignment
| var += 1              ; Increment
| PREPEND(literal)      ; Emit literal as Text event
| PREPEND(:param)       ; Emit parameter value as Text (no-op if empty)
```

`PREPEND()` with empty content is a no-op, as is `PREPEND(:param)` when the
parameter value is 0.

### Inline Literal Events

For emitting events directly with literal or accumulated content:

```
| TypeName              ; Emit event with no payload (BoolTrue, Nil)
| TypeName(literal)     ; Emit with literal value (Attr($id), Attr(?))
| TypeName(USE_MARK)    ; Emit using current MARK/TERM content
```

Examples:
```
|c[?]        | Attr(?) | BoolTrue | ->   |return   ; Emit Attr "?", BoolTrue
|c[[]        | Attr($id) | /value        |>> :next ; Emit Attr "$id", parse value
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

## Automatic Optimizations

### SCAN Inference

If a state has a self-looping default case (`|default | -> |>>`), the explicit
character cases become SCAN targets for SIMD-accelerated bulk scanning.

```
|state[:prose]
  |c[\n]      |.eol      | ...           |>> :next
  |c[<P>]     |.pipe     | ...           |>> :check
  |default    |.collect  | ->            |>>      ; ← triggers auto-SCAN
```

The generator detects this and uses `memchr` to scan for `\n` and `|` in bulk.

### EOF Handling

By default, the generator infers EOF behavior:

1. If `MARK` is active → finalize accumulation
2. If `EXPECTS(x)` declared → emit unclosed error
3. Based on return type:
   - `BRACKET` → emit End event
   - `CONTENT` → emit content event
   - `INTERNAL` / void → just return

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
    |c[\n]      |.blank    | ->           |>>
    |default    |.start    | /line        |>>

|function[line:Text]
  |state[:main]
    |c[\n]      |.eol      | ->           |return
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
    |c[\n]      |.blank    | ->              |>>
    |c[<P>]     |.pipe     | -> | /element(0)|>>
    |default    |.text     | /text(0)        |>>

|function[element:Element] :col
  |state[:identity]
    |LETTER     |.name     | /name           |>> :after
    |default    |.anon     |                 |>> :content

  |state[:after]
    |c[\n]      |.eol      | ->              |>> :children
    |default    |.text     | /text(col)      |>> :children

  |state[:children]
    |c[\n]      |.blank    | ->              |>>
    |c[ \t]     |.ws       | ->              |>> :check
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
    |c[\n]      |.eol      |return
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

## Targets

| Target | Status | Output |
|--------|--------|--------|
| Rust   | In progress | Single `.rs` file with callback API |
| C      | Planned | `.c` + `.h` files |

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
