#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

printf '%s\n' 'CHECK: building debug binary'
cargo build

binary=./target/debug/bughunter
workspace="$(mktemp -d "${TMPDIR:-/tmp}/bughunter-config.XXXXXX")"
cleanup() {
  rm -rf "$workspace"
}
trap cleanup EXIT

mkdir -p "$workspace/src"
cat > "$workspace/bughunter.toml" <<'EOF'
test = "node --experimental-strip-types --test src/policy.test.ts"
operators = ["logical-and-to-or", "return-true-to-false"]
timeout = 5
concurrency = 1
EOF
cat > "$workspace/src/policy.ts" <<'EOF'
export function policy(age: number, approved: boolean): boolean {
  if (age > 18 && approved) {
    return true;
  }
  return false;
}
EOF
cat > "$workspace/src/policy.test.ts" <<'EOF'
import assert from "node:assert/strict";
import { test } from "node:test";
import { policy } from "./policy.ts";

test("policy accepts approved adults", () => {
  assert.equal(policy(19, true), true);
});
EOF

printf '%s\n' 'CHECK: config supplies operators'
config_json="$("$binary" run \
  --repo "$workspace" \
  --file src/policy.ts \
  --json)"
config_total="$(printf '%s\n' "$config_json" | python3 -c 'import json, sys; print(json.load(sys.stdin)["total"])')"

printf '%s\n' 'CHECK: CLI operators override config operators'
cli_json="$("$binary" run \
  --repo "$workspace" \
  --file src/policy.ts \
  --operators cond-boundary-gt \
  --json)"
cli_total="$(printf '%s\n' "$cli_json" | python3 -c 'import json, sys; print(json.load(sys.stdin)["total"])')"

if [[ "$config_total" != 2 ]]; then
  printf '%s\n' "ERROR: expected config operators to produce 2 mutants, got $config_total" >&2
  exit 1
fi
if [[ "$cli_total" != 1 ]]; then
  printf '%s\n' "ERROR: expected CLI operators to produce 1 mutant, got $cli_total" >&2
  exit 1
fi
if [[ "$config_total" == "$cli_total" ]]; then
  printf '%s\n' "ERROR: config and CLI runs produced the same count ($config_total)" >&2
  exit 1
fi

printf '%s\n' 'PASS'
