# libdescent — Rust rewrite of descent — session trail

Assume 100% context turnover between sessions. This file is the state, the
decisions, and the next step. Session 1: 2026-07-07.

## Mission (as amended during session 1)

Rust rewrite of the descent parser generator. Original Phase-1 brief: parse
`.desc`, build a **target-neutral IR** (fix the Rust-literals-baked-into-IR
flaw from January), emit Rust byte-identical to Ruby descent. Three
mid-session amendments from Joseph/coordinator, all now in force:

1. **Byte-identity is demoted** from contract to *development instrument*.
   The real acceptance is semantic equivalence (event-stream-identical
   generated parsers on udon's fixture corpus) + better architecture. Where
   matching Ruby's bytes means reproducing a wart, do the right thing and
   record the divergence in the improvements ledger below.
2. **Templates are first-class**: the volatile generated-text lives in
   editable templates (history shows why: parser.liquid churned in 32 of the
   repo's commits, _command.liquid 20 — they co-evolve with every feature).
3. **Self-hosting bootstrap is the intended front-end** — and specifically
   (session-1 discovery, see below) via **udon-core**, not a separate
   descent.desc grammar.

## The architectural discovery of session 1 (read this first)

**.desc is UDON. udon-core — which descent itself generated — parses .desc
files natively.** Verified empirically (probes below). Therefore:

```
libdescent --compiles udon.desc--> parser.rs --becomes--> udon-core
     ^                                                       |
     └────────── parses .desc as libdescent's front-end ─────┘
```

The self-hosting fixed point closes at ecosystem level: libdescent links
udon-core as its front-end and regenerates udon-core's parser.rs; a
regenerate→rebuild→regenerate cycle must be stable. Stage-0 already exists
(the Ruby-generated parser.rs in ~/src/udon/core/udon-core/src/parser.rs),
so Ruby exits the toolchain entirely, rustc-bootstrap-style. No separate
descent.desc grammar is needed — that idea is **superseded**.

Probe evidence (run via `cargo run --example stdin_parse` in ~/src/udon/core):
- `|function[document:Text]` → `Name("function")`, id attr `BareValue("document:Text")`
- `|c[|]` → id `BareValue("|")` — pipe protected inside bracket-id
- `|c['\n']` → id `BareValue("'\n'")` — quotes preserved
- `|.collect` → element with class-attr "collect" (descent's substate)
- trailing `; comment` mid-line → Comment events (matches descent semantics)
- sameline command tails `| TERM | Text(USE_MARK)` → Text with pipes intact
  + spans, so raw text is always recoverable by slicing the source
- mixed granularity: `|return` (pipe+name-char) becomes a child *element*;
  `| ->` (pipe+space) becomes *text*. Reader must normalize both to parts.

**Front-end plan**: a thin reader consumes udon-core events/tree → descent
`Token`s (the existing seam in `libdescent/src/lexer.rs`): element names,
bracket-ids, comments, nesting from UDON structure; `rest` strings taken as
**raw span slices** (never UDON-value-interpreted — that's where descent
micro-syntax diverges); sameline tails re-split with the quote-aware
splitter already ported in lexer.rs. The hand-ported lexer stays as the
**differential oracle** (Ruby-equivalent tokens) and fallback, not the
production path.

## State of the code (what exists, what's verified)

Branch `rust-rewrite` in ~/src/descent. Workspace at `rust/`.

- `rust/libdescent/src/lexer.rs` — **done, compiles untested-against-corpus**:
  faithful port of lexer.rb's three scanning layers (documented inline as
  quirks). Role: differential oracle + the `Token` seam + the reusable
  quote-aware pipe splitter.
- `rust/libdescent/src/ast.rs`, `parser.rs` — **done, not yet
  corpus-verified**: port of parser.rb (AST + classify_command command
  regexes, incl. quirk mirrors — see ledger).
- `rust/libdescent/src/lib.rs` — **NOT YET WRITTEN** (crate won't build until
  a lib.rs declares the modules; session 2 first task).
- charclass / IR / ir_builder / emitter / templates / CLI — **not started**.
- `rust/tests/fixtures/` — **complete oracle corpus**: 10 .desc grammars
  (combined.desc = cat of udon's udon.desc+values.desc, + 9 descent
  examples), each with `.rs.expected` and `.trace.rs.expected` generated
  from Ruby descent (all 20 OK). Regenerate with:
  ```
  cd ~/src/descent && ruby -I lib -e 'require "descent";
    File.write(ARGV[1], Descent.generate(ARGV[0], target: :rust,
    trace: ARGV[2]=="true").gsub(/\n{3,}/, "\n\n"))' IN.desc OUT.rs true|false
  ```
- Note: shipped udon parser.rs differs from current-Ruby-descent output by
  exactly one word (`/// \`\`\`ignore` vs `\`\`\`text`, template drifted after
  last regeneration). Harmless; the oracle is current Ruby descent.

## Verified knowledge about the Ruby pipeline (read-once dividend)

Pipeline: lexer.rb (3 scan layers) → parser.rb (AST, regex command
classification) → ir_builder.rb (semantic analysis; **this is where Rust
literals get baked into IR** — `transform_call_args_by_type` +
`CharacterClass.to_rust_byte/bytes`, the January flaw) → generator.rb
(context hashes + Liquid filters incl. `rust_expr` transpiler) →
parser.liquid + _command.liquid → 4-regex post-process (strip ws-only lines;
collapse ALL blank runs; re-insert blank before `use|pub|impl` after
column-0 comment; blank after `}` before `//|#[|pub |fn `), then the driver
adds `\n{3,}→\n\n`.

Byte-identity-relevant quirks (matter only while the instrument is in use):
- Liquid 5.x compares Symbol==String as TRUE (`emit_mode == "mark"` works).
- Hash iteration order = insertion order (locals!); `{% include %}` shares
  outer scope; assigns persist across for-iterations.
- Command partial indents everything at 20 spaces regardless of nesting.
- Post-process mangles doc-comment examples (`/// }` triggers the
  blank-after-} rule) — visible in expected fixtures, lines ~213-222 of
  minimal.rs.expected.
- generator's `extract_local_init_values` uses a *mini* expr transpiler
  (COL/LINE/PREV/:param only — no escapes/char-literals).
- `analyze_helper_usage` misses COL/PREV inside conditional-clause
  *conditions* (only checks commands + case conditions) — a latent
  uses_col=false miscompile in Ruby.
- Keywords `fallback_args` are interpolated RAW into the template (no
  rust_expr).
- Entry point rendered via `remove: "/"`.

Target-neutral IR design (decided, not yet coded): IR keeps DSL-level facts
(chars as chars, conditions/exprs as DSL strings, call args as raw tokens +
resolved neutral param types i32/byte/bytes, prepend literals as bytes).
The **emitter's context-builder** does all Rust-literal rendering (port of
transform_call_args_by_type + to_rust_byte/bytes + rust_expr filters).
Template engine decision: **minijinja** (templates translated from Liquid;
mini-Liquid-engine option rejected after byte-identity was demoted).
Differential checkpoints available at every stage: tokens, AST, IR-context
JSON (`Generator#build_context.to_json` on the Ruby side — cheap to dump),
generated source bytes, and generated-parser event streams (udon's fixture
suite, the real acceptance).

