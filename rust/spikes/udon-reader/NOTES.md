# udon-reader spike — findings (session 2, 2026-07-11)

**Result: 10/10 fixture grammars token-identical to the oracle lexer**
(6,926 tokens total; `rust/tools/diff_reader.sh`). Token-identity implies
AST-identity (the parser is a deterministic function of tokens). The
udon-core-as-front-end construction is validated at the token layer.

Architecture that got there: reconstruct descent *parts* from udon events,
feed them through the shared `libdescent::lexer::parse_part` (layer 3), so
the reader replaces only layers 1-2. Three bridges were required; each maps
to a proposals-ledger item and is classified per the table-scan criterion
(~/src/udon/notes/desc-design-principles.md).

## Corpus-wide mismatch classes (udon-core events vs .desc semantics)

Each entry: what udon-core does with the .desc construct / the bridge in this
spike / classification.

1. **Dropped structural pipe on continuation lines.** After a line whose tail
   contains sameline child elements (`… | -> |return`), the next line's
   `|name` is emitted as Text with the pipe consumed, no ElementStart, span
   pointing at line start. At dedents this escalates: udon emits
   `Warning("Inconsistent indentation")` and degrades to text mode for whole
   regions (82 sites in combined.desc, 50 in udon_complete, 14 in markdown).
   *Bridge:* orphaned-pipe detection between the last consumed offset and the
   text's true position; degraded-region text re-split with the oracle's
   pipe splitter. *Classification:* .desc's aligned case rows are principled
   (the load-bearing table); udon-core's degradation is a front-end gap —
   either udon grows a mode/schema for row-shaped content or the fused
   dialect specifies rows as first-class. The Warning+text-mode event shape
   is ALSO a udon-core defect worth fixing regardless (spans/structure lost).

2. **No quote protection for `|` in tails.** `/prose(col, -1, '|')`: UDON
   content rules don't treat quotes as protecting pipes; the text run breaks,
   later events garble (a Name containing `)  |>> :line\n` plus
   Error(Unclosed)). *Bridge:* sentinel pre-pass replaces quote-protected
   pipes (oracle's global quote state) before udon sees the bytes.
   *Classification:* lexer artifact on the .desc side (three implicit
   splitters); the fused dialect needs ONE specified rule — either UDON
   string values in call-args or `<P>`-style aliases (proposals #1/#7).

3. **Quotes inside bracket-ids open UDON strings.** `[PREV == ' ']`: the `'`
   opens a string that swallows past `]` to the next `'` — often the NEXT
   line's id — losing the tail between. Also: ids are not bracket-scoped at
   all (`[PREV == 0]` → BareValue "PREV" + Text "== 0]…" in one run), and
   quoted-whitespace ids garble (`c[' \t']` → `']\t'`). *Bridge:* on the id
   attr, re-extract the id from RAW bytes with the oracle's
   `extract_bracketed_id`, skip/clip events inside the region; quotes inside
   brackets sentineled so no string can open. *Classification:* .desc
   bracket-id quoting is bespoke (proposal #4 — adopt UDON attr-id quoting);
   until then this is the single biggest bridge surface. udon-core's id
   fragmentation is worth a look upstream regardless (spaces in `[...]`).

4. **`;` in tails/quotes becomes a UDON comment.** `PREPEND('|')` rows with
   `';'` args: udon comments out the rest of the line. Conversely, in
   degraded text mode udon does NOT recognize `;` comments at all (comment
   content leaked in as Text). *Bridge:* all Ruby-kept `;` sentineled; all
   Ruby-stripped comment bytes BLANKED to spaces, so udon's comment rules are
   never relied on. *Classification:* comment rules already nearly agree
   (proposal #2 stands); the tail-`;` cases fold into the tail-microsyntax
   proposal (#1).

5. **udon-core span irregularities** (upstream defect notes, all bridged by
   trusting `span.start`+content and searching raw bytes):
   - Text content one byte longer than span on pipe-led runs
     (`span 341..349` vs 9 content bytes);
   - Text spans pointing at line start rather than content start in
     continuation mode;
   - ElementEnd spans pointing PAST the next line's `|`.

## Bridge size (spike metrics)

| fixture | sentineled bytes | indent-degradation sites |
|---|---|---|
| combined | 999 | 82 |
| udon_complete | 636 | 50 |
| markdown | 278 | 14 |
| values | 35 | 2 |
| elements | 20 | 1 |
| others | ≤4 each | ≤1 each |

These numbers are the quantified case for the desc-format proposals: every
sentineled byte and every degradation site is a construct where .desc and
UDON semantics disagree today.

## Normalization spike #1: empty placeholder cells (VERIFIED, oracle-blessed)

Joseph's hypothesis confirmed on all three axes
(`rust/spikes/normalizations/combined-no-placeholder-cells.desc`):

1. *Mechanism:* Ruby lexer DISCARDS whitespace-only parts
   (`next if part.strip.empty?` in tokenize) — placeholder cells are pure
   alignment, never semantic.
2. *Oracle:* Ruby descent generates **byte-identical** parser.rs from the
   normalized grammar (all 4 sites in combined.desc:
   `s/^(\s*)\|(\s+)(?=\|)/$1 $2/` — leading pipe of an empty first cell
   becomes a space, columns preserved).
3. *Front-end:* udon-reader tokens on the normalized grammar are identical
   too (continuation line flows through pipe+space → Text → re-split).

Classification: **lexer artifact** (a visual placeholder for a cell the
lexer throws away) — with one nuance: the *second* pipe (before the first
real command) is load-bearing in Ruby (it terminates the previous part), and
row-leading pipes may still win on table-scan legibility. Both spellings are
oracle-equivalent, so this is a `udon fmt`-policy choice, not a grammar
constraint. Landing the edit in
~/src/udon/core/generator/udon.desc is umbrella-side — staged as evidence
here, flagged to the coordinator, NOT landed from this repo.
