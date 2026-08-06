#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

printf '%s\n' 'CHECK: building debug binary'
cargo build

binary=./target/debug/bughunter
json_path="$(mktemp "${TMPDIR:-/tmp}/bughunter-crawl-json.XXXXXX")"
sarif_path="$(mktemp "${TMPDIR:-/tmp}/bughunter-crawl-sarif.XXXXXX")"
cleanup() {
  rm -f "$json_path" "$sarif_path"
}
trap cleanup EXIT

"$binary" run \
  --repo examples/access-check \
  --file src \
  --operators cond-boundary-lt,return-true-to-false \
  --test 'node --experimental-strip-types --test src/access.test.ts' \
  --json \
  --sarif "$sarif_path" \
  > "$json_path"

python3 - "$json_path" "$sarif_path" <<'PY'
import json
import os
import sys

json_path, sarif_path = sys.argv[1:]
with open(json_path, encoding="utf-8") as handle:
    payload = json.load(handle)
with open(sarif_path, encoding="utf-8") as handle:
    sarif = json.load(handle)

files = payload.get("files")
if not isinstance(files, list) or len(files) < 2:
    raise SystemExit(f"expected at least two file results, got {files!r}")
paths = [entry.get("file") for entry in files]
if any(not isinstance(path, str) or not path for path in paths):
    raise SystemExit(f"expected every file entry to name a file, got {paths!r}")
if len(paths) != len(set(paths)):
    raise SystemExit(f"expected distinct file entries, got {paths!r}")
fields = ("total", "killed", "survived", "timeout", "error", "evaluated")
for field in fields:
    expected = sum(entry.get(field, 0) for entry in files)
    if payload.get(field) != expected:
        raise SystemExit(
            f"expected overall {field} {expected}, got {payload.get(field)!r}"
        )
def visit(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key == "uri" and (not isinstance(child, str) or os.path.isabs(child)):
                raise SystemExit(f"expected relative artifact URI, got {child!r}")
            visit(child)
    elif isinstance(value, list):
        for child in value:
            visit(child)
visit(sarif)
PY

printf '%s\n' 'PASS'
