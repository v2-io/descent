#!/usr/bin/env bash
# Differential test: generated parser source, descent-rs vs the Ruby-descent
# oracle fixtures (.rs.expected / .trace.rs.expected), byte-exact.
#
# Byte-identity is a development instrument, not the contract (PROGRESS.md);
# deliberate divergences get logged in the improvements ledger and this
# harness gains a per-fixture allowlist if/when one lands.
#
# Usage: bash rust/tools/diff_generate.sh   (from the descent repo root)
set -u
cd "$(dirname "$0")/../.." || exit 1

cargo build --quiet --manifest-path rust/Cargo.toml || exit 1
RS=rust/target/debug/descent-rs

fail=0
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
for desc in rust/tests/fixtures/*.desc; do
  base=$(basename "$desc" .desc)
  for mode in plain trace; do
    if [ "$mode" = trace ]; then
      expected="rust/tests/fixtures/$base.trace.rs.expected"
      "$RS" generate "$desc" --trace >"$tmp" 2>/dev/null || { echo "FAIL $base $mode (descent-rs error)"; "$RS" generate "$desc" --trace 2>&1 >/dev/null | head -3; fail=1; continue; }
    else
      expected="rust/tests/fixtures/$base.rs.expected"
      "$RS" generate "$desc" >"$tmp" 2>/dev/null || { echo "FAIL $base $mode (descent-rs error)"; "$RS" generate "$desc" 2>&1 >/dev/null | head -3; fail=1; continue; }
    fi
    if d=$(diff "$expected" "$tmp"); then
      echo "OK   $base $mode"
    else
      echo "FAIL $base $mode"
      echo "$d" | head -20
      fail=1
    fi
  done
done
exit $fail
