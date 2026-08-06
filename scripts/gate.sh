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

printf '%s\n' 'CHECK: access-check result schema, counts, score, survivors, timeouts, and errors'
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
expected_survivors = [
    (10, "return-true-to-false"),
    (15, "return-false-to-true"),
    (28, "cond-boundary-lt"),
]
if payload.get("schema_version") != 2:
    raise SystemExit("expected schema_version 2, got {}".format(payload.get("schema_version")))
total = payload["total"]
if total != 16:
    raise SystemExit("expected total 16, got {}".format(total))
for mutant in payload["mutants"]:
    if not isinstance(mutant.get("id"), str) or not mutant["id"]:
        raise SystemExit(f"expected every mutant to have a non-empty id, got {mutant}")
counts = {}
for status in ("killed", "survived", "timeout", "error"):
    count = payload.get(status)
    if type(count) is not int:
        raise SystemExit(f"expected integer {status} count, got {count!r}")
    observed = sum(mutant["status"] == status for mutant in payload["mutants"])
    if count != observed:
        raise SystemExit(f"expected {status} count {observed}, got {count}")
    counts[status] = count
if sum(counts.values()) != total:
    raise SystemExit(f"expected status counts to sum to total, got {counts} and {total}")
evaluated = payload.get("evaluated")
if type(evaluated) is not int:
    raise SystemExit(f"expected integer evaluated count, got {evaluated!r}")
expected_evaluated = counts["killed"] + counts["survived"]
if evaluated != expected_evaluated:
    raise SystemExit(f"expected evaluated {expected_evaluated}, got {evaluated}")
if "score" not in payload:
    raise SystemExit("expected score to be present")
score = payload["score"]
expected_score = None if evaluated == 0 else counts["killed"] / evaluated
if expected_score is None:
    if score is not None:
        raise SystemExit(f"expected null score, got {score!r}")
elif type(score) not in (int, float) or score != expected_score:
    raise SystemExit(f"expected score {expected_score}, got {score!r}")
survivors = [
    (mutant["line"], mutant["operator"])
    for mutant in payload["mutants"]
    if mutant["status"] == "survived"
]
if survivors != expected_survivors:
    raise SystemExit(f"expected survivors {expected_survivors}, got {survivors}")
for status in ("timeout", "error"):
    if counts[status] != 0:
        raise SystemExit(f"expected zero {status} results, got {counts[status]}")
'

printf '%s\n' 'CHECK: workspace symlink resolves to the materialized mutant'
workspace="$(mktemp -d "${TMPDIR:-/tmp}/bughunter-gate.XXXXXX")"
cleanup() {
  rm -rf "$workspace"
}
trap cleanup EXIT
mkdir -p "$workspace/packages/lib/src" "$workspace/apps/web/src" "$workspace/apps/web/node_modules/@pkg"
printf '%s\n' '{"name":"root","private":true}' > "$workspace/package.json"
cat > "$workspace/packages/lib/src/guard.ts" <<'EOF'
export function withinQuota(used: number, limit: number): boolean {
  return used < limit;
}
EOF
cat > "$workspace/apps/web/src/app.test.ts" <<'EOF'
import { test } from "node:test";
import assert from "node:assert/strict";
import { withinQuota } from "@pkg/lib/src/guard.ts";

test("withinQuota rejects the limit", () => {
  assert.equal(withinQuota(10, 10), false);
});
EOF
ln -s ../../../../packages/lib "$workspace/apps/web/node_modules/@pkg/lib"
workspace_json="$("$binary" run \
  --repo "$workspace" \
  --file packages/lib/src/guard.ts \
  --operators cond-boundary-lt \
  --test 'cd apps/web && node --experimental-strip-types --test src/app.test.ts' \
  --skip-baseline \
  --json 2>/dev/null)"
printf '%s\n' "$workspace_json" | python3 -c '
import json
import sys
payload = json.load(sys.stdin)
results = payload["mutants"]
if len(results) != 1 or results[0]["status"] != "killed":
    raise SystemExit(
        f"P0 workspace-escape regression: expected the workspace mutant to be killed, got {results}"
    )
'

printf '%s\n' 'CHECK: --version prints a semantic version'
version_output="$("$binary" --version)"
printf '%s\n' "$version_output" | python3 -c '
import re
import sys
version = sys.stdin.read().strip()
if not re.fullmatch(r"bughunter .*\d+\.\d+.*", version):
    raise SystemExit(f"expected a bughunter version containing digit.digit, got {version!r}")
'

printf '%s\n' 'CHECK: --fail-on-survivors fails on the access-check survivors'
set +e
"$binary" run \
  --repo examples/access-check \
  --file src/access.ts \
  --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true \
  --test 'node --experimental-strip-types --test src/access.test.ts' \
  --json \
  --fail-on-survivors \
  > /dev/null 2>&1
survivor_exit=$?
set -e
if [[ "$survivor_exit" -eq 0 ]]; then
  printf '%s\n' 'ERROR: --fail-on-survivors succeeded despite surviving mutants' >&2
  exit 1
fi
printf '%s\n' "CHECK: --fail-on-survivors exited $survivor_exit as expected"
printf '%s\n' 'PASS: all gate checks succeeded'
