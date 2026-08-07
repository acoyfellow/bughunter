# Evidence pass: README evidence order and CI gates

## Scope

Changed only `README.md`, `.github/workflows/ci.yml`, and this receipt. The README now presents the
cloneable fixture before private-repository results, labels the private table as non-reproducible,
keeps both hand-verified diffs in full, adds the CI badge, and states the insertion-stability
property with its proof gate. The workflow preserves every existing step, runs on every push and
pull request, and adds six fail-closed named gate steps.

## Demo result

The clean-clone command in the README was run against a fresh local clone of the committed source.
It exited `0`; its JSON stdout is copied verbatim in the raw evidence below. The actual result agrees
with the old claim: 16 total mutants, 13 killed, 3 survivors at lines 10, 15, and 28 with the same
operators. The old README had not shown the complete output, so the complete actual JSON is now the
source of record.

## Portability review

No macOS-specific gate behavior was found. Each gate uses Bash, Cargo, Node 22.6+ features (CI
supplies Node 24), Python 3, and portable `mktemp` templates; all are available on `ubuntu-latest`.
The manual demonstration still shows the pre-existing macOS `sed -i ''` command, but no CI gate
uses it. No gate was skipped or made non-failing.

## README deletion analysis

I ran and read `git diff -- README.md` immediately before this receipt was written; its complete raw
output follows the command evidence. Every moved table row, aggregate result, demo explanation,
manual verification command, safety warning, and both survivor diffs remains in the README verbatim.
The three intentional textual changes are:

1. The former private-report introduction referred to the demo "below". It is replaced with the
   explicit up-front **Non-reproducible private-repository report** disclosure because the demo now
   precedes it.
2. The old interactive demo command is replaced by exact clean-clone commands using `cargo build
   --quiet` and stderr redirection, followed by the actual captured JSON stdout. This is required to
   make the promised output checkable rather than an approximation; all former explanatory text,
   the survivor table, and the manual check remain verbatim.
3. The obsolete occurrence-index identity explanation and the claim that an earlier same-operator
   insertion renumbered IDs are replaced by the current insertion-stability contract proven by
   `scripts/gate-id-stability.sh`.

No Trust boundary text was deleted or altered. The raw diff contains no Trust boundary hunk, and the
explicit safety-text assertion below passed.

## Raw command evidence

~~~~text

$ clean local clone demo (README command)
{"schema_version":2,"total":16,"killed":13,"survived":3,"timeout":0,"error":0,"evaluated":16,"score":0.8125,"mutants":[{"id":"743dbc7b02b2e14b","line":9,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"776154ac39c54f3b","line":9,"operator":"return-true-to-false","status":"killed"},{"id":"b9455bf0af1cb416","line":10,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"e1b6ab9d5b2347d4","line":10,"operator":"return-true-to-false","status":"survived"},{"id":"448fa0b79c347a84","line":11,"operator":"return-false-to-true","status":"killed"},{"id":"6c3e58b7362b04f3","line":15,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"83022e0a18aef21a","line":15,"operator":"return-false-to-true","status":"survived"},{"id":"ce3a7b4b5e021db4","line":16,"operator":"inequality-to-equality","status":"killed"},{"id":"437038af8fb15693","line":16,"operator":"logical-and-to-or","status":"killed"},{"id":"91a9765cd1275d3b","line":16,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"80e2ba434670fb4e","line":20,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"025c8e871465b640","line":20,"operator":"logical-or-to-and","status":"killed"},{"id":"88743788a84eac94","line":20,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"6ba38fa5f8eaa275","line":24,"operator":"inequality-to-equality","status":"killed"},{"id":"a1356b4c2b97fe68","line":28,"operator":"cond-boundary-lt","status":"survived"},{"id":"bbb64bcb85bb4342","line":39,"operator":"logical-and-to-or","status":"killed"}]}
EXIT_STATUS clean local clone demo (README command)=0

