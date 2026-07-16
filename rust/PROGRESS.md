# descent-core — Rust rewrite of descent — session trail

Assume 100% context turnover between sessions. This file is the state, the
decisions, and the next step. Session 1: 2026-07-07. Sessions 2-4:
2026-07-11.

## Session-4 headline

**Templates done, acceptance trio green, self-hosting fixed point closed.**
minijinja templates (`rust/descent-core/templates/rust/{parser,_command}.j2`,
translated from the Liquid pair) + `emit::rust::engine` (filter ports,
Liquid-parity semantics, 4-regex post-process) + `descent-rs generate FILE
[--trace] [--oracle]` + `rust/tools/diff_generate.sh`: **20/20
byte-identical** to the Ruby oracle fixtures (plain+trace) — no deliberate
output divergence was needed; identity achieved exactly. **Acceptance:**
(1) descent-rs regenerates udon's committed parser.rs **byte-identically**
from `core/generator/udon.desc+values.desc`; (2) udon suite green (83
tests, `cd ~/src/udon/core && cargo test --workspace`); (3) **self-hosting
fixed point holds**: reader promoted from spike to `descent-core::reader`
(default front-end; oracle lexer behind `Frontend::OracleLexer` /
`--oracle`), and stage-0 (Ruby-generated vendored parser.rs) == stage-1
(descent-core generating through stage-0) == stage-2 (after
rebuild-with-stage-1), plain and trace — Ruby has exited the toolchain.

Mid-session base reconciliation (coordinator directive): merged descent
origin/main **3f81c3c** (U1 defect sweep: char-aware columns, SCAN vetoed
by param/class cases, PREPEND span restoration) — the scannability fix
ported into `ir_builder.rs`, the parser.liquid changes mirrored into
parser.j2, all 20 fixtures regenerated from updated Ruby, front-end
differential re-proven 30/30. Stage-0 bumped to udon-core @ **d0bc9f9**
(deliberate act per policy, PROVENANCE.md updated); reader token-identity
re-verified 10/10 on the new events; `combined.desc` refreshed to the
current grammar pair (`combined.rs.expected` == udon's committed
parser.rs).

minijinja gotchas learned (they cost the only iteration cycles):
`preserve_order` feature REQUIRED (else map iteration sorts keys — locals
declaration order); `set_keep_trailing_newline(true)`; custom formatter for
Liquid's nil→"" (minijinja prints "none"); `ltruthy` test for Liquid
truthiness ("" and 0 are truthy); `lsize`/`ldefault`/`dstr` filters for
nil-tolerant size/default/concat. Templates are embedded `include_str!`
(standalone builds; editing a template = rebuild).

## Session-3 headline

**Context differential green 10/10** (30/30 counting tokens+ast+context;
`rust/tools/diff_frontend.sh`, non-vacuous, trace=true also identical).
charclass/ir/ir_builder ported; the January flaw is fixed as designed: IR is
target-neutral, and `emit::rust::build_context` (port of Ruby's
`Generator#build_context` PLUS `transform_call_args_by_type`) is the only
place Rust literals are rendered — producing context JSON identical to
Ruby's despite the relocation. Two normalization spikes landed
(`rust/spikes/normalizations/NOTES.md`): **#7 quote-aliases ORACLE-BLESSED**
(byte-identical plain+trace; reader sentinel bridge 999→825; token-identity
preserved) and **#2 comment-rule audit** (97.5%+ corpus agreement; every
disagreement is class-4 quoted-`;` or class-1 degradation, never the rule —
nothing to normalize, proposal confirmed). Templates NOT started (next).

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
`~/src/udon/design/desc-design-principles.md` (**read it**): .desc reads like
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
descent-core --compiles udon.desc--> parser.rs --becomes--> udon-core
     ^                                                       |
     └────────── parses .desc as descent-core's front-end ─────┘
