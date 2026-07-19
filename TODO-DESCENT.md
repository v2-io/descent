# TODO-DESCENT — the descent parser generator

Open items only — closed work lives in git and [CHANGELOG.md](CHANGELOG.md).
(The name follows the UDON umbrella's `TODO-<AREA>.md` lane convention;
descent is its own repo but is treated logistically as part of UDON, its only
consumer today.) descent-rs (`rust/`) is the live implementation; consumer
correctness is proven by the consumer's fixture gates, and any change to
generated code carries the benchmarking discipline in the README.

## Open

- [ ] **Generated EOF: infer positional vs delimited from exit structure**
  (from UDON EOF framing, 2026-07-17 — design of record
  `../../spec/TODO-EOF-refactor.md`, especially **Addendum A**). Prefer
  **no author boolean**: classify function-exit edges (soft success /
  hard success / soft failure); delimited ≈ EXPECTS unpaid on soft end;
  positional ≈ soft success accepting. Extends this repo’s *Inferred
  EXPECTS* sketch (`implementation-spec.md`). Generate soft-success
  default end vs soft-failure unexpected-EOF + **entry site**; closer
  language stays in the grammar. Static reject soft+hard success mix
  without rare override. Deletes the bulk of hand `|eof` arms in
  consumers. Supersedes the aggregate-event sketch in udon's
  `design/eof-model-proposal-2026-07.md`.
  **LARGELY LANDED (2026-07-18, recursive backend):** `classify` module +
  `delimited_code` force-unwind (delimited) + `eof_run_newline`/`eof_run_default`
  (positional EOF ≡ newline). ~34 hand arms deleted in UDON, gate 2→1, benchmark
  flat. *Remaining:* (a) constructs whose closer is a **line-shape / callee-matched**
  and thus invisible to the structural classifier (freeform's ` ``` ` fence; the
  inline comment/raw/directive callee-scanners) still carry a one-line hand `|eof`
  as an explicit delimited *declaration* — the clean form is the `|unclosed`
  directive (next item); (b) the **static reject** of a soft+hard-success mix is
  not implemented; (c) content-keeping at EOF for **accumulating BRACKETs**
  (`sameline_raw` still drops its raw body, `sameline_dir_body` mislabels — UDON
  `TODO-CORE-PARSING`). *(Backend parity is DONE: the pushdown backend now
  generates the same EOF handling as the recursive one — the `eof_run` predicates
  live in shared `ir.rs` — verified by `pushdown_differential`.)*

- [ ] **Derive `Unclosed<Name>` from the construct, not hand-injected** (from
  UDON EOF work, 2026-07-18). A delimited function/type already names its
  construct, so its unclosed-anomaly code should fall out of that name
  (`Embedded` → `UnclosedEmbedded`) instead of being written per construct in
  both grammar and spec. Rides the positional/delimited inference above
  (soft-failure generation + entry site already know the frame's identity).
  One fewer scattered place to keep in sync in every consumer.
  **PARTIAL (2026-07-18):** `Unclosed<ReturnType>` derivation landed and is
  correct for the regular constructs (StringValue/Array/Embedded/Interpolation);
  `UnterminatedFreeform` was normalized to `UnclosedFreeform`. NOT yet solved:
  constructs whose construct-name ≠ return type (`embed_content:Text` →
  `Embedded`; the inline forms `Directive`-typed but meaning InlineRaw/
  InlineDirective). The clean primitive is a declarative **`|unclosed <Name>`**
  function directive (delimited declaration + code in one line) that replaces
  the hand `|eof` arms on those constructs and drives the force-unwind — the
  next concrete piece.

- [ ] **Emit a parser manifest — what the generator detected** (Joseph,
  2026-07-18). Alongside the parser, output the parser's own inventory: the
  warning/error codes it can issue, each function's positional/delimited
  classification, entry sites — a machine-readable self-description that
  doubles as a "pre-fixture" smoke test. Immediate UDON payoff: diff the
  emitted warning-code list against the spec's declared vocabulary (Warning
  codes / End of input) so the two can't silently drift — parser as source,
  spec checked against it, instead of both hand-maintained. Feeds the
  spec-DRY direction in `../../spec/TODO-SPEC-CORE.md`.
  **PARTIAL (2026-07-18):** `descent-rs classify <file>` emits the
  positional/delimited classification (report-only). Still to add: the emitted
  **warning-code list** and its diff against the spec's declared vocabulary
  (the spec-drift guard).

