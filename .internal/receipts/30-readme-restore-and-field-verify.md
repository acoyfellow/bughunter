# README warning restoration and real-repository ID verification

## Result

- Restored the cleartext-secrets warning, shared-host pre-creation warning, and `sh -c` warning in the single `Trust boundary` section.
- Verified the materialization claims against the current runner source before editing the README.
- Ran the real `src/gitlab-egress.ts` source from the read-only target with all eight operators, `--test true`, `--skip-baseline`, and `--json`: **22 mutants, 22 distinct IDs**.
- Copied that source to temporary workspaces, inserted one new top-of-file `||` occurrence in a new function, and reran. All 22 original IDs were byte-identical and present; exactly one ID was added. **PASS**.

## Source verification before the README edit

The cleartext-warning premise is true in the current runner. Regular repository files are copied and then assigned the source file's permissions:

```rust
} else if file_type.is_file() {
    fs::copy(&source_path, &destination_path)?;
    fs::set_permissions(&destination_path, fs::metadata(&source_path)?.permissions())?;
}
```

The owner-only and no-reuse premise is also true. The run directory is created with `0o700`; `DirBuilder::create` fails if that exact path already exists, and the runner test verifies that behavior:

```rust
let path = materialization_root.join(directory_name);
let mut directory_builder = fs::DirBuilder::new();
directory_builder.mode(0o700);
directory_builder
    .create(&path)
    .map_err(|error| format!("failed to create materialized tree: {error}"))?;
if let Err(error) = set_owner_only_permissions(&path) {
    let _ = fs::remove_dir(&path);
    return Err(format!(
        "failed to restrict materialized tree permissions: {error}"
    ));
}
```

```rust
#[test]
fn preexisting_run_directory_is_not_reused() {
    let work_root = unique_work_root("preexisting");
    let existing_directory = create_run_directory_with_name(&work_root, "occupied").unwrap();
    let sentinel = existing_directory.join("sentinel");
    fs::write(&sentinel, "existing").unwrap();

    assert!(create_run_directory_with_name(&work_root, "occupied").is_err());
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "existing");

    fs::remove_dir_all(work_root).unwrap();
}
```

The `sh -c` warning is true as well:

```rust
let mut command = Command::new("sh");
command
    .arg("-c")
    .arg(&configuration.test_command)
    .current_dir(materialized_tree);
```

Conclusion: all three restored warnings are supported by the current source. No warning was omitted or qualified due to a source mismatch.

## README change

The README now has one `## Trust boundary` section. It retains the baseline/materialization path, source validation, `.git` exclusion, temporary-copy cleanup, dependency symlinks, absolute-path risk, and process-group details. It also states without softening:

- the temporary repository copy preserves original permissions and keeps `.env`, `.dev.vars`, credentials, and tokens in cleartext;
- another user on a shared host can pre-create a predictable run path and receive those cleartext secrets, despite fresh `0700` run-directory creation and no reuse; and
- `--test` is invoked with `sh -c` and can do anything the shell can do.

## Field verification on the read-only real repository

The real source under test was `src/gitlab-egress.ts`; `tests/gitlab-egress.test.ts` is its associated test file. The manual source copies and the one insertion were made only under `/tmp/bughunter-field.TywH0Y`. The target was never edited, staged, or committed.

### Real-source run

Command:

```sh
perl -e 'alarm 900; exec @ARGV' -- ./target/debug/bughunter run --repo /Users/jcoeyman/cloudflare/cloudbox --file src/gitlab-egress.ts --operators cond-boundary-gt,cond-boundary-lt,logical-and-to-or,logical-or-to-and,equality-strict-to-loose-neg,inequality-to-equality,return-true-to-false,return-false-to-true --test true --skip-baseline --json
```

Raw output:

