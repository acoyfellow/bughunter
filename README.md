# bughunter

A mutation-testing CLI for TypeScript. It changes one operator in your source, runs your real test
suite against the change, and reports whether any test noticed.

A mutant your tests kill is a line your tests genuinely cover. A mutant that **survives** is a line
your tests execute but do not check. Line coverage cannot tell those apart. This can.

```
$ bughunter run --repo ./my-app --file src/auth.ts \
    --operators logical-and-to-or,equality-strict-to-loose-neg \
    --test 'npx vitest run' --json
{"total":12,"mutants":[{"line":31,"operator":"logical-and-to-or","status":"survived"}, ...]}
```

## What it found

The reproducible demo is in [Try it in 30 seconds](#try-it-in-30-seconds) below. This section is
the wider evidence: five unrelated private repositories, one real source file each, all eight
operators, each project's own vitest suite. You cannot re-run these, so treat the table as a report
rather than a proof — but the two hand-verified survivors below are quoted in full so you can judge
the reasoning.

| repo | file | mutants | killed | survived | timeout | error | score |
|---|---|---:|---:|---:|---:|---:|---:|
| backwards | `src/engine.ts` | 21 | 16 | 5 | 0 | 0 | 76% |
| diffgate | `src/matcher.ts` | 26 | 18 | 8 | 0 | 0 | 69% |
| summon | `src/lib/oauth.ts` | 29 | 11 | 18 | 0 | 0 | 37% |
| up | `src/capabilities.ts` | 48 | 23 | 25 | 0 | 0 | 47% |
| vitest-visual-diff | `src/style.ts` | 16 | 7 | 9 | 0 | 0 | 43% |

140 mutants, zero timeouts, zero errors. Every suite was green before mutating.

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

## Install

Requires a Rust toolchain and a Unix host. Process-group control is mandatory, so Windows is a
compile error rather than a silent degradation.

```
git clone <this repo> && cd bughunter
cargo build --release
./target/release/bughunter --help
```

## Usage

```
bughunter run --repo <DIR> --file <RELATIVE.ts> --operators <IDS> --test <CMD> --json [OPTIONS]
```

| flag | meaning |
|---|---|
| `--repo <DIR>` | repository root; the test command runs here |
| `--file <REL>` | source file to mutate, relative to `--repo` |
| `--operators <IDS>` | comma-separated operator ids; an unknown id is a hard error |
| `--test <CMD>` | test command, run once per mutant |
| `--json` | emit JSON on stdout |
| `--line-range S-E` | only mutate lines S..E inclusive, 1-based |
| `--timeout-ms N` | per-mutant timeout, default 30000 |
| `--concurrency N` | mutants in flight, default 4 |
| `--skip-baseline` | do not verify the suite passes before mutating |

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

`timeout` and `error` are never counted as `killed`. A hung suite is an unknown, not a success.

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

Each mutant is evaluated in its own materialized copy of the repository, so mutants cannot see each
other and your working tree is never modified. `node_modules` is symlinked, not copied.

Execution is supervised by tokio. A `Semaphore` bounds concurrency, each mutant gets a timeout, and
every test process is spawned into its own process group via `setsid` so a timeout can `killpg` the
entire group. Vitest spawns worker children; killing only the shell leaves those workers running and
they will eventually exhaust the host. There is a test that genuinely fails without the `killpg`.

## Baseline check

Before mutating, the suite is run once, unmutated. If it fails, bughunter refuses to continue:

```
error: baseline test command failed with exit status: 1 in /path/to/repo; mutation results
would be meaningless because every mutant would be reported killed. Fix the suite, or pass
--skip-baseline to override
```

This is the difference between a real result and a number-shaped artifact. Against a red suite
every mutant is reported `killed` and the score reads 100%. Against a vacuous suite that asserts
nothing, every mutant `survived`. Neither tells you anything, so the tool declines to pretend.

## Reading the output

**A surviving mutant is not a bug.** It is a fact about your tests. Triage decides which:

1. The mutated behavior is reachable and wrong, and nothing catches it → a real defect. File it
   with a reproduction.
2. No test exercises that path → a test gap. Add the test.

Most survivors are case 2. A generated killing test is only worth having if it **fails against the
mutant and passes against clean source**; a test that passes both ways proves nothing.

### The score

```
score = killed / total
```

That is the whole formula. It answers one question: **when we deliberately broke this file, how
often did your tests complain?**

- **76%** — we broke it 21 ways, tests caught 16. Five breakages slipped through.
- **37%** — we broke it 29 ways, tests caught 11. Eighteen slipped through.

A higher score means more of your changes get noticed. It is a measure of how much your test suite
would protect you during a refactor.

The useful contrast is with line coverage. Coverage asks *did this line run?* The score asks *would
anyone notice if this line were wrong?* A line can run in every test and still be unchecked — see
the `withinQuota` boundary in the demo above, which has full line coverage and a 0% mutation score.
That is why the number is worth having.

**Do not compare scores across files.** The denominator is not effort, it is operator sites. A file
dense with `&&` and `===` guards racks up mutants quickly; a file of straight-line assignments has
almost none. A 37% guard-heavy validator may be better tested than a 76% file with three branches
in it. The number is only meaningful against *itself over time*: if a file drops from 70% to 50%,
someone added logic without adding checks.

Also do not chase 100%. Some mutants are **equivalent** — semantically identical to the original, so
no test can ever kill them. They are permanently unkillable and this tool does not detect them.
A very high score can also mean over-specified tests that pin behavior nobody depends on, which
makes future refactors harder rather than safer. Treat survivors as a to-read list, not a to-do
list.

## Limitations

Honest list.

- **One file per invocation.** No whole-repo crawl yet.
- **TypeScript and JavaScript only.**
- **No test-impact analysis.** Every mutant runs the whole suite you name. Narrow it with
  `--test` and `--line-range`. The cost is roughly `mutants × suite duration ÷ concurrency`.
- **No triage.** The tool reports survivors; deciding what they mean is still yours.
- **No incremental or cached runs.**
- **Equivalent mutants are not detected.** Some survivors are semantically identical to the
  original and are therefore unkillable. They will be reported as survivors anyway.
- **`--concurrency` above your core count will slow things down**, and a suite that binds a fixed
  port cannot run in parallel with itself at all.

## Tests

```
cargo test --workspace
```

28 tests: 15 engine, 6 runner, 7 CLI. The runner tests cover killed, survived, timeout,
process-group orphan reaping, the `node_modules` symlink, and the concurrency bound.

## Try it in 30 seconds

`examples/access-check` is a self-contained fixture: an access-control module and a 12-test suite
that passes. It has **no dependencies and no `node_modules`**. It uses Node's built-in test runner
and native TypeScript support, so it needs only Node 22.6+ and no install step.

```
cargo build
./target/debug/bughunter run \
  --repo examples/access-check --file src/access.ts \
  --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true \
  --test 'node --experimental-strip-types --test src/access.test.ts' --json
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
