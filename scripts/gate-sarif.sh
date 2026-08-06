#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build

binary=./target/debug/bughunter
json_path="$(mktemp "${TMPDIR:-/tmp}/bughunter-sarif-json.XXXXXX")"
sarif_path="$(mktemp "${TMPDIR:-/tmp}/bughunter-sarif-log.XXXXXX")"
error_path="$(mktemp "${TMPDIR:-/tmp}/bughunter-sarif-error.XXXXXX")"
cleanup() {
  rm -f "$json_path" "$sarif_path" "$error_path"
}
trap cleanup EXIT

operators=cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true
run=(
  "$binary" run
  --repo examples/access-check
  --file src/access.ts
  --operators "$operators"
  --test 'node --experimental-strip-types --test src/access.test.ts'
  --json
)

"${run[@]}" > "$json_path" 2>/dev/null
if ! "${run[@]}" --sarif "$sarif_path" > /dev/null 2> "$error_path"; then
  cat "$error_path" >&2
  exit 1
fi

python3 - "$json_path" "$sarif_path" <<'PY'
import json
import os
import sys

json_path, sarif_path = sys.argv[1:]
with open(json_path, encoding="utf-8") as handle:
    payload = json.load(handle)
with open(sarif_path, encoding="utf-8") as handle:
    sarif = json.load(handle)

if sarif.get("version") != "2.1.0":
    raise SystemExit("expected SARIF version 2.1.0")
if not sarif.get("$schema"):
    raise SystemExit("expected SARIF $schema")
runs = sarif.get("runs")
if not isinstance(runs, list) or len(runs) != 1:
    raise SystemExit("expected exactly one SARIF run")
run = runs[0]
results = run.get("results")
if not isinstance(results, list):
    raise SystemExit("expected a SARIF results array")
if len(results) != payload.get("survived"):
    raise SystemExit(
        f"expected {payload.get('survived')} SARIF results, got {len(results)}"
    )
rules = run.get("tool", {}).get("driver", {}).get("rules", [])
declared_rule_ids = {rule.get("id") for rule in rules}
for result in results:
    if result.get("ruleId") not in declared_rule_ids:
        raise SystemExit(f"undeclared ruleId: {result.get('ruleId')!r}")
fingerprint_key = "bughunterMutantId/v1"
fingerprints = [
    result.get("partialFingerprints", {}).get(fingerprint_key) for result in results
]
if any(not isinstance(fingerprint, str) or not fingerprint for fingerprint in fingerprints):
    raise SystemExit("expected every result to have a mutant fingerprint")
if len(fingerprints) != len(set(fingerprints)):
    raise SystemExit("expected distinct mutant fingerprints")
def visit(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key == "uri":
                if not isinstance(child, str) or os.path.isabs(child) or "Users" in child:
                    raise SystemExit(f"expected a relative artifact URI, got {child!r}")
            visit(child)
    elif isinstance(value, list):
        for child in value:
            visit(child)
visit(sarif)
PY

printf '%s\n' 'PASS'
