# Changelog

All notable changes to descent will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
