# Mutant ID identity receipt

## Gate written before identity fix

`scripts/gate-id-stability.sh` was created before changing the Rust ID logic. It creates a temporary workspace, runs a four-function `||` fixture, overwrites that same relative source path with an earlier fifth function, checks distinct IDs in both runs, checks every original ID remains present, checks each existing named function retained its ID, and requires exactly one fresh ID.

## Mandatory failing run before fix

Verbatim output:

```text
CHECK: building debug binary
   Compiling bughunter-cli v0.1.0 (/Users/jcoeyman/cloudflare/bughunter/crates/cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.44s
CHECK: same-operator ids survive an earlier insertion
ERROR: id changed after earlier insertion; existing function ids no longer match
EXIT_STATUS=1
```

The failure is specifically the ID-change assertion.

## Implementation

`stable_mutant_id` now includes a normalized token window bounded to the enclosing function declaration plus its function name. The context does not include source line numbers or byte offsets. A contextual ordinal is retained only if multiple same-operator mutants have identical full context; otherwise its hash value is zero.

## Mandatory passing run after fix

Verbatim output:

```text
CHECK: building debug binary
   Compiling bughunter-cli v0.1.0 (/Users/jcoeyman/cloudflare/bughunter/crates/cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.38s
CHECK: same-operator ids survive an earlier insertion
PASS: same-operator ids survive an earlier insertion with one fresh id
EXIT_STATUS=0
```

## Formatting

Command:

```text
cargo fmt && cargo fmt --check
```

Verbatim output:

```text
EXIT_STATUS=0
```

## Clippy

Command:

```text
cargo clippy --workspace --all-targets -- -D warnings
```

Verbatim output:

```text
    Checking bughunter-cli v0.1.0 (/Users/jcoeyman/cloudflare/bughunter/crates/cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.72s
EXIT_STATUS: 0
```

## Workspace tests

Command:

```text
cargo test --workspace
```

Verbatim relevant output, including the required line:

```text
test tests::same_operator_mutants_have_distinct_json_ids ... ok
test tests::stable_mutant_id_survives_a_line_shift ... ok

test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.02s

Doc-tests bughunter_engine

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Doc-tests bughunter_runner

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

EXIT_STATUS=0
```

## Existing gates

Commands and verbatim outputs:

```text
COMMAND: ./scripts/gate.sh
CHECK: building debug binary
   Compiling bughunter-cli v0.1.0 (/Users/jcoeyman/cloudflare/bughunter/crates/cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.56s
CHECK: access-check result schema, counts, score, survivors, timeouts, and errors
CHECK: workspace symlink resolves to the materialized mutant
CHECK: --version prints a semantic version
CHECK: --fail-on-survivors fails on the access-check survivors
CHECK: --fail-on-survivors exited 2 as expected
PASS: all gate checks succeeded
EXIT_STATUS: 0

COMMAND: ./scripts/gate-sarif.sh
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
PASS
EXIT_STATUS: 0

COMMAND: ./scripts/gate-unique-ids.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s
CHECK: access-check emits one unique id per mutant
PASS: access-check emits one unique id per mutant
EXIT_STATUS: 0

COMMAND: ./scripts/gate-crawl.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s
PASS
EXIT_STATUS: 0

COMMAND: ./scripts/gate-config.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
CHECK: config supplies operators
CHECK: CLI operators override config operators
PASS
EXIT_STATUS: 0

COMMAND: ./scripts/gate-id-stability.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
CHECK: same-operator ids survive an earlier insertion
PASS: same-operator ids survive an earlier insertion with one fresh id
EXIT_STATUS: 0
SUITE_EXIT_STATUS=0
```

The crawl and config gates print expected failing mutant test cases while evaluating mutants; both commands returned zero and printed `PASS`.

## Authorship

Command:

```text
git log --format='%an <%ae>|%cn <%ce>' | sort -u
```

Verbatim output:

```text
acoyfellow <coeyman@gmail.com>|acoyfellow <coeyman@gmail.com>
```

## Constraints verified

- The implementation has no newly added Rust source comments.
- `stable_mutant_id_survives_a_line_shift` and `same_operator_mutants_have_distinct_json_ids` passed.
- No line number or byte offset is hashed as an identity field.
- No Cloudbox path was written.
