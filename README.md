# bughunter

[![CI](https://github.com/acoyfellow/bughunter/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/acoyfellow/bughunter/actions/workflows/ci.yml)

![bughunter](docs/social-card.jpg)

A mutation-testing CLI for TypeScript. It changes one operator in your source, runs your real test
suite against the change, and reports whether any test noticed.

A mutant your tests kill is a line your tests genuinely cover. A mutant that **survives** is a line
your tests execute but do not check. Line coverage cannot tell those apart. This can.

```
$ bughunter run --repo ./my-app --file src/auth.ts \
    --operators logical-and-to-or,equality-strict-to-loose-neg \
    --test 'npx vitest run' --json
{"schema_version":2,"total":12,"killed":8,"survived":4,"timeout":0,"error":0,"evaluated":12,"score":0.6666666666666666,"mutants":[{"id":"a1b2c3d4e5f60708","line":31,"operator":"logical-and-to-or","status":"survived"}, ...]}
```

## Try it in 30 seconds

`examples/access-check` is a self-contained fixture: an access-control module and a 12-test suite
that passes. It has **no dependencies and no `node_modules`**. It uses Node's built-in test runner
and native TypeScript support, so it needs only Node 22.6+ and no install step.

From a clean clone, copy and paste:

```sh
git clone https://github.com/acoyfellow/bughunter.git
cd bughunter
cargo build --quiet
./target/debug/bughunter run \
  --repo examples/access-check --file src/access.ts \
  --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true \
  --test 'node --experimental-strip-types --test src/access.test.ts' --json 2>/dev/null
```

The command above was run from a clean clone. Its stdout is exactly:

```
{"schema_version":2,"total":16,"killed":13,"survived":3,"timeout":0,"error":0,"evaluated":16,"score":0.8125,"mutants":[{"id":"743dbc7b02b2e14b","line":9,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"776154ac39c54f3b","line":9,"operator":"return-true-to-false","status":"killed"},{"id":"b9455bf0af1cb416","line":10,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"e1b6ab9d5b2347d4","line":10,"operator":"return-true-to-false","status":"survived"},{"id":"448fa0b79c347a84","line":11,"operator":"return-false-to-true","status":"killed"},{"id":"6c3e58b7362b04f3","line":15,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"83022e0a18aef21a","line":15,"operator":"return-false-to-true","status":"survived"},{"id":"ce3a7b4b5e021db4","line":16,"operator":"inequality-to-equality","status":"killed"},{"id":"437038af8fb15693","line":16,"operator":"logical-and-to-or","status":"killed"},{"id":"91a9765cd1275d3b","line":16,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"80e2ba434670fb4e","line":20,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"025c8e871465b640","line":20,"operator":"logical-or-to-and","status":"killed"},{"id":"88743788a84eac94","line":20,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"6ba38fa5f8eaa275","line":24,"operator":"inequality-to-equality","status":"killed"},{"id":"a1356b4c2b97fe68","line":28,"operator":"cond-boundary-lt","status":"survived"},{"id":"bbb64bcb85bb4342","line":39,"operator":"logical-and-to-or","status":"killed"}]}
```

16 mutants, 13 killed, 3 survived:

| line | operator | what it means |
|---|---|---|
| 10 | `return-true-to-false` | `isPublicPath("/version")` is never tested. `/health` is. |
| 15 | `return-false-to-true` | the `expected === null` early return is never exercised. |
| 28 | `cond-boundary-lt` | `withinQuota` has an untested boundary. |

That third one is a real off-by-one. Change `used < limit` to `used <= limit` and every test still
passes, but the quota now permits going one over. Verify by hand:

```
cd examples/access-check
sed -i '' 's/used < limit/used <= limit/' src/access.ts
node --experimental-strip-types --test src/access.test.ts   # 12 pass, 0 fail
git checkout src/access.ts
```

12 tests, 100% line coverage of that function, and a boundary bug walks straight through. That gap
is what this tool is for.

The clean clone output above is the checkable evidence for the tool.

## What it found

> **Non-reproducible private-repository report.** The following evidence comes from five unrelated
> private repositories, one real source file each, all eight operators, and each project's own
> vitest suite. You cannot clone or re-run those repositories, so the table is a report rather than
> proof. The two hand-verified survivor diffs below are quoted in full so you can judge the reasoning
> independently.

Two survivors were then reproduced by hand, without the tool, to confirm they are real:

**`backwards/src/engine.ts:248`** — a queue drain loop.

```diff
- while (queue.length > 0) {
+ while (queue.length >= 0) {
```

All 35 tests still pass. The loop's termination condition is untested.

**`summon/src/lib/oauth.ts:31`** — a plain-object guard.

```diff
- return typeof value === "object" && value !== null && !Array.isArray(value);
+ return typeof value === "object" || value !== null && !Array.isArray(value);
```

All 18 tests still pass. The predicate now accepts any non-null value, including strings and
numbers, and nothing notices.

The broader non-reproducible report follows:

| repo | file | mutants | killed | survived | timeout | error | score |
|---|---|---:|---:|---:|---:|---:|---:|
| backwards | `src/engine.ts` | 21 | 16 | 5 | 0 | 0 | 76% |
| diffgate | `src/matcher.ts` | 26 | 18 | 8 | 0 | 0 | 69% |
| summon | `src/lib/oauth.ts` | 29 | 11 | 18 | 0 | 0 | 37% |
| up | `src/capabilities.ts` | 48 | 23 | 25 | 0 | 0 | 47% |
| vitest-visual-diff | `src/style.ts` | 16 | 7 | 9 | 0 | 0 | 43% |

140 mutants, zero timeouts, zero errors. Every suite was green before mutating.

## Install

You need a Rust toolchain and a Unix host. bughunter must control process groups. Windows therefore
fails to compile. It does not degrade silently.

```
git clone <this repo> && cd bughunter
cargo build --release
./target/release/bughunter --help
```

## Usage

```
bughunter run --repo <DIR> --file <RELATIVE.ts|DIR|GLOB> --json [OPTIONS]
```

| flag | meaning |
|---|---|
| `--repo <DIR>` | repository root; the test command runs here |
| `--file <REL>` | source file, directory, or glob to mutate, relative to `--repo` |
| `--config <PATH>` | read configuration from `PATH` instead of `--repo/bughunter.toml` |
| `--operators <IDS>` | comma-separated operator ids; required unless the config supplies them |
| `--test <CMD>` | test command, required unless the config supplies it |
| `--json` | emit JSON on stdout |
| `--sarif <PATH>` | write a SARIF 2.1.0 log to a path |
| `--line-range S-E` | only mutate lines S..E inclusive, 1-based |
| `--timeout-ms N` | per-mutant timeout, default 30000 |
| `--concurrency N` | mutants in flight, default 4 |
| `--skip-baseline` | do not verify the suite passes before mutating |
| `--fail-on-survivors` | gate CI: exit 2 on survivors, exit 3 on unevaluated mutants |
| `--version` | print the installed bughunter version |

`--sarif` can be combined with `--json`.

### Configuration

bughunter reads `bughunter.toml` from `--repo` when it exists. Use `--config <PATH>` to read a
specific file. See [`bughunter.toml.example`](bughunter.toml.example).

```toml
test = "npx vitest run"
operators = ["logical-and-to-or", "equality-strict-to-loose-neg"]
timeout = 30
concurrency = 4
```

`timeout` is in seconds. CLI values always take precedence: `--operators`, `--test`,
`--timeout-ms`, and `--concurrency` override the corresponding config values. Config values override
the built-in timeout and concurrency defaults. A missing discovered config file is ignored; unknown
keys and malformed values are errors.

The JSON payload remains on stdout.

`--file` accepts one source file, a directory crawled recursively, or a glob such as
`src/**/*.ts`. Directory and glob discovery includes `.ts` and `.tsx` files, sorts paths, and
skips test files plus `node_modules`, `dist`, `build`, and `.git`. A direct file preserves the
single-file JSON payload. Multi-file runs add a `files` array with per-file counts and mutants while
the top-level counts remain the overall roll-up. A selector that matches no source files is an error.

The SARIF log contains one result for each surviving mutant.

### Exit codes

| code | meaning |
|---|---|
| `0` | the run completed and no selected gate failed |
| `1` | a usage, parse, or baseline error occurred |
| `2` | `--fail-on-survivors` found surviving mutants; takes precedence over `3` |
| `3` | `--fail-on-survivors` found only timed-out or errored mutants |

Exit `3` protects you from a false pass. A whole run can fail: a bound port, a full disk, or no
memory. No mutant then survives, because no mutant ran. A gate that looks only for survivors reports
success. It tested nothing. Unevaluated mutants therefore fail the gate on their own.

If survivors and unevaluated mutants both appear, bughunter exits `2`. A survivor tells you more.

### Operators

Eight, each a single-token change:

| id | change |
|---|---|
| `cond-boundary-gt` | `>` → `>=` |
| `cond-boundary-lt` | `<` → `<=` |
| `logical-and-to-or` | `&&` → `\|\|` |
| `logical-or-to-and` | `\|\|` → `&&` |
| `equality-strict-to-loose-neg` | `===` → `!==` |
| `inequality-to-equality` | `!==` → `===` |
| `return-true-to-false` | `return true;` → `return false;` |
| `return-false-to-true` | `return false;` → `return true;` |

### Statuses

| status | meaning |
|---|---|
| `killed` | the suite failed, so a test detects this change |
| `survived` | the suite passed, so no test detects it: a test gap |
| `timeout` | the suite hung; its whole process group was killed |
| `error` | the mutant could not be evaluated |

bughunter never counts `timeout` or `error` as `killed`. A hung suite is an unknown, not a success.

## Trust boundary

`--test` is trusted input. bughunter is not a security sandbox. It runs your `--test` command with `sh -c`.
That command can do anything your shell can do.

The baseline check runs before materialization, with its working directory set directly to the real `--repo`.
For each mutant, bughunter canonicalizes and validates the source path is under `--repo`, then recursively
copies the repository without `.git` into a mode-`0700` directory under
`$TMPDIR/bh-work/runner-materializations/<pid>-<time>-<sequence>`, preserving original file permissions,
and writes the mutated source only there. The per-mutant repository copy holds your secrets in cleartext,
including `.env`, `.dev.vars`, credentials, and tokens. Per-mutant `sh -c` commands run in that temporary
copy, which is removed afterward.

bughunter creates each run directory fresh with mode `0700` and refuses to reuse an existing directory.
This behavior matters on a shared host. Another user can create a predictable path first. That user then
receives your secrets in cleartext.

However, `node_modules` regular files and dependency directories are symlinked to their originals, so a
per-mutant test can write through those links to real dependencies. A test command can also name an absolute
path and write anywhere the invoking user can access. Process-group handling limits timeout orphans; it does
not limit filesystem access. Do not point bughunter at a repository whose test command or dependencies you
do not trust.

## How it works

Mutation is AST-based. The file is parsed with [oxc](https://oxc.rs), the visitor matches on
`BinaryOperator` and `LogicalOperator` node kinds, and the replacement rewrites the exact token
span. Consequences:

- An `&&` inside a string literal or a comment is not a mutation site.
- `>=` is not mutated by `cond-boundary-gt`. The inclusive form is a different node kind, so it is
  structurally unreachable rather than filtered out.
- `return x;` is not mutated. Only a boolean literal argument qualifies.
- `return true` inside a nested arrow function is found.
- A file that does not parse is an **error**, not an empty result.

Each mutant is evaluated in its own materialized copy of the repository, so mutations cannot see each
other or overwrite the original source. `node_modules` is symlinked, not copied; see [Trust boundary](#trust-boundary).

tokio supervises execution. A `Semaphore` bounds concurrency. Each mutant gets its own timeout.
bughunter spawns every test process into its own process group with `setsid`. A timeout can therefore
`killpg` the whole group.

This matters because vitest spawns worker children. If you kill only the shell, those workers keep
running and eventually exhaust the host. One test in the suite genuinely fails without the `killpg`.

## Baseline check

bughunter runs your suite once before it mutates anything. If that run fails, bughunter stops:

```
error: baseline test command failed with exit status: 1 in /path/to/repo; mutation results
would be meaningless because every mutant would be reported killed. Fix the suite, or pass
--skip-baseline to override
```

This check separates a real result from a number-shaped artifact. A red suite reports every mutant as
`killed`, and the score then reads 100%. A suite that asserts nothing reports every mutant as
`survived`. Neither number tells you anything. bughunter therefore refuses to produce one.

## Reading the output

**A surviving mutant is not a bug.** It is a fact about your tests. Triage decides which:

1. The mutated behavior is reachable and wrong, and nothing catches it → a real defect. File it
   with a reproduction.
2. No test exercises that path → a test gap. Add the test.

Most survivors are case 2. A generated killing test is only worth having if it **fails against the
mutant and passes against clean source**; a test that passes both ways proves nothing.

### JSON schema and score

Every payload starts with `"schema_version": 2`. Consumers can reject an unknown format instead of
guessing how to parse it. The payload provides these top-level fields:

| field | meaning |
|---|---|
| `total` | all generated mutants, including unevaluated ones |
| `killed`, `survived`, `timeout`, `error` | explicit integer counts for each status; their sum is `total` |
| `evaluated` | `killed + survived`, the number of mutants with a test result |
| `score` | `killed / evaluated`, or `null` when `evaluated` is zero |
| `mutants` | individual findings, each with its current `line`, operator, status, and stable `id` |

Each mutant `id` is unique within a payload. It is a fixed-width lowercase FNV-1a hash.

The hash covers the relative file path, operator name, original span text, replacement text, and
stable source context that distinguishes same-operator mutations.

The hash excludes line numbers, byte offsets, and absolute paths. Add unrelated lines above a
mutation and its `line` changes, but its `id` does not. IDs stay stable across line shifts.

Inserting a new occurrence of the same operator earlier in a file leaves every pre-existing mutant
ID byte-identical, and the newly inserted mutant receives a fresh unused ID.
[`scripts/gate-id-stability.sh`](scripts/gate-id-stability.sh) proves this property, and CI runs it
on every push.

```
evaluated = killed + survived
score = killed / (killed + survived)
```

`timeout` and `error` are excluded from the score denominator: a mutant that could not be evaluated
is missing data, not evidence that the suite did or did not detect it. When no mutants were
evaluated, `score` is `null`, never `0.0`; zero would falsely claim that the suite caught none when
nothing was measured. Always report `n = evaluated` next to a score.

### Mutation density and interpreting score

Operator coverage is deliberately narrow: bughunter has eight operators. Do not add operators merely
to inflate the sample; that would change the access-check fixture's contractual total of 16 mutants
and break its regression gate. A small file can therefore yield very few mutants, and a score over a
handful of mutants is an anecdote, not a metric.

Equivalent mutants are semantically identical to the original and cannot be killed by any test, so
they impose a real ceiling below 1.0. For example, under the Web Streams contract,
`if (done || !value)` can be equivalent to either condition alone when either condition is sufficient
to take the same branch. Read the survivor list before chasing a number, and report the sample size
alongside every score.

The useful contrast is with line coverage. Coverage asks *did this line run?* The score asks *would
anyone notice if this line were wrong?* A line can run in every test and still be unchecked — see
the `withinQuota` boundary in the demo above, which has full line coverage and a 0% mutation score.

Do not compare scores across files. The denominator is operator sites, not effort. A file dense with
`&&` and `===` guards racks up mutants quickly; a file of straight-line assignments has almost none.
A very high score can also mean over-specified tests that pin behavior nobody depends on, making
future refactors harder rather than safer. Treat survivors as a to-read list, not a to-do list.

## Limitations

Honest list.

- **TypeScript and JavaScript only.**
- **No test-impact analysis.** Every mutant runs the whole suite you name. Narrow it with
  `--test` and `--line-range`. The cost is roughly `mutants × suite duration ÷ concurrency`.
- **No triage.** The tool reports survivors; deciding what they mean is still yours.
- **No incremental or cached runs.**
- **Equivalent mutants are not detected.** Some survivors are semantically identical to the
  original and are therefore unkillable. They will be reported as survivors anyway.
- **`--concurrency` above your core count slows the run down.** A suite that binds a fixed port
  cannot run in parallel with itself at all.

## Tests

```
cargo test --workspace
```

46 tests: 21 CLI, 15 engine, 10 runner. The runner tests cover killed, survived, timeout,
process-group orphan reaping, the `node_modules` symlink, and the concurrency bound.
