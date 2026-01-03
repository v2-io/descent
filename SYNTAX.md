# `.desc` DSL Syntax Reference

Complete syntax reference for descent parser specifications.

For character literal syntax, see [characters.md](characters.md).

---

## Document Structure

```
|parser <name>                    ; Required: parser name

|type[<TypeName>] <KIND>          ; Type declarations (zero or more)

|entry-point /<function>          ; Required: where parsing begins

|keywords[<name>] ...             ; Keyword blocks (zero or more)

|function[<name>] ...             ; Function definitions (one or more)
```

---

## Type Declarations

Types determine what events are emitted for functions returning that type.

```
|type[<Name>] <KIND>
```

| KIND     | On Entry          | On Return                  |
|----------|-------------------|----------------------------|
| BRACKET  | Emit `NameStart`  | Emit `NameEnd`             |
| CONTENT  | `MARK` position   | Emit `Name` with content   |
| INTERNAL | Nothing           | Nothing (internal use)     |

**Examples:**
```
|type[Element]   BRACKET    ; ElementStart on entry, ElementEnd on return
|type[Text]      CONTENT    ; MARK on entry, emit Text with span on return
|type[Counter]   INTERNAL   ; No emit - for internal values only
```

---

## Entry Point

```
|entry-point /<function>
```

Specifies which function begins parsing. The leading `/` is required.

---

## Functions

```
|function[<name>]                     ; Void function (no auto-emit)
|function[<name>:<Type>]              ; Returns/emits Type
|function[<name>:<Type>] :<param>     ; With one parameter
|function[<name>:<Type>] :p1 :p2      ; Multiple parameters
```

### Entry Actions

Actions on the function line execute once on function entry:

```
|function[<name>] | <action> | <action>
```

**Example:**
```
|function[brace_comment] | depth = 1
```

### Parameter Types

Parameter types are inferred from usage:

| Usage                  | Inferred Type       |
|------------------------|---------------------|
| `\|c[:param]`           | `:byte` (u8)        |
| `PREPEND(:param)`      | `:bytes` (&[u8])    |
| Arithmetic/conditions  | `:i32`              |

---

## States

```
|state[:<name>]
  <cases...>
```

States contain cases that match input and execute actions.

---

## Cases

Cases have the form: `|<match> |<substate> | <actions> |<transition>`

### Match Types

| Syntax        | Matches                              |
|---------------|--------------------------------------|
| `c[x]`        | Single character `x`                 |
| `c[abc]`      | Any of `a`, `b`, or `c`              |
| `c['\n']`     | Quoted character (newline)           |
| `c[<0-9>]`    | Character class (digits)             |
| `c[:param]`   | Byte parameter value                 |
| `LETTER`      | ASCII letter (a-z, A-Z)              |
| `DIGIT`       | ASCII digit (0-9)                    |
| `HEX_DIGIT`   | Hex digit (0-9, a-f, A-F)            |
| `LABEL_CONT`  | Letter, digit, `_`, or `-`           |
| `default`     | Fallback (any other byte)            |
| `eof`         | End of input                         |
| `if[<cond>]`  | Conditional guard                    |
| (empty)       | Bare action (unconditional)          |

### Substate Label

Optional label for debugging (appears in trace output):

```
|c[x] |.found    | -> |>> :next    ; .found is the substate label
```

---

## Actions

Actions are pipe-separated and execute left-to-right:

| Action               | Description                              |
|----------------------|------------------------------------------|
| `->`                 | Advance one byte                         |
| `->[<chars>]`        | Advance TO first occurrence (SIMD scan)  |
| `MARK`               | Mark position for accumulation           |
| `TERM`               | Terminate slice (MARK to current)        |
| `TERM(-N)`           | Terminate excluding last N bytes         |
| `/<func>`            | Call function                            |
| `/<func>(<args>)`    | Call with arguments                      |
| `/error`             | Emit error, return                       |
| `/error(<Code>)`     | Emit error with custom code, return      |
| `<var> = <value>`    | Assignment                               |
| `<var> += <N>`       | Increment                                |
| `PREPEND('<lit>')`   | Prepend literal to accumulation          |
| `PREPEND(:param)`    | Prepend parameter bytes                  |
| `KEYWORDS(<name>)`   | Lookup in keyword map                    |
| `<Type>`             | Emit event with no payload               |
| `<Type>('<lit>')`    | Emit event with literal value            |
| `<Type>(USE_MARK)`   | Emit event with accumulated content      |

