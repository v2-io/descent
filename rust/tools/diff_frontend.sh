#!/bin/sh
# Differential test: Rust vs Ruby descent on all fixture grammars, at three
# checkpoints: tokens (lexer), ast (parser), context (ir_builder + emitter
# context — the full template input).
# JSON is key-order-normalized through `jq -S` so only content differences show.
#
# Usage: rust/tools/diff_frontend.sh   (from the descent repo root)
set -u
cd "$(dirname "$0")/../.." || exit 1

cargo build --quiet --manifest-path rust/Cargo.toml || exit 1
RS=rust/target/debug/descent-rs

fail=0
for desc in rust/tests/fixtures/*.desc; do
  base=$(basename "$desc" .desc)
  for kind in tokens ast context; do
    rb_out=$(ruby -I lib "rust/tools/dump_${kind}.rb" "$desc" 2>&1) || { echo "FAIL $base $kind (ruby error)"; echo "$rb_out" | head -3; fail=1; continue; }
    rs_out=$("$RS" "$kind" "$desc" 2>&1) || { echo "FAIL $base $kind (rust error)"; echo "$rs_out" | head -3; fail=1; continue; }
    if d=$(diff <(printf '%s' "$rb_out" | jq -S .) <(printf '%s' "$rs_out" | jq -S .)); then
      echo "OK   $base $kind"
    else
      echo "FAIL $base $kind"
      echo "$d" | head -20
      fail=1
    fi
  done
done
exit $fail
