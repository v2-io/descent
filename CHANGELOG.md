# Changelog

All notable changes to descent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.7] - 2025-01-01

### Fixed
- **`:param` in conditionals**: `if[col <= :line_col]` now correctly generates
  `col <= line_col` instead of literal `:line_col`.
- **`<>` for `:byte` params**: Empty class now generates `0u8` (never-match sentinel)
  instead of `b'?'` which incorrectly matched question marks.
- **Function call arg validation**: `/func(param)` where `param` matches a known
  parameter now errors with helpful message suggesting `:param` or `'param'`.

## [0.6.6] - 2025-01-01

### Added
- **Unified CharacterClass parser**: New `CharacterClass` module implements the
  `characters.md` spec with consistent parsing everywhere (c[...], function args,
  PREPEND). All character class syntax now goes through a single code path.
- **Param reference validation**: Bare identifiers matching param names now raise
  helpful errors in both PREPEND and function calls:
  - `PREPEND(foo)` → suggests `PREPEND(:foo)` or `PREPEND('foo')`
  - `/func(foo)` → suggests `/func(:foo)` or `/func('foo')`
  - This prevents confusing bugs where param names are treated as literal strings

### Fixed
- **`<>` empty class consistency**: `<>` now correctly means "empty" everywhere:
  - `PREPEND(<>)` → `b""` (no-op, empty prepend)
  - `/func(<>)` for `:bytes` param → `b""` (empty byte slice)
  - Previously `PREPEND(<>)` incorrectly output literal `<>` characters
- **Type inference for numeric comparisons**: Conditions like `space_term == 0`
  no longer incorrectly type the param as `:byte`. Numeric flag comparisons stay
  as `:i32`; only character literal comparisons (e.g., `close == '|'`) set `:byte`.
- **`:byte` type propagation**: When function A passes `:param` to function B
  where B's param is `:byte`, A's param now correctly becomes `:byte`. Previously
  only `:bytes` was propagated.
- **Hex escapes in literals**: `'\x00'` and other hex escapes now work correctly
  in PREPEND and function arguments, producing actual byte values.

### Changed
- Removed duplicate constant definitions (PREDEFINED_RANGES, SINGLE_CHAR_CLASSES)
  in favor of unified CharacterClass module.
- `bytes_like_value?` now only matches `<>` - single-char values like `'|'` are
  typed based on usage, not call-site inference.

## [0.6.5] - 2024-12-31

### Fixed
- **PREPEND quote stripping**: `PREPEND('|')` now correctly generates `b"|"` (1 byte)
  instead of `b"'|'"` (3 bytes). Quoted literals are properly unquoted before embedding.
- **Lexer bracket extraction**: `c[']']` now works correctly - the lexer respects
  single quotes when extracting bracket content, so `]` inside quotes doesn't close.

### Changed
- **Stricter character validation**: Characters outside `/A-Za-z0-9_-/` in `c[...]`
  must now be quoted. This catches common errors and enforces consistent syntax:
  - `c["]` is invalid, use `c['"']`
  - `c[#]` is invalid, use `c['#']`
  - `c[abc]` is valid (alphanumeric)
  - `c[-_]` is valid (hyphen and underscore allowed bare)
- **Escape sequences outside class wrapper**: Using `<SQ>`, `<P>` etc. outside a
  `<...>` class wrapper now raises a clear error suggesting proper syntax.

## [0.6.3] - 2024-12-31

### Fixed
- **Semicolon in quoted strings**: `PREPEND(';')` no longer treats the semicolon
  as a comment start. The lexer now tracks quotes when stripping comments.
- **Pipe in quoted arguments**: Function calls like `/func('|')` now parse correctly.
  The lexer tracks quotes when splitting on pipe delimiters.

### Changed
- **Validation for character syntax**: Added comprehensive validation for `c[...]`
  patterns to catch unterminated quotes, bare special characters, and invalid
  legacy syntax before parsing.

## [0.6.2] - 2024-12-31

### Fixed
- **Conditionals in SCAN branches**: Character literals and escape sequences like
  `<P>` now work correctly in conditional expressions (e.g., `|if[PREV == <P>]`).