```text
{"schema_version":2,"total":22,"killed":0,"survived":22,"timeout":0,"error":0,"evaluated":22,"score":0,"mutants":[{"id":"4e5237b5b7173d81","line":22,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"5adee592227bd9c0","line":22,"operator":"logical-or-to-and","status":"survived"},{"id":"3d546ffd95f355c0","line":22,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"4f02c4f630bc9ce5","line":22,"operator":"logical-or-to-and","status":"survived"},{"id":"b10ec70abe177c0b","line":22,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"05e01499fc6e6895","line":31,"operator":"logical-or-to-and","status":"survived"},{"id":"ab66ac7c4516a250","line":31,"operator":"logical-or-to-and","status":"survived"},{"id":"09ed620abfc34c00","line":40,"operator":"inequality-to-equality","status":"survived"},{"id":"c64dc5b0abdae595","line":40,"operator":"logical-and-to-or","status":"survived"},{"id":"b547a57ad11050e4","line":51,"operator":"logical-and-to-or","status":"survived"},{"id":"fe4f0fa7f3448002","line":51,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"492221e2c3eed207","line":54,"operator":"logical-and-to-or","status":"survived"},{"id":"c4607f82ae8c59f0","line":54,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"cfb0f7c9b00f9fbb","line":57,"operator":"logical-and-to-or","status":"survived"},{"id":"d9ae7d73f873e5ce","line":59,"operator":"logical-or-to-and","status":"survived"},{"id":"45113167c8f828d7","line":59,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"28d96cf140e348ca","line":62,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"c3d31cdebebbcb2c","line":67,"operator":"logical-or-to-and","status":"survived"},{"id":"0ed93453bde7600b","line":67,"operator":"logical-and-to-or","status":"survived"},{"id":"4cbec23f354359cc","line":81,"operator":"logical-or-to-and","status":"survived"},{"id":"0123d6ae6e2c0d39","line":81,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"a1e4eaf1d02334a9","line":85,"operator":"inequality-to-equality","status":"survived"}]}
REAL_SOURCE_ALL_OPERATORS_EXIT_STATUS=0
```

Total mutants: `22`.

ID count: `22`.

Distinct ID count: `22`.

### Temporary insertion and reruns

Only the modified temporary copy received this insertion, immediately after `GIT_SUFFIXES` and before the original mutation sites:

```diff
 const GIT_SUFFIXES = /\/(info\/refs|HEAD|git-upload-pack|git-receive-pack|objects\/.*)$/;

+function newlyIntroducedLogicalOr(left: boolean, right: boolean): boolean {
+  return left || right;
+}
+
 function unsafeRepo(repo: string): boolean {
```

Both temporary runs used the same command shape as the real-source run, with `--repo` set respectively to `/tmp/bughunter-field.TywH0Y/real-unmodified` and `/tmp/bughunter-field.TywH0Y/real-modified`.

Raw unmodified JSON:

```text
{"schema_version":2,"total":22,"killed":0,"survived":22,"timeout":0,"error":0,"evaluated":22,"score":0,"mutants":[{"id":"4e5237b5b7173d81","line":22,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"5adee592227bd9c0","line":22,"operator":"logical-or-to-and","status":"survived"},{"id":"3d546ffd95f355c0","line":22,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"4f02c4f630bc9ce5","line":22,"operator":"logical-or-to-and","status":"survived"},{"id":"b10ec70abe177c0b","line":22,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"05e01499fc6e6895","line":31,"operator":"logical-or-to-and","status":"survived"},{"id":"ab66ac7c4516a250","line":31,"operator":"logical-or-to-and","status":"survived"},{"id":"09ed620abfc34c00","line":40,"operator":"inequality-to-equality","status":"survived"},{"id":"c64dc5b0abdae595","line":40,"operator":"logical-and-to-or","status":"survived"},{"id":"b547a57ad11050e4","line":51,"operator":"logical-and-to-or","status":"survived"},{"id":"fe4f0fa7f3448002","line":51,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"492221e2c3eed207","line":54,"operator":"logical-and-to-or","status":"survived"},{"id":"c4607f82ae8c59f0","line":54,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"cfb0f7c9b00f9fbb","line":57,"operator":"logical-and-to-or","status":"survived"},{"id":"d9ae7d73f873e5ce","line":59,"operator":"logical-or-to-and","status":"survived"},{"id":"45113167c8f828d7","line":59,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"28d96cf140e348ca","line":62,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"c3d31cdebebbcb2c","line":67,"operator":"logical-or-to-and","status":"survived"},{"id":"0ed93453bde7600b","line":67,"operator":"logical-and-to-or","status":"survived"},{"id":"4cbec23f354359cc","line":81,"operator":"logical-or-to-and","status":"survived"},{"id":"0123d6ae6e2c0d39","line":81,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"a1e4eaf1d02334a9","line":85,"operator":"inequality-to-equality","status":"survived"}]}
REAL_UNMODIFIED_EXIT_STATUS=0
```

