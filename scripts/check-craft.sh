#!/usr/bin/env bash
# Mechanical craft gates that clippy cannot express.
set -euo pipefail
cd "$(dirname "$0")/.."
fail=0

# 1. No god-files: production .rs files stay under 800 lines.
while read -r n f; do
  [[ "$f" == "total" ]] && continue
  if (( n > 800 )); then echo "file too long ($n > 800): $f"; fail=1; fi
done < <(find crates -name '*.rs' -not -path '*/tests/*' -not -name '*_tests.rs' -print0 | xargs -0 wc -l)

# 2. Every file with copied code names its origin.
for f in $(grep -rl 'Adapted from' crates --include='*.rs' || true); do
  grep -q 'Adapted from .*(\(GPL\|MIT\|Apache\)' "$f" || { echo "bad attribution header: $f"; fail=1; }
done

# 3. Every crate opts into the workspace lints.
for c in crates/*/Cargo.toml; do
  grep -q '^\[lints\]' "$c" && grep -A1 '^\[lints\]' "$c" | grep -q 'workspace = true' \
    || { echo "missing [lints] workspace = true: $c"; fail=1; }
done

exit $fail