- **Escape sequences in expressions**: `rust_expr` filter now transforms embedded
  escape sequences like `<P>` to `b'|'` in all expression contexts.

## [0.6.1] - 2024-12-31

### Added
- **LINE variable**: Access current line number (1-indexed) in expressions.
  Transforms to `self.line as i32` in generated Rust code.

## [0.6.0] - 2024-12-31

### Changed
- **PREPEND semantics fixed**: PREPEND now correctly adds bytes to the accumulation
  buffer instead of emitting a separate Text event. The prepended content is combined
  with the next `term()` result using `Cow<[u8]>` for zero-copy in the common case.
- **Event content type**: Content fields in events are now `Cow<'a, [u8]>` instead of
  `&'a [u8]`. This enables zero-copy when no PREPEND is used, with owned data only
  when prepending is needed.

### Added
- **Unicode identifier classes**: `XID_START`, `XID_CONT`, `XLBL_START`, `XLBL_CONT`
  for Unicode-aware identifier parsing (requires `unicode-xid` crate)
- **Conditional unicode-xid import**: The crate is only required when Unicode
  classes are actually used in the parser

### Fixed
- **PREPEND buffer persistence**: The prepend buffer now persists across nested
  function calls, allowing `PREPEND(*) | /paragraph` patterns to work correctly

## [0.2.1] - 2024-12-30

### Added
- **DIGIT character class**: Matches ASCII digits (0-9) using `is_ascii_digit()`
- **HEX_DIGIT character class**: Matches hex digits (0-9, a-f, A-F) using `is_ascii_hexdigit()`
- **`|eof` directive**: Explicit EOF handling with custom actions and inline emits
- **Parameterized byte terminators**: Functions can take byte parameters for dynamic character matching
  - Syntax: `|c[:param]|` matches against parameter value
  - Parameters used in char matches become `u8` type automatically
  - Enables single functions to handle multiple bracket types ([], {}, ())
- **Escape sequences**: `<LP>` for `(` and `<RP>` for `)` in function arguments
- **PREPEND with parameter references**: `PREPEND(:param)` emits parameter value as Text event
  - `PREPEND()` with empty content is a no-op
  - `PREPEND(:param)` where param is 0 is also a no-op (runtime check)
  - Parameters used in PREPEND are inferred as `u8` type

### Fixed
- **Double emit bug (#11)**: CONTENT functions with inline emits no longer emit twice
  - Inline emit (e.g., `Integer(USE_MARK)`) followed by bare `|return` now correctly
    suppresses the auto-emit for the function's return type
- **EOF bypasses inline emits (#12)**: Use `|eof` directive for explicit EOF behavior
- **`|eof` not generating code (#13)**: The `|eof` directive now properly generates
  action code including inline emits
- **Quote characters in function parameters**: Bare `"` and `'` now correctly convert
  to `b'"'` and `b'\''` when passed as function arguments

### Changed
- EOF handling documentation updated to reflect explicit `|eof` support
- README and CLAUDE.md updated with all character classes and EOF directive

## [0.2.0] - 2024-12-29

### Added
- Parameterized functions with `:param` syntax
- Combined character classes: `|c[LETTER'[.?!]|` matches class OR literal chars
- TERM adjustments: `TERM(-1)` terminates slice before current position
- PREPEND command: `PREPEND(|)` emits literal as text event
- Inline literal emits: `TypeName`, `TypeName(literal)`, `TypeName(USE_MARK)`
- PREV variable for previous byte context
- Custom error codes via `/error(ErrorCode)`

### Fixed
- Duplicate error code generation for same return types
- Local variable scoping across states
- Functions with no states now handled gracefully
- Return with value for INTERNAL types

## [0.1.0] - 2024-12-20

### Added
- Initial release
- Lexer, Parser, IR Builder, Generator pipeline
- Rust code generation via Liquid templates
- SCAN optimization inference (memchr-based SIMD scanning)
- Type system: BRACKET, CONTENT, INTERNAL
- Character classes: LETTER, LABEL_CONT
- Automatic MARK/TERM for CONTENT types
- Recursive descent with true call stack