Raw modified JSON:

```text
{"schema_version":2,"total":23,"killed":0,"survived":23,"timeout":0,"error":0,"evaluated":23,"score":0,"mutants":[{"id":"b27014195f6115cc","line":22,"operator":"logical-or-to-and","status":"survived"},{"id":"4e5237b5b7173d81","line":26,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"5adee592227bd9c0","line":26,"operator":"logical-or-to-and","status":"survived"},{"id":"3d546ffd95f355c0","line":26,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"4f02c4f630bc9ce5","line":26,"operator":"logical-or-to-and","status":"survived"},{"id":"b10ec70abe177c0b","line":26,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"05e01499fc6e6895","line":35,"operator":"logical-or-to-and","status":"survived"},{"id":"ab66ac7c4516a250","line":35,"operator":"logical-or-to-and","status":"survived"},{"id":"09ed620abfc34c00","line":44,"operator":"inequality-to-equality","status":"survived"},{"id":"c64dc5b0abdae595","line":44,"operator":"logical-and-to-or","status":"survived"},{"id":"b547a57ad11050e4","line":55,"operator":"logical-and-to-or","status":"survived"},{"id":"fe4f0fa7f3448002","line":55,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"492221e2c3eed207","line":58,"operator":"logical-and-to-or","status":"survived"},{"id":"c4607f82ae8c59f0","line":58,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"cfb0f7c9b00f9fbb","line":61,"operator":"logical-and-to-or","status":"survived"},{"id":"d9ae7d73f873e5ce","line":63,"operator":"logical-or-to-and","status":"survived"},{"id":"45113167c8f828d7","line":63,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"28d96cf140e348ca","line":66,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"c3d31cdebebbcb2c","line":71,"operator":"logical-or-to-and","status":"survived"},{"id":"0ed93453bde7600b","line":71,"operator":"logical-and-to-or","status":"survived"},{"id":"4cbec23f354359cc","line":85,"operator":"logical-or-to-and","status":"survived"},{"id":"0123d6ae6e2c0d39","line":85,"operator":"equality-strict-to-loose-neg","status":"survived"},{"id":"a1e4eaf1d02334a9","line":89,"operator":"inequality-to-equality","status":"survived"}]}
REAL_MODIFIED_EXIT_STATUS=0
```

### Full sorted ID lists and set comparison

```text
UNMODIFIED_SORTED_IDS=0123d6ae6e2c0d39,05e01499fc6e6895,09ed620abfc34c00,0ed93453bde7600b,28d96cf140e348ca,3d546ffd95f355c0,45113167c8f828d7,492221e2c3eed207,4cbec23f354359cc,4e5237b5b7173d81,4f02c4f630bc9ce5,5adee592227bd9c0,a1e4eaf1d02334a9,ab66ac7c4516a250,b10ec70abe177c0b,b547a57ad11050e4,c3d31cdebebbcb2c,c4607f82ae8c59f0,c64dc5b0abdae595,cfb0f7c9b00f9fbb,d9ae7d73f873e5ce,fe4f0fa7f3448002
MODIFIED_SORTED_IDS=0123d6ae6e2c0d39,05e01499fc6e6895,09ed620abfc34c00,0ed93453bde7600b,28d96cf140e348ca,3d546ffd95f355c0,45113167c8f828d7,492221e2c3eed207,4cbec23f354359cc,4e5237b5b7173d81,4f02c4f630bc9ce5,5adee592227bd9c0,a1e4eaf1d02334a9,ab66ac7c4516a250,b10ec70abe177c0b,b27014195f6115cc,b547a57ad11050e4,c3d31cdebebbcb2c,c4607f82ae8c59f0,c64dc5b0abdae595,cfb0f7c9b00f9fbb,d9ae7d73f873e5ce,fe4f0fa7f3448002
PRESERVED_COUNT=22
LOST_COUNT=0
NEW_COUNT=1
NEW_SORTED_IDS=b27014195f6115cc
FIELD_ASSERTION_EXIT_STATUS=0
```