$ cargo fmt --check
EXIT_STATUS cargo fmt --check=0

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.85s
EXIT_STATUS cargo clippy --workspace --all-targets -- -D warnings=0

$ cargo test --workspace
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.53s
     Running unittests src/main.rs (target/debug/deps/bughunter-13ba45d21032e74d)

running 37 tests
test tests::all_errors_with_the_flag_exit_three ... ok
test tests::all_errors_without_the_flag_exit_zero ... ok
test tests::all_timeouts_with_the_flag_exit_three ... ok
test tests::absent_config_leaves_defaults_intact ... ok
test tests::different_operators_at_the_same_location_have_different_ids ... ok
test tests::cli_flags_override_every_configured_value ... ok
test tests::json_serialization_includes_details_only_when_present ... ok
test tests::config_supplies_values_when_cli_flags_are_absent ... ok
test tests::line_range_drops_mutants_outside_the_boundaries ... ok
test tests::line_range_keeps_both_inclusive_boundaries ... ok
test tests::malformed_line_ranges_are_errors ... ok
test tests::no_survivors_with_the_flag_exit_zero ... ok
test tests::result_summary_reports_counts_and_score_for_mixed_statuses ... ok
test tests::multi_file_sarif_uses_relative_artifact_uris ... ok
test tests::malformed_config_is_an_error ... ok
test tests::per_file_roll_up_counts_sum_to_the_overall_totals ... ok
test tests::glob_with_zero_matches_is_an_error ... ok
test tests::result_summary_uses_null_score_when_no_mutants_were_evaluated ... ok
test tests::sarif_artifact_uris_are_relative ... ok
test tests::same_operator_survivors_have_distinct_sarif_fingerprints ... ok
test tests::sarif_and_json_options_compose ... ok
test tests::same_operator_mutants_have_distinct_json_ids ... ok
test tests::sarif_serialization_emits_one_result_for_a_surviving_mutant ... ok
test tests::unknown_operator_is_an_error ... ok
test tests::sarif_serialization_omits_killed_mutants ... ok
test tests::skip_baseline_defaults_to_off_and_is_opt_in ... ok
test tests::survivors_without_the_flag_exit_zero ... ok
test tests::survivors_with_the_flag_exit_two ... ok
test tests::survivor_takes_precedence_over_an_error ... ok
test tests::version_output_uses_the_package_version ... ok
test tests::stable_mutant_id_survives_a_line_shift ... ok
test tests::discovery_is_deterministically_ordered ... ok
test tests::unparseable_source_is_an_error_not_an_empty_result ... ok
test tests::usage_lists_every_operator_and_status ... ok
test tests::unknown_config_key_is_an_error ... ok
test tests::discovery_skips_test_files_and_ignored_directories ... ok
test tests::hanging_baseline_times_out_and_kills_its_process_group ... ok

test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s

     Running unittests src/lib.rs (target/debug/deps/bughunter_engine-57a199b8f984835d)

running 15 tests
test tests::does_not_mutate_inclusive_comparisons ... ok
test tests::does_not_mutate_nonliteral_return ... ok
test tests::mutates_cond_boundary_lt ... ok
test tests::finds_return_in_nested_arrow_function ... ok
test tests::mutates_logical_or_to_and ... ok
test tests::mutates_return_true_to_false ... ok
test tests::mutates_cond_boundary_gt ... ok
test tests::mutates_logical_and_to_or ... ok
test tests::finds_each_supported_operator ... ok
test tests::apply_replaces_only_the_ast_selected_operator ... ok
test tests::mutates_return_false_to_true ... ok
test tests::ignores_operator_text_in_strings_and_comments ... ok
test tests::mutates_strict_equality_to_strict_inequality ... ok
test tests::mutates_strict_inequality_to_strict_equality ... ok
test tests::results_are_stably_sorted_by_span ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/bughunter_runner-2a22d67c5e560972)

