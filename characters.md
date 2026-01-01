# Character, String, and Class Literals

This document specifies the type system for character data in descent `.desc` files.

## Three Types

| Type | Syntax | Semantics |
|------|--------|-----------|
| **Character** | `'x'` | Single byte or Unicode codepoint |
| **String** | `'hello'` | Ordered sequence of characters |
| **Class** | `<...>` | Unordered set of characters |

## Character Literals

A single character enclosed in single quotes:

```
'x'           ; ASCII character
'\''          ; Escaped single quote
'\\'          ; Escaped backslash
'\n'          ; Newline
'\t'          ; Tab
'\r'          ; Carriage return
'\xHH'        ; Hex byte (e.g., '\x7F')
'\uXXXX'      ; Unicode codepoint (e.g., '\u2042')
```

## String Literals

Multiple characters in single quotes. Order is preserved:

```
'hello'       ; 5-character string
'a'           ; Also valid as single-char string (coerces to Character)
';-|$\''      ; String with escaped quote
```

## Character Classes

Angle brackets `<...>` define an unordered set of characters:

```
<>                    ; Empty class (never matches)
<abc>                 ; = <'a' 'b' 'c'> (bare lowercase = decomposed string)
<'abc'>               ; Same as above (explicit string, decomposed)
<LETTER>              ; Predefined class
<0-9 a-f>             ; Predefined "ranges"
<LETTER 0-9 '_'>      ; Combined: letters, digits, underscore
<:var>                ; From variable (see below)
<P SQ>                ; Pipe + single-quote (predefined single-char classes)
```

### Bare Lowercase Shorthand

Inside `<...>`, bare lowercase letters (no spaces, no special chars) are treated
as a string and decomposed into the class:

```
<abc>         ; = <'a' 'b' 'c'>
<abc def>     ; = <'a' 'b' 'c' 'd' 'e' 'f'> (space separates tokens)
```

**Restrictions on bare form:**
- No special characters (brackets, quotes, pipe, etc.)
- No mixing bare and quoted in same token: `<ab'c>` is an ERROR
- Use predefined classes or quotes for special chars

```
; These are ERRORS:
<ab'c>        ; Mixed bare and quoted
<ab[c>        ; Bracket in bare

; Use instead:
<abc SQ>      ; abc + single quote
<abc LB>      ; abc + left bracket
```

### Whitespace in Classes

Whitespace requires quoting:

```
<' '>         ; Space only
<' \t'>       ; Space and tab (in one quoted string)
<' ' '\t'>    ; Same (two quoted chars)
<WS>          ; Predefined whitespace class
```

### Variable References in Classes

Variables can be included with `:name` syntax:

```
<:c>                  ; Include variable c
<:c 0-9 '|'>          ; Variable + digits + pipe
```

Type coercion for variables:
- If `:c` is a Character → added to class
- If `:c` is a String → decomposed, chars added to class
- If `:c` is a Class → merged into this class

## Predefined Classes

### Character Ranges (as class names)

| Name | Characters | Notes |
|------|------------|-------|
| `0-9` | `0123456789` | Decimal digits |
| `0-7` | `01234567` | Octal digits |
| `0-1` | `01` | Binary digits |
| `a-z` | `abcdefghijklmnopqrstuvwxyz` | Lowercase ASCII |
| `A-Z` | `ABCDEFGHIJKLMNOPQRSTUVWXYZ` | Uppercase ASCII |
| `a-f` | `abcdef` | Lowercase hex |
| `A-F` | `ABCDEF` | Uppercase hex |

### ASCII Character Classes

| Name | Description |
|------|-------------|
| `LETTER` | `a-z` + `A-Z` |
| `DIGIT` | `0-9` |
| `HEX_DIGIT` | `0-9` + `a-f` + `A-F` |
| `LABEL_CONT` | `LETTER` + `DIGIT` + `_` + `-` |
| `WS` | Space + tab |
| `NL` | Newline (`\n`) |

### Unicode Character Classes

Requires `unicode-xid` crate in generated Rust code.