The original real-source and unmodified-copy ID sets were also identical. The 22 preserved IDs are byte-identical; no original ID was rehashed when the earlier `||` site was inserted.

### Read-only target cleanliness checks

Immediately after every target command (source inspection, direct mutation run, and each source copy), the following command was run:

```sh
git -C /Users/jcoeyman/cloudflare/cloudbox status --porcelain
```

Every invocation produced empty output. Representative raw output after the real mutation run:

```text
--- target status after real source all-operator mutation run ---
```

Representative raw output after each temporary source copy:

```text
--- target status after copying real source to unmodified temporary workspace ---
--- target status after copying real source to modified temporary workspace ---
```

## Required verification

### `cargo test --workspace`

Command:

```sh
perl -e 'alarm 900; exec @ARGV' -- cargo test --workspace
```

Raw output:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Running unittests src/main.rs (target/debug/deps/bughunter-13ba45d21032e74d)

running 37 tests
test tests::all_timeouts_with_the_flag_exit_three ... ok
test tests::all_errors_with_the_flag_exit_three ... ok
test tests::all_errors_without_the_flag_exit_zero ... ok
test tests::malformed_line_ranges_are_errors ... ok
test tests::no_survivors_with_the_flag_exit_zero ... ok
test tests::absent_config_leaves_defaults_intact ... ok
test tests::line_range_drops_mutants_outside_the_boundaries ... ok
test tests::different_operators_at_the_same_location_have_different_ids ... ok
test tests::json_serialization_includes_details_only_when_present ... ok
test tests::multi_file_sarif_uses_relative_artifact_uris ... ok
test tests::line_range_keeps_both_inclusive_boundaries ... ok
test tests::per_file_roll_up_counts_sum_to_the_overall_totals ... ok
test tests::config_supplies_values_when_cli_flags_are_absent ... ok
test tests::result_summary_reports_counts_and_score_for_mixed_statuses ... ok
test tests::result_summary_uses_null_score_when_no_mutants_were_evaluated ... ok
test tests::sarif_and_json_options_compose ... ok
test tests::skip_baseline_defaults_to_off_and_is_opt_in ... ok
test tests::cli_flags_override_every_configured_value ... ok
test tests::same_operator_mutants_have_distinct_json_ids ... ok
test tests::sarif_serialization_emits_one_result_for_a_surviving_mutant ... ok
test tests::sarif_artifact_uris_are_relative ... ok
test tests::same_operator_survivors_have_distinct_sarif_fingerprints ... ok
test tests::sarif_serialization_omits_killed_mutants ... ok
test tests::survivors_without_the_flag_exit_zero ... ok
test tests::malformed_config_is_an_error ... ok
test tests::stable_mutant_id_survives_a_line_shift ... ok
test tests::survivors_with_the_flag_exit_two ... ok
test tests::glob_with_zero_matches_is_an_error ... ok
test tests::unknown_operator_is_an_error ... ok
test tests::survivor_takes_precedence_over_an_error ... ok
test tests::version_output_uses_the_package_version ... ok
test tests::usage_lists_every_operator_and_status ... ok
test tests::unparseable_source_is_an_error_not_an_empty_result ... ok
test tests::unknown_config_key_is_an_error ... ok
test tests::discovery_is_deterministically_ordered ... ok
test tests::discovery_skips_test_files_and_ignored_directories ... ok
test tests::hanging_baseline_times_out_and_kills_its_process_group ... ok