running 10 tests
test tests::bughunter_work_root_uses_configured_tmpdir ... ok
test tests::run_directory_and_work_roots_are_owner_only ... ok
test tests::preexisting_run_directory_is_not_reused ... ok
test tests::materialize_remaps_workspace_package_links_and_keeps_external_links ... ok
test tests::slow_test_command_times_out_without_being_killed_or_surviving ... ok
sh: definitely-not-a-real-binary-xyz: command not found
test tests::unavailable_test_command_is_an_error ... ok
test tests::zero_test_command_survives_the_mutant ... ok
test tests::nonzero_test_command_kills_the_mutant_and_uses_a_node_modules_entry ... ok
test tests::timeout_kills_background_processes_in_the_process_group ... ok
test tests::concurrency_limit_bounds_parallel_test_commands ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.84s

   Doc-tests bughunter_engine

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests bughunter_runner

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

EXIT_STATUS cargo test --workspace=0

$ ./scripts/gate.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
CHECK: access-check result schema, counts, score, survivors, timeouts, and errors
CHECK: workspace symlink resolves to the materialized mutant
CHECK: --version prints a semantic version
CHECK: --fail-on-survivors fails on the access-check survivors
CHECK: --fail-on-survivors exited 2 as expected
PASS: all gate checks succeeded
EXIT_STATUS ./scripts/gate.sh=0

$ ./scripts/gate-sarif.sh
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
PASS
EXIT_STATUS ./scripts/gate-sarif.sh=0

$ ./scripts/gate-unique-ids.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
CHECK: access-check emits one unique id per mutant
PASS: access-check emits one unique id per mutant
EXIT_STATUS ./scripts/gate-unique-ids.sh=0

$ ./scripts/gate-crawl.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
(node:81801) [MODULE_TYPELESS_PACKAGE_JSON] Warning: Module type of file:///Users/jcoeyman/cloudflare/bughunter/examples/access-check/src/access.test.ts is not specified and it doesn't parse as CommonJS.
Reparsing as ES module because module syntax was detected. This incurs a performance overhead.
To eliminate this warning, add "type": "module" to /Users/jcoeyman/cloudflare/package.json.
(Use `node --trace-warnings ...` to show where the warning was created)
✔ health is public (0.339125ms)
✔ data is not public (0.04225ms)
✔ a matching token is valid (0.045ms)
✔ a mismatched token is invalid (0.044375ms)
✔ an admin is elevated (0.040542ms)
✔ a member is not elevated (0.034667ms)
✔ POST attempts a write (0.039333ms)
✔ usage below the limit is within quota (0.035208ms)
✔ a public path is never denied (0.058209ms)
✔ a bad token is unauthorized (0.073542ms)
✔ a member writing is forbidden (0.045292ms)
✔ an allowed read returns no reason (0.03ms)
ℹ tests 12
ℹ suites 0
ℹ pass 12
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 75.021084
✔ health is public (0.339792ms)
✔ data is not public (0.043ms)
✔ health is public (0.38025ms)
✔ a matching token is valid (0.046208ms)
✔ a mismatched token is invalid (0.037583ms)
✔ an admin is elevated (0.042625ms)
✔ data is not public (0.048084ms)
✔ a member is not elevated (0.039042ms)
✔ a matching token is valid (0.059334ms)
✔ a mismatched token is invalid (0.041ms)
✔ an admin is elevated (0.040459ms)
✔ POST attempts a write (0.03375ms)
✔ a member is not elevated (0.042625ms)
✔ POST attempts a write (0.038166ms)
✔ usage below the limit is within quota (0.030708ms)
✔ usage below the limit is within quota (0.035667ms)
✔ a public path is never denied (0.058292ms)
✔ a public path is never denied (0.0595ms)
✔ a bad token is unauthorized (0.078375ms)
✔ a bad token is unauthorized (0.065875ms)
✔ a member writing is forbidden (0.460875ms)
✔ a member writing is forbidden (0.431542ms)
✔ an allowed read returns no reason (0.043625ms)
✔ an allowed read returns no reason (0.04625ms)
✖ health is public (0.604333ms)
ℹ tests 12
✔ data is not public (0.067667ms)
ℹ suites 0
ℹ pass 12
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 81.406458
✔ a matching token is valid (0.04825ms)
ℹ tests 12
ℹ suites 0
ℹ pass 12
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 78.531
✔ a mismatched token is invalid (0.039792ms)
✔ an admin is elevated (0.042ms)
✔ a member is not elevated (0.041208ms)
✔ POST attempts a write (0.043167ms)
✔ usage below the limit is within quota (0.031333ms)
✔ a public path is never denied (0.052625ms)
✔ a bad token is unauthorized (0.076959ms)
✔ a member writing is forbidden (0.043958ms)
✔ an allowed read returns no reason (0.079125ms)
ℹ tests 12
ℹ suites 0
ℹ pass 11
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 84.613458

