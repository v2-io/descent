# libdescent — Rust rewrite of descent — session trail

Assume 100% context turnover between sessions. This file is the state, the
decisions, and the next step. Session 1: 2026-07-07. Session 2: 2026-07-11.

## Session-2 headline

Front-end differential **green 20/20** (10 grammars × {tokens, ast}, Rust
port vs Ruby, jq-normalized JSON). udon-core reader spike **green 10/10**
(token-identical to the oracle on the whole corpus, 6,926 tokens) — the
udon-core-as-front-end construction is now *empirically validated*, with
three bridges (see `rust/spikes/udon-reader/NOTES.md` for the mismatch
classes, metrics, and classifications). Stage-0 vendored per policy
(`rust/vendor/udon-core/`, PROVENANCE.md). First normalization
oracle-verified (empty placeholder cells — byte-identical output; evidence
in `rust/spikes/normalizations/`, umbrella-side landing flagged, not done).
New evaluative criterion from Joseph recorded at
`~/src/udon/notes/desc-design-principles.md` (**read it**): .desc reads like
a bit lookup table; every proposal is judged on cursor-advance/table-scan
legibility, and every quirk classified *principled* vs *lexer artifact*.

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

Branch `rust-rewrite` in ~/src/descent. Workspace at `rust/`
(members: libdescent, descent-cli, spikes/udon-reader, vendor/udon-core).

- `rust/libdescent/src/lexer.rs` — **done, corpus-verified** (20/20 vs Ruby):
  faithful port of lexer.rb's three scanning layers. Role: differential
  oracle + the `Token` seam. `split_on_pipes`, `parse_part`,
  `extract_bracketed_id` are `pub` (reader seams).
- `rust/libdescent/src/ast.rs`, `parser.rs` — **done, corpus-verified**
  (AST JSON identical to Ruby on all 10 grammars).
- `rust/libdescent/src/dump.rs` — canonical JSON for tokens/AST; MUST stay in
  lockstep with `rust/tools/dump_tokens.rb` / `dump_ast.rb` (Ruby side).
- `rust/descent-cli` — `descent-rs tokens|ast FILE` dump subcommands.
- `rust/tools/diff_frontend.sh` — Rust-vs-Ruby differential (needs jq).
- `rust/tools/diff_reader.sh` — reader-vs-oracle differential.
- `rust/vendor/udon-core/` — vendored stage-0 (policy-compliant; see
  PROVENANCE.md there for SHAs + bump procedure).
- `rust/spikes/udon-reader/` — **the production front-end candidate,
  token-identical 10/10**: udon events → parts → shared `parse_part`.
  Bridges: sentinel pre-pass (quoted pipes/semicolons, quotes in brackets,
  comment blanking), raw-source bracket-id extraction, orphaned-pipe
  detection. NOTES.md there = mismatch classes + classifications + metrics.
- `rust/spikes/normalizations/` — oracle-blessed grammar normalization
  evidence (placeholder cells, so far).
- charclass / IR / ir_builder / emitter / templates — **not started**.
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
- Ruby parse_function: `id.split(':')` silently drops third+ colon segments
  of a function id ("a:b:c" → rtype "b") — mirrored in Rust for oracle
  fidelity; make it an error in the improved front-end.
- udon-core upstream defects found by the reader spike (flag to umbrella):
  Text span/content off-by-one on pipe-led runs; Text spans at line start in
  continuation mode; ElementEnd spans past the next line's pipe;
  "Inconsistent indentation" → text-mode degradation drops structural pipes;
  bracket-ids not scoped (space-fragmented, quoted-whitespace garbled).

## desc-format proposals (lexer-conformance artifacts → UDON-friendly .desc)

Joseph confirmed the frame: bridge-friction points are exactly where .desc
syntax conformed to what was easy for the Ruby lexer. **Evaluative criterion
(2026-07-11, `~/src/udon/notes/desc-design-principles.md`):** .desc began as
a literal bit-lookup table (RTMP parser, still running at Twitch); the
load-bearing property is table-scan/cursor-advance legibility. Classify every
quirk *principled* (serves the table — keep, spec it) vs *lexer artifact*
(accident of the Ruby scanner — normalize under the oracle rule or redesign).
Session-2 quantification: bridging .desc onto udon-core needed 999 sentineled
bytes + 82 degradation sites in combined.desc alone (full table in
`rust/spikes/udon-reader/NOTES.md`). Spiking on alternative constructs is
explicitly authorized — keep spikes in `rust/spikes/` with short notes.