test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s

     Running unittests src/lib.rs (target/debug/deps/bughunter_engine-57a199b8f984835d)

running 15 tests
test tests::does_not_mutate_inclusive_comparisons ... ok
test tests::mutates_logical_and_to_or ... ok
test tests::does_not_mutate_nonliteral_return ... ok
test tests::mutates_logical_or_to_and ... ok
test tests::apply_replaces_only_the_ast_selected_operator ... ok
test tests::mutates_return_false_to_true ... ok
test tests::finds_return_in_nested_arrow_function ... ok
test tests::mutates_return_true_to_false ... ok
test tests::mutates_cond_boundary_gt ... ok
test tests::mutates_strict_inequality_to_strict_equality ... ok
test tests::mutates_cond_boundary_lt ... ok
test tests::mutates_strict_equality_to_strict_inequality ... ok
test tests::finds_each_supported_operator ... ok
test tests::ignores_operator_text_in_strings_and_comments ... ok
test tests::results_are_stably_sorted_by_span ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/bughunter_runner-2a22d67c5e560972)

running 10 tests
test tests::bughunter_work_root_uses_configured_tmpdir ... ok
test tests::preexisting_run_directory_is_not_reused ... ok
test tests::run_directory_and_work_roots_are_owner_only ... ok
test tests::materialize_remaps_workspace_package_links_and_keeps_external_links ... ok
sh: definitely-not-a-real-binary-xyz: command not found
test tests::unavailable_test_command_is_an_error ... ok
test tests::nonzero_test_command_kills_the_mutant_and_uses_a_node_modules_entry ... ok
test tests::zero_test_command_survives_the_mutant ... ok
test tests::slow_test_command_times_out_without_being_killed_or_surviving ... ok
test tests::concurrency_limit_bounds_parallel_test_commands ... ok
test tests::timeout_kills_background_processes_in_the_process_group ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.02s

   Doc-tests bughunter_engine

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests bughunter_runner

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

CARGO_TEST_WORKSPACE_EXIT_STATUS=0
```

### `./scripts/gate.sh`

Command:

```sh
perl -e 'alarm 900; exec @ARGV' -- ./scripts/gate.sh
```

Raw output:

```text
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s
CHECK: access-check result schema, counts, score, survivors, timeouts, and errors
CHECK: workspace symlink resolves to the materialized mutant
CHECK: --version prints a semantic version
CHECK: --fail-on-survivors fails on the access-check survivors
CHECK: --fail-on-survivors exited 2 as expected
PASS: all gate checks succeeded
GATE_EXIT_STATUS=0
```

### `./scripts/gate-id-stability.sh`

Command:

```sh
perl -e 'alarm 900; exec @ARGV' -- ./scripts/gate-id-stability.sh
```

Raw output:

```text
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
CHECK: same-operator ids survive an earlier insertion
PASS: same-operator ids survive an earlier insertion with one fresh id
GATE_ID_STABILITY_EXIT_STATUS=0
```

### Leak scan

Command:

```sh
perl -e 'alarm 900; exec @ARGV' -- git grep -nEi 'jcoeyman|cloudbox|cfdata|loops\.ax|/Users/' -- . ':!.internal'
```

Raw output was empty. `git grep` uses exit status `1` for zero matches:

```text
LEAK_SCAN_EXIT_STATUS=1
```

### Author and committer identity

Command:

```sh
perl -e 'alarm 900; exec @ARGV' -- sh -c "git log --format='%an <%ae>|%cn <%ce>' | sort -u"
```

Raw output:

```text
acoyfellow <coeyman@gmail.com>|acoyfellow <coeyman@gmail.com>
AUTHOR_LOG_EXIT_STATUS=0
```

## Final field assertions

```text
REAL_REPO_MUTANT_COUNT=22
REAL_REPO_IDS_ALL_DISTINCT=yes
IDS_PRESERVED_COUNT=22
IDS_LOST_COUNT=0
IDS_NEW_COUNT=1
FIELD_INSERTION_STABILITY=PASS
TARGET_REPO_STILL_CLEAN=yes
```