### Advance-To Constraints

`->[<chars>]` uses SIMD memchr and supports:
- 1-6 literal bytes only
- Quoted characters: `->['\n\t']`
- NO character classes, NO parameter refs

---

## Transitions

| Syntax         | Description                      |
|----------------|----------------------------------|
| `\|>>`          | Self-loop (stay in current state)|
| `\|>> :<state>` | Go to named state                |
| `\|return`      | Return from function             |

---

## Conditionals

Single-line guards (no block structure):

```
|if[<condition>] | <actions> |<transition>
```

Followed by fallthrough case:
```
|if[COL <= :col]  |return
|                 |>> :continue    ; else branch
```

### Condition Syntax

- Comparisons: `==`, `!=`, `<`, `<=`, `>`, `>=`
- Variables: `COL`, `LINE`, `PREV`, `:param`, local vars
- Parentheses allowed: `(COL == 1)`

---

## Special Variables

| Variable | Type  | Description                          |
|----------|-------|--------------------------------------|
| `COL`    | i32   | Current column (1-indexed)           |
| `LINE`   | i32   | Current line (1-indexed)             |
| `PREV`   | byte  | Previous byte (0 at start)           |

---

## Keywords

Perfect hash lookup for keyword matching:

```
|keywords[<name>] :fallback /<func>
  | <keyword> => <EventType>
  | <keyword> => <EventType>
```

**Usage:**
```
|default | TERM | KEYWORDS(<name>) |return
```

**Example:**
```
|keywords[bare] :fallback /identifier
  | true   => BoolTrue
  | false  => BoolFalse
  | null   => Nil
```

---

## Comments

Semicolon starts a comment (rest of line ignored):

```
|parser test   ; this is a comment
```

---

## Character Classes

See [characters.md](characters.md) for complete specification.

### Quick Reference

| Syntax      | Description                    |
|-------------|--------------------------------|
| `'x'`       | Single character               |
| `'\n'`      | Escape sequence                |
| `'\xHH'`    | Hex byte                       |
| `<abc>`     | Character class (a, b, c)      |
| `<0-9>`     | Predefined range (digits)      |
| `<LETTER>`  | Predefined class               |
| `<P>`       | DSL-reserved char (`\|`)       |

### Predefined Classes

| Name         | Characters                     |
|--------------|--------------------------------|
| `LETTER`     | a-z, A-Z                       |
| `DIGIT`      | 0-9                            |
| `HEX_DIGIT`  | 0-9, a-f, A-F                  |
| `LABEL_CONT` | LETTER + DIGIT + `_` + `-`     |
| `WS`         | Space + tab                    |
| `NL`         | Newline                        |

### DSL-Reserved Escapes

| Name | Char | Name | Char |
|------|------|------|------|
| `P`  | `\|` | `SQ` | `'`  |
| `L`  | `[`  | `DQ` | `"`  |
| `R`  | `]`  | `BS` | `\`  |
| `LB` | `{`  | `LP` | `(`  |
| `RB` | `}`  | `RP` | `)`  |

---

## Complete Example

```
|parser json_value

|type[StringValue] CONTENT
|type[Object] BRACKET

|entry-point /value

|keywords[kw] :fallback /identifier
  | true  => BoolTrue
  | false => BoolFalse
  | null  => Nil

|function[value]
  |state[:dispatch]
    |c['"']     | -> | /string_value  |return
    |c['{']     | -> | /object        |return
    |LETTER     | /bare_keyword      |return
    |default    | /error             |return

|function[string_value:StringValue]
  |state[:main]
    |c['"']     | ->                  |return     ; Close quote
    |c['\\']    | -> | ->             |>>         ; Escape: skip 2
    |default    | ->                  |>>         ; Collect

|function[object:Object]
  |state[:main]
    |c['}']     | ->                  |return     ; Close brace
    |c['"']     | -> | /string_value  |>> :after
    |WS         | ->                  |>>
    |default    | /error             |return

  |state[:after]
    |c[':']     | -> | /value         |>> :comma
    |WS         | ->                  |>>
    |default    | /error             |return

  |state[:comma]
    |c[',']     | ->                  |>> :main
    |c['}']     | ->                  |return
    |WS         | ->                  |>>
    |default    | /error             |return

|function[bare_keyword]
  |state[:main]
    |LETTER     | ->                  |>>
    |default    | TERM | KEYWORDS(kw) |return
```
