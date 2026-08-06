# Trust-boundary documentation receipt

## Source verification

Facts 1 and 3 are established by `crates/runner/src/lib.rs`. Fact 2's stated file location was incorrect: `verify_baseline` is implemented in `crates/cli/src/main.rs`, not in the runner library. The behavior stated in fact 2 is correct.

### Fact 1: materialization and per-mutant execution

The runner canonicalizes the repository, resolves the source beneath it, creates a materialization, copies the repository, and writes the mutation only there:

```rust
let repository = fs::canonicalize(&configuration.repository)
    .map_err(|error| format!("failed to resolve repository: {error}"))?;
let source_file = resolve_source_file(&repository, &configuration.source_file)?;
let relative_source_file = source_file
    .strip_prefix(&repository)
    .map_err(|error| format!("source file is outside repository: {error}"))?;
let metadata = fs::symlink_metadata(&source_file)
    .map_err(|error| format!("failed to inspect source file: {error}"))?;
if metadata.file_type().is_symlink() || !metadata.is_file() {
    return Err("source file must be a regular file".to_owned());
}

let source = fs::read_to_string(&source_file)
    .map_err(|error| format!("failed to read source file: {error}"))?;
let mutated_source = replace_mutant(&source, mutant)?;
let materialized_tree = create_run_directory()?;

if let Err(error) = copy_repository(&repository, &materialized_tree) {
    let _ = fs::remove_dir_all(&materialized_tree);
    return Err(format!("failed to materialize repository: {error}"));
}

let materialized_source_file = materialized_tree.join(relative_source_file);
if let Err(error) = fs::write(&materialized_source_file, mutated_source) {
    let _ = fs::remove_dir_all(&materialized_tree);
    return Err(format!("failed to write mutated source file: {error}"));
}
```

The directory name contains the process ID, timestamp, and sequence; the mode is `0700`:

```rust
let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
    .as_nanos();
let sequence = RUN_DIRECTORY_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
let directory_name = format!("{}-{timestamp}-{sequence}", std::process::id());
create_run_directory_with_name(&bughunter_work_root(), &directory_name)
```

```rust
let path = materialization_root.join(directory_name);
let mut directory_builder = fs::DirBuilder::new();
directory_builder.mode(0o700);
directory_builder
    .create(&path)
    .map_err(|error| format!("failed to create materialized tree: {error}"))?;
```

The work root and recursive copy exclusion are:

```rust
fn bughunter_work_root_from(temporary_directory: &Path) -> PathBuf {
    temporary_directory.join("bh-work")
}
```

```rust
for entry in fs::read_dir(source)? {
    let entry = entry?;
    let name = entry.file_name();
    if name == OsStr::new(".git") {
        continue;
    }
```

```rust
} else {
    copy_directory(
        &source_path,
        &destination_path,
        repository,
        materialized_repository,
    )?;
}
```

Per-mutant commands use the materialized tree and it is removed afterward:

```rust
let result = run_command(configuration, &mutant, &materialized_tree).await;
if let Err(error) = fs::remove_dir_all(&materialized_tree) {
```

```rust
command
    .arg("-c")
    .arg(&configuration.test_command)
    .current_dir(materialized_tree);
```

### Fact 2: baseline execution

`crates/runner/src/lib.rs` has no `verify_baseline`. The CLI invokes it before constructing any runner:

```rust
if !options.skip_baseline {
    verify_baseline(&options).await?;
}
```

The CLI implementation runs the supplied command directly in the real repository:

```rust
command
    .arg("-c")
    .arg(format!("({}) 1>&2", options.test_command))
    .current_dir(&options.repository);
```

### Fact 3: dependency symlinks

`copy_node_modules_directory` symlinks regular files and ordinary dependency directories to their originals:

```rust
} else if file_type.is_file() {
    symlink_original(&source_path, &destination_path)?;
} else if file_type.is_dir() {
    if node_modules_container(&name, &source_path) {
        fs::create_dir(&destination_path)?;
        copy_node_modules_directory(
            &source_path,
            &destination_path,
            repository,
            materialized_repository,
        )?;
    } else {
        symlink_original(&source_path, &destination_path)?;
    }
}
```

```rust
fn symlink_original(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}
```

## Documentation result

The README now states that `--test` is trusted input, bughunter is not a security sandbox, baseline execution can write directly to the real repository, dependency symlinks can write to real dependencies, absolute paths remain unrestricted, and process groups only limit timeout orphans.

## Verification

Each command was launched without a pipeline through `perl -e 'alarm 900; exec @ARGV' --`. The commands completed before the first 30-second progress interval.

### Workspace tests