| Name | Description |
|------|-------------|
| `XID_START` | Unicode XID_Start (can start identifier) |
| `XID_CONT` | Unicode XID_Continue (can continue identifier) |
| `XLBL_START` | = `XID_START` (label start) |
| `XLBL_CONT` | `XID_CONT` + `-` (label continue, for kebab-case) |

**Note:** These match Unicode codepoints, not bytes. When used, the parser
decodes UTF-8 on demand (single codepoint, not full validation).

### Single-Character Classes (DSL-Reserved)

| Name | Character | Notes |
|------|-----------|-------|
| `P` | `\|` | Pipe (column separator) |
| `L` | `[` | Left bracket |
| `R` | `]` | Right bracket |
| `LB` | `{` | Left brace |
| `RB` | `}` | Right brace |
| `LP` | `(` | Left paren |
| `RP` | `)` | Right paren |
| `SQ` | `'` | Single quote |
| `DQ` | `"` | Double quote |
| `BS` | `\` | Backslash |

These exist because these characters have special meaning in the DSL syntax.

## Type Coercion

### Implicit Coercion

| From | To | Rule |
|------|----|------|
| Character | String | Trivial (single-char string) |
| Character | Class | Trivial (single-element class) |
| String | Class | Decompose into constituent chars (order lost) |

### Errors (No Coercion)

| From | To | Why |
|------|----|-----|
| Class | String | No defined order |
| Class | Character | Undefined if size ≠ 1 |
| Integer | Character | Use explicit conversion if needed |

## Usage Contexts

### In `c[...]` (Character Matching)

The `c[...]` form is shorthand for `c[<...>]`:

```
|c[abc]               ; = |c[<abc>]
|c[LETTER '_']        ; = |c[<LETTER '_'>]
|c[0-9 a-f]           ; = |c[<0-9 a-f>]
|c[:close]            ; Match against variable
```

### In Function Calls

```
/func('|')            ; Pass character
/func('hello')        ; Pass string
/func(<P ';'>)        ; Pass class (pipe + semicolon)
/func(:var)           ; Pass variable (type preserved)
```

### In PREPEND / Inline Emit

These expect String (ordered). Character coerces to single-char string:

```
PREPEND('|')          ; Emit pipe
PREPEND('**')         ; Emit two asterisks
Text('hello')         ; Emit as Text event
```

**ERROR:** Cannot use Class where String expected:

```
PREPEND(<LETTER>)     ; ERROR - class has no order
```

## Reserved Variables

These are predefined by descent (SCREAMING_CASE):

| Name | Type | Description |
|------|------|-------------|
| `COL` | Integer | Current column (1-indexed) |
| `LINE` | Integer | Current line (1-indexed) |
| `PREV` | Integer | Previous byte (0 at start) |

User-defined variables use `snake_case`:

```
|function[example] :close :depth
  ; :close and :depth are user-defined parameters
```

## Migration from Current Syntax

| Old | New | Notes |
|-----|-----|-------|
| `<P>` | `<P>` or `'|'` | Both work |
| `<R>` | `<R>` or `']'` | Both work |
| `<RB>` | `<RB>` or `'}'` | Both work |
| `/func(124)` | `/func('|')` | Prefer quoted literal |
| `PREPEND(\|)` | `PREPEND('|')` | Prefer quoted literal |
| `|c[abc]` | `|c[abc]` | Unchanged |
| `|c[LETTER'[.?!]` | `|c[LETTER L '.' '?' '!']` | Explicit |

## Examples

```
; Character class with letters, digits, underscore, hyphen
|c[LETTER 0-9 '_-']

; Match against parameter (character or class)
|function[quoted] :close
  |c[:close]        | TERM | -> |return

; Called with character:
/quoted('\'')       ; Close on single quote

; Called with class:
/quoted(<'"' '\''>)   ; Close on either quote type

; String literal with escapes
PREPEND(';-|\$\'')    ; Emits ;-|$'

; Unicode class
|XID_START            ; Match Unicode identifier start
|c[XID_CONT '-']      ; Match identifier continue or hyphen
```