- [ ] **State templates / a "self-terminating value" state property** (from
  UDON grammar refactor, 2026-07-16) — UDON's `typed_value` has ~15 number
  states that each repeat the same four terminator rows
  (`eof`/`'\n'`/`' '`/`:bracket` → emit + return). A declarable per-state
  terminator template (or a lightweight state-macro facility) would DRY
  them without losing per-base digit validation. Merging digit classes is
  NOT an alternative (it changes behavior: `0o9` must fall through to
  BareValue).
  **NOTE (2026-07-18):** the `eof` terminator row is now gone from all ~15 num
  states — EOF is auto-handled by `eof_run_newline` (EOF ≡ the state's `\n`
  arm). So a state-template need only DRY the remaining `\n`/`' '`/`:bracket`
  terminator rows (per-base digit rows + the typed emit still differ). This is
  the biggest remaining visible cruft in UDON's grammar.

  **Design options (2026-07-16 pass — none forced; needs a design call):**
  The states differ in TWO dimensions: the digit/continue rows AND the
  emitted event type (`Integer`/`Float`/`Rational`/`Complex`/`BareValue`),
  so any template needs an event-type slot, not just shared rows.

  1. *Row-splice templates with typed args.* A top-level block
     `|template[val_end] :T` holding case rows
     (`|eof | T(USE_MARK) |return` / `|c['\n']` / `|c[' ']` /
     `|c[:bracket]` …), spliced into a state at the position of a
     `|use[val_end(Integer)]` row. AST-level expansion (after parse,
     before IR) with `T` substituted as an event-type token — keeps ".desc
     is valid UDON" (no lexer macros), preserves row ordering control
     (splice point sits above the state's own `|default`), and adds no
     runtime semantics. Most general; ~1 new AST node + an expansion pass.
     Leading candidate.
  2. *State property.* `|state[:num_hex] SELF_TERM(Integer)` — the
     generator injects the standard terminator rows. Tersest, but the
     terminator SET (`\n`/space/`:bracket`) would be hardwired into
     descent, which is UDON policy, not generator mechanics — wrong layer
     unless combined with a declarable set (option 3).
  3. *Named terminator sets.* `|terminators[val_end] eof '\n' ' ' :bracket`
     + per-state `|ends[val_end -> Integer]`. Cleanly splits the WHAT
     (set, declared once) from the HOW (emit+return, generator-known).
     Middle ground; two new declarations.
  4. *Grammar-side helper function.* Rejected: the terminator rows must
     return from `typed_value` with the byte unconsumed AND emit a
     per-state type; a callee can do the emit but the caller still needs
     per-state routing on the report — no net savings.

  Whichever lands, the |const substitution pass and TypeName(:param) mean
  templates only need event-type + byte args, not general expressions.

- [ ] **Validator: reject grammar locals that collide with generated frame
  field names** — `st` is reserved by the pushdown backend today (each
  frame's state field); a grammar local named `st` would generate broken
  code with no diagnostic. (descent-rs currently has no validator pass at
  all — the Ruby validator was never ported — so this may seed one.)

- [ ] **`|state[:name] MARK` — entry actions on the state line** (Dec 2025
  values.desc feedback #14; verified still true of descent-rs 2026-07-16:
  `ast::State` has no entry-actions field, and trailing tokens on a state
  line are ignored). Workaround: put `| MARK` in each entry row. A
  state-property mechanism (state-templates option 2 above) would subsume
  this — consider them together.

## Sunset the Ruby plumbing — descent as a full Rust crate (Joseph, 2026-07-16)

descent-rs is the live implementation; the Ruby gem is legacy lineage. Move
to a proper standalone Rust crate and retire the Ruby workflow:

- [ ] Restructure so `rust/` becomes the repo root shape (or the crate is
      published from it): `descent-core` (lib) + `descent-cli` (bin).
- [ ] crates.io naming: `descent` is squatted by an unrelated 2021 crate
      (verified 2026-07-09) — pick and reserve a name (`descent-rs`?
      `descent-parser`?) early.
- [ ] Retire the Ruby-parity instrumentation deliberately: the byte-identity
      differential (`descent-rs context` vs `rust/tools/dump_context.rb`)
      and the faithfully-reproduced Ruby quirks (see emit/rust/mod.rs docs)
      served the migration; once Ruby is sunset they are dead weight and the
      quirks can be cleaned up as *improvements* (byte-identical output is
      never a goal — the consumer's fixture gates + benchmarks are the
      criteria).
- [ ] Archive `lib/`, `exe/`, the gemspec, and the `dx` Ruby workflow;
      migrate anything still referenced (characters.md etc. are shared docs
      and stay).
- [ ] Decide whether the Ruby side keeps a running test suite at all in the
      meantime: `rake test` fails to load under Ruby 4.0 (dies in the
      `generator_test.rb` require, at pre-2026-07-15 HEAD too — environment
      breakage, not a regression).
- [ ] CI + publish pipeline for the crate; consumers (udon's
      regenerate-parser) already build from source via the submodule and
      keep working throughout.

## Pin the UDON version descent reads (Joseph, 2026-07-16)

`.desc` files are (nominally) UDON documents — now named `*.descent.udon` in
the consumer — but descent currently reads them with a bespoke
pipe-delimited lexer, not a conformant UDON parser. Track the contract
carefully, in both eras:

- [ ] **Now (bespoke lexer):** declare, in one operable place (a
      `DESC-UDON-VERSION` file or a constant surfaced by `descent-rs
      --version`), which UDON core version the `.desc` dialect is *written
      against* — i.e. the version whose syntax the lexer's subset is meant
      to be a subset of. Consumers' grammar files should stay valid UDON at
      that pinned version (a CI check parsing them with the pinned
      `udon-core` — the vendored copy at `rust/vendor/udon-core` is the
      seed — would catch drift long before bootstrapping).
- [ ] **Bootstrapping era:** when descent starts parsing its input via
      `udon-core` proper, the dependency must be a *stable, tagged*
      compliance version (`core-vX.Y.Z`), declared the same way UDON's
      consumers declare `core ^X.Y` — never a floating spec. Circularity
      note: the udon-core that parses the grammar and the udon-core the
      grammar generates must be allowed to differ by exactly one
      stable-version step, or a broken grammar could regenerate the parser
      that misreads the grammar (same bootstrap trap as udon-in-udon
      fixtures — see the umbrella's TODO-META).

## Ideas (unscheduled — Ruby-era wishlist, kept deliberately)

- [ ] **Parser profiling mode** — instrument a generated parser, run it over
      exemplar corpora, and report actual branching behavior (transition
      counts, scan hit rates, dead branches) to inform case ordering, SCAN
      targets, and state merging.
- [ ] **Static analysis on the IR** — infinite-loop detection (states where
      every case self-loops), unreachable states, type-consistency lints
      (e.g. CONTENT functions that never MARK).
- [ ] **C target** — `.c` + `.h` via a C template, planned since the
      original design; no current consumer, unscheduled.