```text
COMMAND: cargo test --workspace
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.19s
     Running unittests src/main.rs (target/debug/deps/bughunter-13ba45d21032e74d)

running 37 tests
test tests::all_errors_without_the_flag_exit_zero ... ok
test tests::all_timeouts_with_the_flag_exit_three ... ok
test tests::all_errors_with_the_flag_exit_three ... ok
test tests::malformed_line_ranges_are_errors ... ok
test tests::no_survivors_with_the_flag_exit_zero ... ok
test tests::per_file_roll_up_counts_sum_to_the_overall_totals ... ok
test tests::line_range_keeps_both_inclusive_boundaries ... ok
test tests::line_range_drops_mutants_outside_the_boundaries ... ok
test tests::json_serialization_includes_details_only_when_present ... ok
test tests::result_summary_reports_counts_and_score_for_mixed_statuses ... ok
test tests::different_operators_at_the_same_location_have_different_ids ... ok
test tests::absent_config_leaves_defaults_intact ... ok
test tests::multi_file_sarif_uses_relative_artifact_uris ... ok
test tests::result_summary_uses_null_score_when_no_mutants_were_evaluated ... ok
test tests::sarif_and_json_options_compose ... ok
test tests::same_operator_mutants_have_distinct_json_ids ... ok
test tests::sarif_serialization_omits_killed_mutants ... ok
test tests::sarif_artifact_uris_are_relative ... ok
test tests::sarif_serialization_emits_one_result_for_a_surviving_mutant ... ok
test tests::same_operator_survivors_have_distinct_sarif_fingerprints ... ok
test tests::skip_baseline_defaults_to_off_and_is_opt_in ... ok
test tests::stable_mutant_id_survives_a_line_shift ... ok
test tests::survivor_takes_precedence_over_an_error ... ok
test tests::survivors_with_the_flag_exit_two ... ok
test tests::survivors_without_the_flag_exit_zero ... ok
test tests::unknown_operator_is_an_error ... ok
test tests::version_output_uses_the_package_version ... ok
test tests::usage_lists_every_operator_and_status ... ok
test tests::unparseable_source_is_an_error_not_an_empty_result ... ok
test tests::config_supplies_values_when_cli_flags_are_absent ... ok
test tests::cli_flags_override_every_configured_value ... ok
test tests::malformed_config_is_an_error ... ok
test tests::unknown_config_key_is_an_error ... ok
test tests::glob_with_zero_matches_is_an_error ... ok
test tests::discovery_is_deterministically_ordered ... ok
test tests::discovery_skips_test_files_and_ignored_directories ... ok
test tests::hanging_baseline_times_out_and_kills_its_process_group ... ok

test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s

     Running unittests src/lib.rs (target/debug/deps/bughunter_engine-57a199b8f984835d)

running 15 tests
test tests::does_not_mutate_inclusive_comparisons ... ok
test tests::mutates_return_false_to_true ... ok
test tests::mutates_logical_or_to_and ... ok
test tests::mutates_strict_inequality_to_strict_equality ... ok
test tests::finds_return_in_nested_arrow_function ... ok
test tests::mutates_return_true_to_false ... ok
test tests::mutates_cond_boundary_gt ... ok
test tests::does_not_mutate_nonliteral_return ... ok
test tests::mutates_logical_and_to_or ... ok
test tests::mutates_strict_equality_to_strict_inequality ... ok
test tests::apply_replaces_only_the_ast_selected_operator ... ok
test tests::mutates_cond_boundary_lt ... ok
test tests::ignores_operator_text_in_strings_and_comments ... ok
test tests::finds_each_supported_operator ... ok
test tests::results_are_stably_sorted_by_span ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/bughunter_runner-2a22d67c5e560972)

running 10 tests
test tests::bughunter_work_root_uses_configured_tmpdir ... ok
test tests::run_directory_and_work_roots_are_owner_only ... ok
test tests::preexisting_run_directory_is_not_reused ... ok
test tests::materialize_remaps_workspace_package_links_and_keeps_external_links ... ok
sh: definitely-not-a-real-binary-xyz: command not found
test tests::nonzero_test_command_kills_the_mutant_and_uses_a_node_modules_entry ... ok
test tests::zero_test_command_survives_the_mutant ... ok
test tests::slow_test_command_times_out_without_being_killed_or_surviving ... ok
test tests::unavailable_test_command_is_an_error ... ok
test tests::concurrency_limit_bounds_parallel_test_commands ... ok
test tests::timeout_kills_background_processes_in_the_process_group ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.04s

   Doc-tests bughunter_engine

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests bughunter_runner

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

EXIT_STATUS=0
```

### Gate

```text
COMMAND: ./scripts/gate.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s
CHECK: access-check result schema, counts, score, survivors, timeouts, and errors
CHECK: workspace symlink resolves to the materialized mutant
CHECK: --version prints a semantic version
CHECK: --fail-on-survivors fails on the access-check survivors
CHECK: --fail-on-survivors exited 2 as expected
PASS: all gate checks succeeded
EXIT_STATUS=0
```

### ID-stability gate

```text
COMMAND: ./scripts/gate-id-stability.sh
CHECK: building debug binary
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s
CHECK: same-operator ids survive an earlier insertion
PASS: same-operator ids survive an earlier insertion with one fresh id
EXIT_STATUS=0
```

### Tracked-content leak scan

```text
COMMAND: git grep -nEi 'jcoeyman|cloudbox|cfdata|loops\.ax|/Users/' -- . ':!.internal'
EXIT_STATUS=1
```

The command produced no output. Exit status `1` is `git grep`'s real no-match status.

### Commit-author audit before this commit

```text
COMMAND: git log --format='%an <%ae>|%cn <%ce>' | sort -u
acoyfellow <coeyman@gmail.com>|acoyfellow <coeyman@gmail.com>
EXIT_STATUS=0
```