## Improvements ledger (deliberate divergences from Ruby, when we get there)

- Fix `analyze_helper_usage` blind spot (conditional-clause conditions).
- Inline `/call(args)` in rest position silently drops args → make it an
  error (or support it).
- `emit()` with empty parens produces a nil-typed event → error.
- Unknown `:param` in `c[...]` silently matches literal ":name" → error.
- Post-process doc-comment mangling (the `/// }` blank-insertion) → fix.
- `unreachable_patterns` in generated matches → the determinism check
  (Phase-2 feature; REBOOT-PLAN H3 defers to it).
- prepend_values stored pre-Rust-escaped in Ruby IR (`'\\\\'` for `<BS>`) —
  unused by templates; keep neutral bytes in our IR.

## desc-format proposals (lexer-conformance artifacts → UDON-friendly .desc)

Joseph confirmed the frame: bridge-friction points are exactly where .desc
syntax conformed to what was easy for the Ruby lexer. Inventory so far:

1. **Sameline command soup** (`| -> |>> :next`): exists because Ruby splits
   the whole file on pipes before any structure. UDON-friendly: sameline
   command tails and **indent-nested child actions become two equivalent
   spellings of the same command sequence** (Joseph 2026-07-11: nesting can
   now be used liberally for multi-action cases; but the one-line form has a
   table-like quality that aids comprehension — udon.desc's aligned
   one-liner case rows are load-bearing legibility, so keep both, and let a
   future `udon fmt` normalize by policy rather than the grammar forcing
   either). Either way, the tail micro-syntax gets *one* specified
   quote-aware splitter instead of three implicit ones.
2. **Three inconsistent comment/quote/bracket scanners** (strip_comments:
   per-line-resetting bracket depth + both quotes; split_on_pipes: sticky
   single-level bracket + both quotes; parse_part: bracket+paren depth +
   single quotes only). Proposal: adopt UDON's single comment rule — probes
   show corpus-compatible behavior already.
3. **Tag capitalization dispatch** (SCREAMING=char-class, Pascal=emit,
   else=command) decided *in the lexer* by regex. Belongs in the semantic
   layer (where the Rust port put it); make the convention a schema fact.