0. **Empty placeholder cells in continuation rows** — VERIFIED lexer
   artifact, oracle-blessed removal (byte-identical output; evidence
   `rust/spikes/normalizations/combined-no-placeholder-cells.desc`). Ruby
   discards whitespace-only parts; the leading pipe of an empty first cell
   is pure alignment. Nuance: the pipe before the first real command IS
   load-bearing (terminates the previous part), and row-leading pipes may
   still win on table legibility — a fmt-policy choice, not grammar.
   Umbrella-side landing flagged, not done from this repo.

Inventory (1-8 from session 1, classifications added session 2):

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

Session-2 classifications (per the table-scan criterion): #2 #3 #4 #5 #6 #8
are **lexer artifacts** (normalize/redesign); #1 is **mixed** — the aligned
one-line case row is principled (it IS the table), the three implicit
splitters behind it are artifact; #7 is **artifact-born but keepable sugar**
(aliases like `<SQ>` may read better in a table cell than quoted literals —
Joseph's call). The reader-spike mismatch classes map onto these: dropped
pipes/degradation → #1+#5, quoted pipes → #1/#7, bracket-id quoting → #4,
semicolons → #1/#2 (details in `rust/spikes/udon-reader/NOTES.md`).

Endpoint: .desc as a **pure UDON dialect + schema**, bridge layer → zero;
grammars become first-class UDON documents that udon's own tooling (lint,
paths, schema, agentic edits) applies to. The fusion succeeds only if the
UDON rendering stays *at least as table-legible* as today's .desc (rendered
lookup-table views are part of the answer — see the design-principles note).

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

## Exact next steps (session 3, in order)

1. **Ruby-side context dump** (`rust/tools/dump_context.rb`):
   `Generator#build_context.to_json` per fixture (Ruby side untouched — new
   file under rust/tools only).
2. **charclass.rs (neutral parts) + ir.rs + ir_builder.rs ports**; then the
   emit/rust context-builder (port of transform_call_args_by_type +
   to_rust_byte/bytes — the ONLY place Rust literals get baked); verify via
   context-JSON diff vs Ruby on all 10 fixtures (extend diff_frontend.sh).
3. minijinja templates translated from parser.liquid/_command.liquid +
   filters (rust_expr etc.) + 4-regex post-process; converge to
   byte-identity on the 20 expected files (instrument, not contract — log
   deliberate divergences in the improvements ledger instead of chasing
   warts; the Liquid quirks list above is the map).
4. Real acceptance: regenerate udon's parser.rs with descent-rs, run udon
   fixture suite (`cd ~/src/udon/core && cargo test`), event-stream equal.
   Then the self-hosting fixed-point check (regenerate→rebuild→regenerate
   stable) — and promote the reader from spike to libdescent's default
   front-end (oracle lexer stays as differential fallback).
5. Ongoing, interleaved: more normalization spikes (per-proposal oracle
   checks — quote-alias adoption #7 and comment-rule #2 look nearest);
   classifications per the table-scan criterion; flag umbrella-side items
   (grammar edits + udon-core defect list) to the coordinator.
6. Keep commits local on `rust-rewrite`; no push (Joseph reviews).

## Oracle status per corpus (contract instrument)

- Front-end differential (Rust lexer+parser vs Ruby): **20/20 OK**
  (10 grammars × {tokens, ast}; `rust/tools/diff_frontend.sh`; harness
  verified non-vacuous via cross-fixture diff).
- udon-core reader vs oracle lexer: **10/10 token-identical**
  (`rust/tools/diff_reader.sh`; 6,926 tokens).
- Generated-output comparison: **not yet attempted** (emitter not built).
  Fixtures ready for all 10 grammars × {plain, trace}.
- Normalization checks run so far: placeholder-cells (byte-identical ✓).