```

The self-hosting fixed point closes at ecosystem level: descent-core links
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
`Token`s (the existing seam in `descent-core/src/lexer.rs`): element names,
bracket-ids, comments, nesting from UDON structure; `rest` strings taken as
**raw span slices** (never UDON-value-interpreted — that's where descent
micro-syntax diverges); sameline tails re-split with the quote-aware
splitter already ported in lexer.rs. The hand-ported lexer stays as the
**differential oracle** (Ruby-equivalent tokens) and fallback, not the
production path.

## State of the code (what exists, what's verified)

Branch `rust-rewrite` in ~/src/descent. Workspace at `rust/`
(members: descent-core, descent-cli, spikes/udon-reader, vendor/udon-core).

- `rust/descent-core/src/lexer.rs` — **done, corpus-verified** (20/20 vs Ruby):
  faithful port of lexer.rb's three scanning layers. Role: differential
  oracle + the `Token` seam. `split_on_pipes`, `parse_part`,
  `extract_bracketed_id` are `pub` (reader seams).
- `rust/descent-core/src/ast.rs`, `parser.rs` — **done, corpus-verified**
  (AST JSON identical to Ruby on all 10 grammars).
- `rust/descent-core/src/dump.rs` — canonical JSON for tokens/AST; MUST stay in
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
  evidence + NOTES.md (placeholder cells #0, quote-aliases #7, comment-rule
  audit #2; `comment_audit` probe binary lives in spikes/udon-reader).
- `rust/descent-core/src/charclass.rs` — **done, corpus-verified**:
  CharacterClass port MINUS the Rust-literal renderers (those live in
  emit::rust::literals — the January-flaw fix).
- `rust/descent-core/src/ir.rs`, `ir_builder.rs` — **done, corpus-verified**
  (via context differential): target-neutral IR. Deliberate divergences
  from Ruby, context-JSON-neutral by construction:
  (a) `transform_call_args_by_type` NOT run in the builder (moved to emit);
  (b) prepend_values stored as neutral bytes (`<BS>` -> `\`, not `\\`);
  emit::rust re-escapes. Command args are the Ruby args-hash as a JSON
  object with raw DSL values; conditional clauses hoisted into a typed
  `clauses` field, re-nested at serialization.
- `rust/descent-core/src/emit/rust/` — **context builder done,
  corpus-verified** (`build_context` + call-arg transform + literals.rs).
  Reproduces Ruby quirks on purpose: states-only call-arg transform
  (function-level eof handlers + entry actions keep RAW args), helper-usage
  COL/PREV blind spot, mini init-value transpiler, pre-escaped prepend
  values. `descent-rs context FILE [trace]` dumps it; Ruby side is
  `rust/tools/dump_context.rb` (Generator#build_context via #send).
- `rust/descent-core/templates/rust/{parser,_command}.j2` +
  `src/emit/rust/engine.rs` — **done, byte-identical 20/20** (plain+trace):
  minijinja env (Chainable undefined, keep_trailing_newline, nil→""
  formatter), filter ports (rust_expr/pascalcase/escape_rust_char +
  ltruthy/ldefault/lsize/dstr Liquid-parity helpers), `render_command()`
  global fn in place of `{% include 'command' %}` (recursion + the 20-space
  indent quirk preserved), post_process = the 4 regexes + driver collapse.
- `rust/descent-core/src/reader.rs` — **promoted production front-end**
  (moved from spikes/udon-reader, which keeps a re-export shim + probes).
  `Frontend::{UdonCore (default), OracleLexer}` on
  tokenize/parse_with/build_ir_with; CLI `--oracle` selects the lexer
  (diff_reader.sh uses it for the reference side).
- `rust/tests/fixtures/` — **complete oracle corpus**: 10 .desc grammars
  (combined.desc = cat of udon's udon.desc+values.desc, + 9 descent
  examples), each with `.rs.expected` and `.trace.rs.expected` generated
  from Ruby descent (all 20 OK). Regenerate with:
  ```
  cd ~/src/descent && ruby -I lib -e 'require "descent";
    File.write(ARGV[1], Descent.generate(ARGV[0], target: :rust,
    trace: ARGV[2]=="true").gsub(/\n{3,}/, "\n\n"))' IN.desc OUT.rs true|false
  ```
- (session 4) shipped udon parser.rs, current Ruby descent (3f81c3c), and
  descent-rs now all agree byte-exactly; the earlier one-word drift note is
  obsolete.

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
- (session 3) Ruby's call-arg type transform only walks *states*: call args
  in function-level eof handlers and entry actions stay RAW in the context
  (templates happen to re-render them via rust_expr). Mirrored in
  emit::rust for context parity; unify once byte-identity is retired.
- (session 3) `collect_prepend_values` parses the WHOLE call_args string as
  ONE byte literal, so only single-arg calls ever contribute — on this
  corpus every prepend_values array is empty ([] for text/sameline_text).
  Mirrored; either fix the tracer or drop the feature (templates don't use
  prepend_values — see below).
- (session 3) prepend_values are template-unused AND pre-Rust-escaped in
  Ruby; our IR stores neutral bytes, emit::rust re-escapes only for the
  context diff. Candidate for deletion from the context entirely.
- udon-core upstream defects found by the reader spike (flag to umbrella):
  Text span/content off-by-one on pipe-led runs; Text spans at line start in
  continuation mode; ElementEnd spans past the next line's pipe;
  "Inconsistent indentation" → text-mode degradation drops structural pipes;
  bracket-ids not scoped (space-fragmented, quoted-whitespace garbled).

## desc-format proposals (lexer-conformance artifacts → UDON-friendly .desc)

Joseph confirmed the frame: bridge-friction points are exactly where .desc
syntax conformed to what was easy for the Ruby lexer. **Evaluative criterion
(2026-07-11, `~/src/udon/design/desc-design-principles.md`):** .desc began as
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

Session-3 spike results (full write-ups in
`rust/spikes/normalizations/NOTES.md`):

- **#7 quote-aliases: ORACLE-BLESSED.**
  `combined-quote-aliases.desc` (83 lines): quoted DSL chars → aliases in
  the four CharacterClass contexts (`c[...]`, class members, `->[...]`,
  PREPEND/call args). Byte-identical Ruby output (plain AND trace); reader
  sentinel bridge 999→825 (−17%); reader token-identity preserved; comment
  disagreements 26→16 as a side effect. Hazard contexts (conditions —
  quotes are load-bearing for byte-param inference; keywords fallback args
  — raw-interpolated) untouched and empty of such sites in combined.desc.
  **Valve proposal** (Ruby-side, not oracle-normalizable): add `SC` (`;`),
  maybe `EX`/`BT`/`TAB`, to CharacterClass SINGLE_CHAR — removes most of
  the remaining ~55 quoted-special sites. Umbrella-side landing of the
  grammar edit: flagged, not done from this repo.
- **#2 comment rule: MEASURED, nothing to normalize.** `comment_audit`
  probe (Ruby strip_comments rule vs udon-core comment events, raw bytes):
  452/471 exact matches on combined, 100% on 6 fixtures; ALL disagreements
  are quoted-`;` tails (class 4 → #1/`<SC>` valve) or indent-degradation
  regions (class 1, incl. full-line comments between substate rows). The
  three Ruby scanner quirks are never load-bearing on the corpus —
  "adopt UDON's single comment rule" is corpus-compatible today, gated
  only on class-1 handling. Classification: lexer artifact, removable.

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

**Vendor the stage-0 parser.rs snapshot** in descent-core with source SHA +
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

## Exact next steps (session 5)

Sessions 1-4 delivered the whole Phase-1 pipeline with the acceptance trio
green. What remains is judgment + cleanup + Phase-2, roughly in order:

1. **Decide byte-identity retirement with Joseph/coordinator.** The
   instrument has served its purpose (everything is proven equal); keeping
   it live blocks executing the improvements ledger (every ledger item
   changes output bytes). Proposal: retire diff_generate byte-identity to
   an on-demand check, promote udon's fixture suite + diff_frontend to the
   standing contract, then execute ledger items (helper-usage blind spot,
   silent arg-drops → errors, doc-comment post-process mangling,
   prepend_values deletion, `a:b:c` id error, unreachable_patterns /
   determinism check). Each divergence gets a ledger entry + fixture
   regeneration.
2. **Normalization track:** re-bless #0/#7 against the 3f81c3c base (cheap
   rerun), then #4 (bracket-id quoting, nearest), and coordinate the `<SC>`
   valve (Ruby-side SINGLE_CHAR addition) for #7's second pass.
   Umbrella-side grammar edits stay flagged, not landed from here.
3. **Housekeeping candidates:** spike crate is now a shim + probes (keep —
   NOTES.md and comment_audit are the normalization evidence base);
   `dump_context.rb`/diff harnesses unchanged; consider `descent-rs
   generate -o FILE` and wiring udon's regenerate-parser to descent-rs as
   the umbrella-side switch (flag to coordinator, needs their call).
4. Keep commits local on `rust-rewrite`; no push (Joseph reviews).

## Oracle status per corpus (contract instrument)

All current as of session 4, on the reconciled base (descent 3f81c3c merged,
stage-0 = udon d0bc9f9, fixtures regenerated from updated Ruby):

- Front-end differential (Rust vs Ruby): **30/30 OK**
  (10 grammars × {tokens, ast, context}; `rust/tools/diff_frontend.sh`) —
  now runs THROUGH the promoted reader front-end, so it checks
  reader==Ruby end-to-end; harness non-vacuous (caught real divergences
  during template bring-up).
- udon-core reader vs oracle lexer: **10/10 token-identical**
  (`rust/tools/diff_reader.sh`, oracle side via `descent-rs tokens
  --oracle`; 6,937 tokens) — re-verified after the stage-0 bump (udon's
  event-stream changes did not break token identity).
- Generated output vs Ruby fixtures: **20/20 byte-identical**
  (10 grammars × {plain, trace}; `rust/tools/diff_generate.sh`).
- **Acceptance trio (all green):** udon parser.rs regenerated
  byte-identically from `~/src/udon/core/generator/*.desc`; udon fixture
  suite 83/83 (`cargo test --workspace` in ~/src/udon/core);
  regenerate→rebuild→regenerate fixed point stable (stage-0==1==2,
  plain+trace).
- Normalization checks run: placeholder-cells #0 (byte-identical ✓),
  quote-aliases #7 (byte-identical plain+trace ✓), comment-rule #2
  (audit only — no grammar change needed). NOTE: these were blessed
  against pre-3f81c3c Ruby; re-bless #0/#7 against the new base before
  landing umbrella-side (cheap: rerun the oracle check).
