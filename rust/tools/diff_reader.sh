#!/usr/bin/env bash
# Reader differential: udon-core front-end tokens (descent-rs default) vs
# oracle lexer tokens (descent-rs --oracle) on all fixture grammars. Per-fixture mismatch counts; details on demand with
# VERBOSE=1. Usage: rust/tools/diff_reader.sh  (from the descent repo root)
set -u
cd "$(dirname "$0")/../.." || exit 1

cargo build --quiet --manifest-path rust/Cargo.toml || exit 1
ORACLE=rust/target/debug/descent-rs
READER=rust/target/debug/udon-reader

fail=0
for desc in rust/tests/fixtures/*.desc; do
  base=$(basename "$desc" .desc)
  o=$("$ORACLE" tokens "$desc" --oracle 2>/dev/null) || { echo "ERR  $base (oracle)"; fail=1; continue; }
  r=$("$READER" "$desc" 2>/dev/null) || { echo "ERR  $base (reader)"; fail=1; continue; }
  if d=$(diff <(printf '%s' "$o" | jq -c '.[]') <(printf '%s' "$r" | jq -c '.[]')); then
    echo "OK   $base ($(printf '%s' "$o" | jq length) tokens)"
  else
    n=$(echo "$d" | grep -c '^[<>]')
    echo "DIFF $base ($n diff lines / $(printf '%s' "$o" | jq length) oracle tokens)"
    [ "${VERBOSE:-0}" = "1" ] && echo "$d" | head -40
    fail=1
  fi
done
exit $fail