4. **Bracket-id escaping bespokeness** (`c[|]`, `c[']']`, `->[']']`):
   udon's id-bracket quoting already covers corpus cases; define c[...] as
   UDON attr-id quoting, drop the bespoke nesting/quote rules.
5. **Multi-line parts** (part = text until next pipe, across newlines):
   pure split-first artifact. Line/indent-scoped parts (UDON-native) parse
   keywords-mapping blocks cleanly already.
6. **`/func(args)` tag capture via `[^)]*`** + the `func())` bare-paren
   hack in parse_call_value: one paren-matching rule, or UDON-native call
   syntax.
7. **Escape aliases `<P> <BS> <SQ>`...**: exist partly because bare `|`/`\`
   couldn't survive the Ruby pipe-splitter; with UDON quoting they could be
   ordinary quoted chars uniformly (keep aliases as sugar if liked).
8. **rest micro-syntaxes** (`:fallback /f`, `kw => Type`, `name:Type`) each
   have ad-hoc regexes; as a UDON dialect these become attrs/values (udon
   already parses `document:Text` as one clean BareValue).

Endpoint: .desc as a **pure UDON dialect + schema**, bridge layer → zero;
grammars become first-class UDON documents that udon's own tooling (lint,
paths, schema, agentic edits) applies to.

### Normalization rule (RATIFIED, coordinator 2026-07-11)

A .desc surface normalization may land **iff Ruby descent produces
byte-identical parser output from the normalized grammar** — un-freezes
surface while provably freezing semantics. Process: oracle-check it; land
as its own small commit (grammar change + evidence note); record in this
ledger with the quirk it eliminated. Constructs the oracle won't bless stay
bridged in the reader and become valve proposals. **Changes to
~/src/udon/core/generator/*.desc belong to the umbrella repo** — stage
separately from rust-rewrite and flag to the coordinating session (the
umbrella CI drift gate is satisfied by construction under this rule).

### Stage-0 policy (RATIFIED, coordinator 2026-07-11)

**Vendor the stage-0 parser.rs snapshot** in libdescent with source SHA +
generation provenance recorded (rustc-style); descent must build standalone
(it's an independent repo — a path dep on ~/src/udon breaks every clone but
this machine). A path-dep on ~/src/udon/core/udon-core may exist only as
local-dev convenience behind a feature/env gate; committed default build
uses the vendored copy. Bumping stage-0 is a deliberate recorded act (new
snapshot + udon-core commit + regeneration-stability check), never an
incidental sync. The udon-core-as-front-end construction itself is
**approved** (supersedes the earlier descent.desc plan).

## Open questions for Joseph

1. ~~Path-dep vs vendored stage-0~~ — RESOLVED: vendor (see above).
2. Which desc-format proposals may land early — RESOLVED in principle:
   any that pass the oracle-guarded normalization rule; the rest are valve
   proposals. Per-proposal oracle checks still to run.
3. Crate name for eventual publication (`descent` squatted; REBOOT-PLAN
   floats `descent-parser`, `udon-descent`). No urgency; publish=false.

## Exact next steps (session 2, in order)

1. `libdescent/src/lib.rs` declaring modules; `cargo build`; fix port
   compile errors.
2. Ruby-side dump scripts (`rust/tools/`): tokens-as-JSON and
   build_context-as-JSON. Differential-test lexer.rs+parser.rs tokens/AST
   against Ruby on all 10 fixtures. (Ruby side of repo stays untouched —
   new files under rust/tools only.)
3. **udon-core reader spike** (`rust/spikes/`): udon-core events → Tokens;
   diff against lexer.rs tokens corpus-wide; produce the mismatch list
   (feeds the proposals ledger + per-proposal oracle checks). Wire stage-0
   vendoring per the ratified policy (vendored parser.rs + provenance note;
   optional feature-gated path-dep for local iteration).
4. charclass.rs (neutral parts only) + ir.rs + ir_builder.rs ports;
   emit/rust context-builder (bakes Rust literals); verify via context-JSON
   diff vs Ruby on all fixtures.
5. minijinja templates translated from parser.liquid/_command.liquid +
   filters (rust_expr etc.) + post-process; converge to byte-identity on
   the 20 expected files (instrument, not contract — log deliberate
   divergences in the ledger instead of chasing warts).
6. Real acceptance: regenerate udon's parser.rs with descent-rs, run udon
   fixture suite (`cd ~/src/udon/core && cargo test`), event-stream equal.
   Then the self-hosting fixed-point check (regenerate→rebuild→regenerate
   stable).
7. Keep commits local on `rust-rewrite`; no push (Joseph reviews).

## Oracle status per corpus (contract instrument)

Generated-output comparison: **not yet attempted** (emitter not built).
Fixtures ready for all 10 grammars × {plain, trace}. Front-end differential:
not yet run. Nothing is verified beyond the probes and the fixture
generation itself.
