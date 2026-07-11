# Normalization spikes — oracle-checked .desc surface changes

Rule (RATIFIED, PROGRESS.md): a normalization may land iff Ruby descent
produces **byte-identical** parser output from the normalized grammar.
Umbrella-side landing of any grammar edit is flagged, never done from here.

Spike #0 (placeholder cells) is documented in
`rust/spikes/udon-reader/NOTES.md`; evidence file
`combined-no-placeholder-cells.desc` here.

## Spike #7 — quote-alias adoption (session 3, 2026-07-11): ORACLE-BLESSED

Direction: quoted DSL-special chars -> `<X>` aliases, in the four
CharacterClass-parsed contexts only (`c[...]` cells, `<...>` class members,
`->[...]` targets, `PREPEND(...)`/call args). This is the udon-friendly
direction: every quote inside a bracket-id is a bridge site (reader mismatch
class 3), every quoted pipe in a tail is class 2 — aliases contain no
special bytes at all.

Evidence: `combined-quote-aliases.desc` (83 lines changed, built by
scripted exact-context replacement):

- `c['|']`->`c[<P>]`, `c['[']`->`c[<L>]`, `c[']']`->`c[<R>]`,
  `c['{']`->`c[<LB>]`, `c['}']`->`c[<RB>]`
- `->[']']`->`->[<R>]`, `->['\n']`->`->[<NL>]`
- `PREPEND('|')`->`PREPEND(<P>)`; call-arg tails `, '|')`->`, <P>)` etc.
- class members: `SQ '[' `->`SQ L `; `c[<'\n:|' '}'>]`->`c[<'\n:' P RB>]`
  (token order preserved => chars order, scan_chars order preserved)

Results:

1. **Oracle**: Ruby descent output byte-identical, plain AND trace.
2. **Bridge shrink**: udon-reader sentineled bytes 999 -> 825 (-17%);
   reader still token-identical to the oracle on the normalized grammar
   (3,191 tokens).
3. **Comment-rule convergence bonus** (see spike #2 below): ruby-vs-udon
   comment disagreements 26 -> 16 on combined — quotes were also
   misleading udon's comment scanner.

Deliberately untouched (hazard contexts, none of which occur with quoted
specials in combined.desc, but the script must never touch them):
conditions (`if[x == '|']` — the quote is load-bearing for Ruby's
byte-param inference in infer_param_types), keywords `:fallback` args
(interpolated raw into the template).

Remaining quoted specials with NO alias: `';'` (52 sites), `'!'`, `` '`' ``,
`'\t'`, multi-char strings (`'.?!*+'`). Valve proposal (Ruby-side change,
not oracle-normalizable): extend CharacterClass SINGLE_CHAR with `SC` (`;`)
— and possibly `EX`/`BT`/`TAB` — then a second alias pass removes most of
the rest of the bridge.

Alignment nuance: `<LB>`/`<RB>` are one char longer than `'{'`/`'}'`; the
script eats one following space so columns stay put. `'[' `->`L ` shortens
its class cell by 2 (cosmetic column drift on 2 lines — a `udon fmt`
concern, not grammar).

Length-preserving by construction: `<P>`/`<L>`/`<R>` == quoted forms,
`<NL>` == `'\n'`.

Classification: **lexer artifact bridged by principled sugar** — per Joseph
(proposals ledger #7), aliases like `<SQ>` read at least as well in a table
cell as quoted literals, and they are the spelling udon parses cleanly.

## Spike #2 — comment-rule unification (session 3, 2026-07-11): MEASURED, NO GRAMMAR CHANGE NEEDED

Probe: `udon-reader/src/bin/comment_audit.rs` — implements Ruby
strip_comments' rule (per line: `;` at bracket-depth 0 outside single/double
quotes) and compares against udon-core CommentStart/CommentEnd events on the
raw bytes.

Corpus results (ruby sites / match / ruby-only / udon-only):

| fixture | ruby | match | ruby_only | udon_only |
|---|---|---|---|---|
| combined | 471 | 452 | 19 | 7 |
| combined-quote-aliases | 471 | 459 | 12 | 4 |
| udon_complete | 346 | 329 | 17 | 7 |
| markdown | 144 | 140 | 4 | 0 |
| values | 16 | 15 | 1 | 0 |
| other 6 | 71 | 71 | 0 | 0 |

**Every disagreement traces to an already-classified cause, none to the
comment rule itself:**

- **udon_only** (udon eats live bytes): all are quoted-`;` call args
  (`/prose(col, -1, ';')`) — reader mismatch class 4, folds into
  proposal #1 (tail micro-syntax) / the `<SC>` alias valve above.
- **ruby_only** (udon leaks comment bytes as content): all sit inside
  udon-core indent-degradation regions (mismatch class 1 — including
  full-line comments between substate rows, e.g. values.desc L83) where
  udon stops recognizing `;` entirely. That is the degradation defect, not
  a rule difference.

Conclusion: Ruby's three scanner quirks (per-line bracket-depth reset,
dual quote tracking) are **never load-bearing on the corpus** — no grammar
site depends on them, so there is nothing to normalize under the oracle
rule; proposal #2's "adopt UDON's single comment rule" is corpus-compatible
today, gated only on class-1 (row degradation) and the `;`-alias valve.
Classification: **lexer artifact, confirmed removable.**
