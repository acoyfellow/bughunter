#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

printf '%s\n' 'CHECK: building debug binary'
cargo build

binary=./target/debug/bughunter
if [[ ! -x "$binary" ]]; then
  printf '%s\n' 'ERROR: cargo build completed without target/debug/bughunter' >&2
  exit 1
fi

printf '%s\n' 'CHECK: access-check emits one unique id per mutant'
fixture_json="$("$binary" run \
  --repo examples/access-check \
  --file src/access.ts \
  --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true \
  --test 'node --experimental-strip-types --test src/access.test.ts' \
  --json 2>/dev/null)"
printf '%s\n' "$fixture_json" | python3 -c '
import json
import sys

payload = json.load(sys.stdin)
mutants = payload["mutants"]
total = payload["total"]
if total != 16:
    raise SystemExit(f"expected total 16, got {total}")
if len(mutants) != total:
    raise SystemExit(f"expected {total} mutants, got {len(mutants)}")
ids = [mutant.get("id") for mutant in mutants]
if any(not isinstance(identifier, str) or not identifier for identifier in ids):
    raise SystemExit("expected every mutant to have a non-empty id")
unique_ids = len(set(ids))
if unique_ids != total:
    raise SystemExit(f"expected {total} unique ids, got {unique_ids}")
'

printf '%s\n' 'PASS: access-check emits one unique id per mutant'
