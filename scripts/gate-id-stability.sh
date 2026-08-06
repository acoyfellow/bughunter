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

workspace="$(mktemp -d "${TMPDIR:-/tmp}/bughunter-id-stability.XXXXXX")"
cleanup() {
  rm -rf "$workspace"
}
trap cleanup EXIT

cat > "$workspace/source.ts" <<'EOF'
export function alpha(left: boolean, right: boolean): boolean {
  return left || right;
}

export function bravo(left: boolean, right: boolean): boolean {
  return left || right;
}

export function charlie(left: boolean, right: boolean): boolean {
  return left || right;
}

export function delta(left: boolean, right: boolean): boolean {
  return left || right;
}
EOF

"$binary" run --repo "$workspace" --file source.ts --operators logical-or-to-and --test true --skip-baseline --json > "$workspace/original.json"

cat > "$workspace/source.ts" <<'EOF'
export function inserted(left: boolean, right: boolean): boolean {
  return left || right;
}

export function alpha(left: boolean, right: boolean): boolean {
  return left || right;
}

export function bravo(left: boolean, right: boolean): boolean {
  return left || right;
}

export function charlie(left: boolean, right: boolean): boolean {
  return left || right;
}

export function delta(left: boolean, right: boolean): boolean {
  return left || right;
}
EOF

printf '%s\n' 'CHECK: same-operator ids survive an earlier insertion'
"$binary" run --repo "$workspace" --file source.ts --operators logical-or-to-and --test true --skip-baseline --json > "$workspace/inserted.json"

python3 - "$workspace/original.json" "$workspace/inserted.json" <<'PY'
import json
import sys

original = [mutant["id"] for mutant in json.load(open(sys.argv[1], encoding="utf-8"))["mutants"]]
inserted = [mutant["id"] for mutant in json.load(open(sys.argv[2], encoding="utf-8"))["mutants"]]

if len(original) != len(set(original)):
    raise SystemExit("ERROR: original run contains duplicate ids")
if len(inserted) != len(set(inserted)):
    raise SystemExit("ERROR: inserted run contains duplicate ids")
if len(original) != 4 or len(inserted) != 5:
    raise SystemExit(f"ERROR: expected four original ids and five inserted ids, got {len(original)} and {len(inserted)}")
if original != inserted[1:]:
    raise SystemExit("ERROR: id changed after earlier insertion; existing function ids no longer match")
missing = sorted(set(original) - set(inserted))
if missing:
    raise SystemExit(f"ERROR: id changed after earlier insertion; missing original ids: {missing}")
fresh = set(inserted) - set(original)
if len(fresh) != 1:
    raise SystemExit(f"ERROR: expected exactly one fresh id for the inserted mutant, got {len(fresh)}")
PY

printf '%s\n' 'PASS: same-operator ids survive an earlier insertion with one fresh id'