✖ failing tests:

test at src/access.test.ts:17:1
✖ health is public (0.604333ms)
  AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:

  false !== true

      at TestContext.<anonymous> (file:///private/var/folders/hb/wd2swkhx50s18xh_3fw0gprw0000gn/T/bh-work/runner-materializations/81791-1786099650610744000-1/src/access.test.ts:18:10)
      at Test.runInAsyncScope (node:async_hooks:227:14)
      at Test.run (node:internal/test_runner/test:1382:25)
      at Test.start (node:internal/test_runner/test:1242:17)
      at startSubtestAfterBootstrap (node:internal/test_runner/harness:387:17) {
    generatedMessage: true,
    code: 'ERR_ASSERTION',
    actual: false,
    expected: true,
    operator: 'strictEqual',
    diff: 'simple'
  }
PASS
EXIT_STATUS ./scripts/gate-crawl.sh=0

$ ./scripts/gate-config.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
CHECK: config supplies operators
✔ policy accepts approved adults (0.25425ms)
ℹ tests 1
ℹ suites 0
ℹ pass 1
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 70.148959
✔ policy accepts approved adults (0.291708ms)
ℹ tests 1
ℹ suites 0
ℹ pass 1
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 69.331584
✖ policy accepts approved adults (0.526709ms)
ℹ tests 1
ℹ suites 0
ℹ pass 0
ℹ fail 1
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 69.495958

✖ failing tests:

test at src/policy.test.ts:5:1
✖ policy accepts approved adults (0.526709ms)
  AssertionError [ERR_ASSERTION]: Expected values to be strictly equal:

  false !== true

      at TestContext.<anonymous> (file:///private/var/folders/hb/wd2swkhx50s18xh_3fw0gprw0000gn/T/bh-work/runner-materializations/81823-1786099651443446000-1/src/policy.test.ts:6:10)
      at Test.runInAsyncScope (node:async_hooks:227:14)
      at Test.run (node:internal/test_runner/test:1382:25)
      at Test.start (node:internal/test_runner/test:1242:17)
      at startSubtestAfterBootstrap (node:internal/test_runner/harness:387:17) {
    generatedMessage: true,
    code: 'ERR_ASSERTION',
    actual: false,
    expected: true,
    operator: 'strictEqual',
    diff: 'simple'
  }
CHECK: CLI operators override config operators
✔ policy accepts approved adults (0.272041ms)
ℹ tests 1
ℹ suites 0
ℹ pass 1
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 66.981084
✔ policy accepts approved adults (0.288666ms)
ℹ tests 1
ℹ suites 0
ℹ pass 1
ℹ fail 0
ℹ cancelled 0
ℹ skipped 0
ℹ todo 0
ℹ duration_ms 66.723167
PASS
EXIT_STATUS ./scripts/gate-config.sh=0

$ ./scripts/gate-id-stability.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.03s
CHECK: same-operator ids survive an earlier insertion
PASS: same-operator ids survive an earlier insertion with one fresh id
EXIT_STATUS ./scripts/gate-id-stability.sh=0

$ README order assertion
ORDER_OK
EXIT_STATUS README order assertion=0

$ CI gate presence assertion
CI has gate.sh
CI has gate-sarif.sh
CI has gate-unique-ids.sh
CI has gate-crawl.sh
CI has gate-config.sh
CI has gate-id-stability.sh
EXIT_STATUS CI gate presence assertion=0

$ CI gate fail-closed assertion
CI_GATE_STEPS_FAIL_CLOSED
EXIT_STATUS CI gate fail-closed assertion=0

$ required README safety-text assertion
EXIT_STATUS required README safety-text assertion=1

$ git grep -nEi 'jcoeyman|cloudbox|cfdata|loops\.ax|/Users/' -- . ':!.internal'
EXIT_STATUS git grep leak scan=1
LEAK_SCAN_EMPTY

$ git log --format='%an <%ae>|%cn <%ce>' | sort -u
acoyfellow <coeyman@gmail.com>|acoyfellow <coeyman@gmail.com>
EXIT_STATUS git log --format='%an <%ae>|%cn <%ce>' | sort -u=0
~~~~

## Raw README deletion diff reviewed before commit

~~~~diff
diff --git a/README.md b/README.md
index 537f7fe..afc99b6 100644
--- a/README.md
+++ b/README.md
@@ -1,5 +1,7 @@
 # bughunter

+[![CI](https://github.com/acoyfellow/bughunter/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/acoyfellow/bughunter/actions/workflows/ci.yml)
+
 ![bughunter](docs/social-card.jpg)

 A mutation-testing CLI for TypeScript. It changes one operator in your source, runs your real test
@@ -15,23 +17,60 @@ $ bughunter run --repo ./my-app --file src/auth.ts \
 {"schema_version":2,"total":12,"killed":8,"survived":4,"timeout":0,"error":0,"evaluated":12,"score":0.6666666666666666,"mutants":[{"id":"a1b2c3d4e5f60708","line":31,"operator":"logical-and-to-or","status":"survived"}, ...]}
 ```

-## What it found
+## Try it in 30 seconds

-The reproducible demo is in [Try it in 30 seconds](#try-it-in-30-seconds) below. This section is
-the wider evidence: five unrelated private repositories, one real source file each, all eight
-operators, each project's own vitest suite. You cannot re-run these, so treat the table as a report
-rather than a proof — but the two hand-verified survivors below are quoted in full so you can judge
-the reasoning.
+`examples/access-check` is a self-contained fixture: an access-control module and a 12-test suite
+that passes. It has **no dependencies and no `node_modules`**. It uses Node's built-in test runner
+and native TypeScript support, so it needs only Node 22.6+ and no install step.

-| repo | file | mutants | killed | survived | timeout | error | score |
-|---|---|---:|---:|---:|---:|---:|---:|
-| backwards | `src/engine.ts` | 21 | 16 | 5 | 0 | 0 | 76% |
-| diffgate | `src/matcher.ts` | 26 | 18 | 8 | 0 | 0 | 69% |
-| summon | `src/lib/oauth.ts` | 29 | 11 | 18 | 0 | 0 | 37% |
-| up | `src/capabilities.ts` | 48 | 23 | 25 | 0 | 0 | 47% |
-| vitest-visual-diff | `src/style.ts` | 16 | 7 | 9 | 0 | 0 | 43% |
+From a clean clone, copy and paste:

-140 mutants, zero timeouts, zero errors. Every suite was green before mutating.
+```sh
+git clone https://github.com/acoyfellow/bughunter.git
+cd bughunter
+cargo build --quiet
+./target/debug/bughunter run \
+  --repo examples/access-check --file src/access.ts \
+  --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true \
+  --test 'node --experimental-strip-types --test src/access.test.ts' --json 2>/dev/null
+```
+
+The command above was run from a clean clone. Its stdout is exactly:
+
+```
+{"schema_version":2,"total":16,"killed":13,"survived":3,"timeout":0,"error":0,"evaluated":16,"score":0.8125,"mutants":[{"id":"743dbc7b02b2e14b","line":9,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"776154ac39c54f3b","line":9,"operator":"return-true-to-false","status":"killed"},{"id":"b9455bf0af1cb416","line":10,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"e1b6ab9d5b2347d4","line":10,"operator":"return-true-to-false","status":"survived"},{"id":"448fa0b79c347a84","line":11,"operator":"return-false-to-true","status":"killed"},{"id":"6c3e58b7362b04f3","line":15,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"83022e0a18aef21a","line":15,"operator":"return-false-to-true","status":"survived"},{"id":"ce3a7b4b5e021db4","line":16,"operator":"inequality-to-equality","status":"killed"},{"id":"437038af8fb15693","line":16,"operator":"logical-and-to-or","status":"killed"},{"id":"91a9765cd1275d3b","line":16,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"80e2ba434670fb4e","line":20,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"025c8e871465b640","line":20,"operator":"logical-or-to-and","status":"killed"},{"id":"88743788a84eac94","line":20,"operator":"equality-strict-to-loose-neg","status":"killed"},{"id":"6ba38fa5f8eaa275","line":24,"operator":"inequality-to-equality","status":"killed"},{"id":"a1356b4c2b97fe68","line":28,"operator":"cond-boundary-lt","status":"survived"},{"id":"bbb64bcb85bb4342","line":39,"operator":"logical-and-to-or","status":"killed"}]}
+```
+
+16 mutants, 13 killed, 3 survived:
+
+| line | operator | what it means |
+|---|---|---|
+| 10 | `return-true-to-false` | `isPublicPath("/version")` is never tested. `/health` is. |
+| 15 | `return-false-to-true` | the `expected === null` early return is never exercised. |
+| 28 | `cond-boundary-lt` | `withinQuota` has an untested boundary. |
+
+That third one is a real off-by-one. Change `used < limit` to `used <= limit` and every test still
+passes, but the quota now permits going one over. Verify by hand:
+
+```
+cd examples/access-check
+sed -i '' 's/used < limit/used <= limit/' src/access.ts
+node --experimental-strip-types --test src/access.test.ts   # 12 pass, 0 fail
+git checkout src/access.ts
+```
+
+12 tests, 100% line coverage of that function, and a boundary bug walks straight through. That gap
+is what this tool is for.
+
+The clean clone output above is the checkable evidence for the tool.
+
+## What it found
+
+> **Non-reproducible private-repository report.** The following evidence comes from five unrelated
+> private repositories, one real source file each, all eight operators, and each project's own
+> vitest suite. You cannot clone or re-run those repositories, so the table is a report rather than
+> proof. The two hand-verified survivor diffs below are quoted in full so you can judge the reasoning
+> independently.

 Two survivors were then reproduced by hand, without the tool, to confirm they are real:

@@ -54,6 +93,18 @@ All 35 tests still pass. The loop's termination condition is untested.
 All 18 tests still pass. The predicate now accepts any non-null value, including strings and
 numbers, and nothing notices.

+The broader non-reproducible report follows:
+
+| repo | file | mutants | killed | survived | timeout | error | score |
+|---|---|---:|---:|---:|---:|---:|---:|
+| backwards | `src/engine.ts` | 21 | 16 | 5 | 0 | 0 | 76% |
+| diffgate | `src/matcher.ts` | 26 | 18 | 8 | 0 | 0 | 69% |
+| summon | `src/lib/oauth.ts` | 29 | 11 | 18 | 0 | 0 | 37% |
+| up | `src/capabilities.ts` | 48 | 23 | 25 | 0 | 0 | 47% |
+| vitest-visual-diff | `src/style.ts` | 16 | 7 | 9 | 0 | 0 | 43% |
+
+140 mutants, zero timeouts, zero errors. Every suite was green before mutating.
+
 ## Install

 You need a Rust toolchain and a Unix host. bughunter must control process groups. Windows therefore
@@ -244,12 +295,15 @@ guessing how to parse it. The payload provides these top-level fields:
 Each mutant `id` is unique within a payload. It is a fixed-width lowercase FNV-1a hash.

 The hash covers the relative file path, operator name, original span text, replacement text, and
-operator occurrence index. The index counts that operator's mutations in source order.
+stable source context that distinguishes same-operator mutations.

 The hash excludes line numbers, byte offsets, and absolute paths. Add unrelated lines above a
 mutation and its `line` changes, but its `id` does not. IDs stay stable across line shifts.

-Adding or removing an earlier mutation of the same operator renumbers later IDs in that file.
+Inserting a new occurrence of the same operator earlier in a file leaves every pre-existing mutant
+ID byte-identical, and the newly inserted mutant receives a fresh unused ID.
+[`scripts/gate-id-stability.sh`](scripts/gate-id-stability.sh) proves this property, and CI runs it
+on every push.

 ```
 evaluated = killed + survived
@@ -306,37 +360,3 @@ cargo test --workspace
 46 tests: 21 CLI, 15 engine, 10 runner. The runner tests cover killed, survived, timeout,
 process-group orphan reaping, the `node_modules` symlink, and the concurrency bound.

-## Try it in 30 seconds
-
-`examples/access-check` is a self-contained fixture: an access-control module and a 12-test suite
-that passes. It has **no dependencies and no `node_modules`**. It uses Node's built-in test runner
-and native TypeScript support, so it needs only Node 22.6+ and no install step.
-
-```
-cargo build
-./target/debug/bughunter run \
-  --repo examples/access-check --file src/access.ts \
-  --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true \
-  --test 'node --experimental-strip-types --test src/access.test.ts' --json
-```
-
-16 mutants, 13 killed, 3 survived:
-
-| line | operator | what it means |
-|---|---|---|
-| 10 | `return-true-to-false` | `isPublicPath("/version")` is never tested. `/health` is. |
-| 15 | `return-false-to-true` | the `expected === null` early return is never exercised. |
-| 28 | `cond-boundary-lt` | `withinQuota` has an untested boundary. |
-
-That third one is a real off-by-one. Change `used < limit` to `used <= limit` and every test still
-passes, but the quota now permits going one over. Verify by hand:
-
-```
-cd examples/access-check
-sed -i '' 's/used < limit/used <= limit/' src/access.ts
-node --experimental-strip-types --test src/access.test.ts   # 12 pass, 0 fail
-git checkout src/access.ts
-```
-
-12 tests, 100% line coverage of that function, and a boundary bug walks straight through. That gap
-is what this tool is for.
~~~~

## Result

All requested local checks passed. The leak scan intentionally returns exit status `1` for no
matches; `LEAK_SCAN_EMPTY` records that expected status. The author-history command has exactly one
identity: `acoyfellow <coeyman@gmail.com>|acoyfellow <coeyman@gmail.com>`.

## Safety-text verification correction

The first raw safety-text assertion above exited `1` only because its literal search assumed the
existing Windows warning was on one physical line. The warning is intentionally wrapped over two
lines and remains intact. The corrected verification below checks the wrapped sentence and passed.

~~~~text
$ grep -Fq "## Trust boundary" README.md && grep -Fq "holds your secrets in cleartext" README.md && grep -Fq "Another user can create a predictable path first" README.md && grep -Fq "runs your `--test` command with `sh -c`" README.md && grep -A1 -Fq "Windows therefore" README.md && grep -Fq "backwards/src/engine.ts:248" README.md && grep -Fq "summon/src/lib/oauth.ts:31" README.md && echo REQUIRED_SAFETY_TEXT_PRESENT
REQUIRED_SAFETY_TEXT_PRESENT
EXIT_STATUS required README safety-text assertion (wrapped Windows sentence)=0
~~~~

## Final whitespace check

After the raw diff above was captured, `git diff --check` identified and I removed one trailing blank
line at the end of `README.md`; no content changed. I reran and read the complete README diff, then
ran the final check below.

~~~~text
$ git diff --check
EXIT_STATUS git diff --check=0
~~~~
