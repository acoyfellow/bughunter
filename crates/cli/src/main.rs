use bughunter_engine::{try_mutants, Operator};
use bughunter_runner::{MutantResult, MutantStatus, Runner, RunnerConfig};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

unsafe extern "C" {
    fn setsid() -> i32;
    fn killpg(process_group: i32, signal: i32) -> i32;
}

const SIGNAL_KILL: i32 = 9;
const NO_SUCH_PROCESS: i32 = 3;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CONCURRENCY: usize = 4;

#[tokio::main]
async fn main() {
    match run(env::args().skip(1).collect()).await {
        Ok(exit_code) if exit_code != 0 => std::process::exit(exit_code),
        Ok(_) => {}
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

const USAGE: &str = "bughunter - mutation testing for TypeScript

USAGE:
  bughunter run --repo <DIR> --file <RELATIVE.ts|DIR|GLOB> --operators <IDS> --test <CMD> --json [OPTIONS]

REQUIRED:
  --repo <DIR>          repository root; the test command runs here
  --file <PATH>         source file, directory, or glob to mutate, relative to --repo
  --operators <IDS>     comma-separated operator ids (see below)
  --test <CMD>          test command, run once per mutant
  --json                emit JSON on stdout
  --sarif <PATH>        write SARIF 2.1.0 output to PATH

OPTIONS:
  --line-range S-E      only mutate lines S..E inclusive, 1-based
  --timeout-ms N        per-mutant timeout, default 30000
  --concurrency N       mutants in flight, default 4
  --skip-baseline       do not verify the suite passes before mutating
  --fail-on-survivors   gate on survivors and unevaluated mutants
  --version             print the bughunter version

OPERATORS:
  cond-boundary-gt              >   ->  >=
  cond-boundary-lt              <   ->  <=
  logical-and-to-or             &&  ->  ||
  logical-or-to-and             ||  ->  &&
  equality-strict-to-loose-neg  === ->  !==
  inequality-to-equality        !== ->  ===
  return-true-to-false          return true;  ->  return false;
  return-false-to-true          return false; ->  return true;

STATUSES:
  killed     the suite failed, so a test detects this change
  survived   the suite passed, so no test detects it: a test gap
  timeout    the suite hung and its process group was killed
  error      the mutant could not be evaluated

EXIT CODES:
  0  the run completed and JSON was written without a selected gate failure
  1  a usage, parse, or baseline error occurred
  2  --fail-on-survivors found one or more surviving mutants; takes precedence over exit 3
  3  --fail-on-survivors found one or more timed-out or errored mutants and no survivors

A surviving mutant is a fact about your tests, not a proven bug.
";

async fn run(arguments: Vec<String>) -> Result<i32, String> {
    if arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "--help" | "-h" | "help"))
    {
        print!("{USAGE}");
        return Ok(0);
    }
    if arguments
        .iter()
        .any(|argument| argument.as_str() == "--version")
    {
        println!("{}", version_output());
        return Ok(0);
    }
    let options = parse_run_arguments(arguments)?;
    let plans = discover_source_files(&options.repository, &options.file)?
        .into_iter()
        .map(|file| prepare_file_run(&options, file))
        .collect::<Result<Vec<_>, _>>()?;
    if !options.skip_baseline {
        verify_baseline(&options).await?;
    }

    let mut file_runs = Vec::with_capacity(plans.len());
    for plan in plans {
        let configuration = RunnerConfig::new(
            &options.repository,
            &plan.file,
            format!("({}) 1>&2", options.test_command),
            Duration::from_millis(options.timeout_ms),
            options.concurrency,
        );
        let runner = Runner::new(configuration);
        let mut results = runner.run(&plan.mutants).await;
        results.sort_by_key(|result| (result.mutant.span_start, result.mutant.operator));
        file_runs.push(FileRun {
            file: plan.file,
            source: plan.source,
            results,
        });
    }

    if file_runs.len() == 1 {
        let file_run = &file_runs[0];
        write_json(&file_run.file, &file_run.source, &file_run.results)
            .map_err(|error| format!("failed to write JSON: {error}"))?;
    } else {
        write_multi_json(&file_runs).map_err(|error| format!("failed to write JSON: {error}"))?;
    }
    if let Some(path) = &options.sarif {
        if file_runs.len() == 1 {
            let file_run = &file_runs[0];
            write_sarif(path, &file_run.file, &file_run.source, &file_run.results)
                .map_err(|error| format!("failed to write SARIF: {error}"))?;
        } else {
            write_multi_sarif(path, &file_runs)
                .map_err(|error| format!("failed to write SARIF: {error}"))?;
        }
    }
    Ok(exit_code_for_results(
        options.fail_on_survivors,
        &file_runs
            .iter()
            .flat_map(|file_run| file_run.results.iter())
            .cloned()
            .collect::<Vec<_>>(),
    ))
}

fn version_output() -> String {
    format!("bughunter {}", env!("CARGO_PKG_VERSION"))
}

fn exit_code_for_results(fail_on_survivors: bool, results: &[MutantResult]) -> i32 {
    if !fail_on_survivors {
        return 0;
    }
    if results
        .iter()
        .any(|result| result.status == MutantStatus::Survived)
    {
        return 2;
    }
    if results
        .iter()
        .any(|result| matches!(result.status, MutantStatus::Timeout | MutantStatus::Error))
    {
        return 3;
    }
    0
}

struct RunOptions {
    repository: PathBuf,
    file: PathBuf,
    operators: Vec<Operator>,
    test_command: String,
    timeout_ms: u64,
    concurrency: usize,
    line_range: Option<LineRange>,
    sarif: Option<PathBuf>,
    skip_baseline: bool,
    fail_on_survivors: bool,
}

struct FilePlan {
    file: PathBuf,
    source: String,
    mutants: Vec<bughunter_engine::Mutant>,
}

struct FileRun {
    file: PathBuf,
    source: String,
    results: Vec<MutantResult>,
}

async fn verify_baseline(options: &RunOptions) -> Result<(), String> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("({}) 1>&2", options.test_command))
        .current_dir(&options.repository);
    configure_process_group(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run the baseline test command: {error}"))?;
    let process_group = child
        .id()
        .map(|identifier| identifier as i32)
        .ok_or_else(|| {
            "failed to run the baseline test command: spawned command has no process identifier"
                .to_owned()
        })?;

    match timeout(Duration::from_millis(options.timeout_ms), child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!(
            "baseline test command failed with {status} in {}; mutation results would be meaningless because every mutant would be reported killed. Fix the suite, or pass --skip-baseline to override",
            options.repository.display()
        )),
        Ok(Err(error)) => Err(format!("failed to run the baseline test command: {error}")),
        Err(_) => {
            let cleanup_detail = kill_process_group(process_group)
                .err()
                .map(|error| error.to_string());
            let wait_detail = child.wait().await.err().map(|error| error.to_string());
            Err(baseline_timeout_error(
                options,
                cleanup_detail,
                wait_detail,
            ))
        }
    }
}

fn configure_process_group(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn kill_process_group(process_group: i32) -> io::Result<()> {
    if unsafe { killpg(process_group, SIGNAL_KILL) } == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(NO_SUCH_PROCESS) {
            return Err(error);
        }
    }
    Ok(())
}

fn baseline_timeout_error(
    options: &RunOptions,
    cleanup_detail: Option<String>,
    wait_detail: Option<String>,
) -> String {
    let cleanup_suffix = match (cleanup_detail, wait_detail) {
        (None, None) => String::new(),
        (Some(cleanup_detail), None) => format!("; process-group cleanup failed: {cleanup_detail}"),
        (None, Some(wait_detail)) => format!("; child reap failed: {wait_detail}"),
        (Some(cleanup_detail), Some(wait_detail)) => format!(
            "; process-group cleanup failed: {cleanup_detail}; child reap failed: {wait_detail}"
        ),
    };
    format!(
        "baseline test command timed out after {} ms in {}; increase --timeout-ms or pass --skip-baseline to override{cleanup_suffix}",
        options.timeout_ms,
        options.repository.display()
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LineRange {
    start: u64,
    end: u64,
}

fn parse_run_arguments(arguments: Vec<String>) -> Result<RunOptions, String> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some("run") {
        return Err("expected the run command; try bughunter --help".to_owned());
    }

    let mut repository = None;
    let mut file = None;
    let mut operators = None;
    let mut test_command = None;
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    let mut concurrency = DEFAULT_CONCURRENCY;
    let mut line_range = None;
    let mut sarif = None;
    let mut json = false;
    let mut skip_baseline = false;
    let mut fail_on_survivors = false;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--repo" => repository = Some(PathBuf::from(next_value(&mut arguments, "--repo")?)),
            "--file" => file = Some(PathBuf::from(next_value(&mut arguments, "--file")?)),
            "--operators" => {
                operators = Some(parse_operators(&next_value(
                    &mut arguments,
                    "--operators",
                )?)?)
            }
            "--test" => test_command = Some(next_value(&mut arguments, "--test")?),
            "--timeout-ms" => {
                timeout_ms = parse_positive_u64(
                    &next_value(&mut arguments, "--timeout-ms")?,
                    "--timeout-ms",
                )?
            }
            "--concurrency" => {
                concurrency = parse_positive_usize(
                    &next_value(&mut arguments, "--concurrency")?,
                    "--concurrency",
                )?
            }
            "--line-range" => {
                line_range = Some(parse_line_range(&next_value(
                    &mut arguments,
                    "--line-range",
                )?)?)
            }
            "--json" => json = true,
            "--sarif" => sarif = Some(PathBuf::from(next_value(&mut arguments, "--sarif")?)),
            "--skip-baseline" => skip_baseline = true,
            "--fail-on-survivors" => fail_on_survivors = true,
            _ => return Err(format!("unknown argument {argument}")),
        }
    }

    if !json {
        return Err("--json is required".to_owned());
    }
    let repository = repository.ok_or_else(|| "--repo is required".to_owned())?;
    let file = file.ok_or_else(|| "--file is required".to_owned())?;
    if !is_relative_selector(&file) {
        return Err(
            "--file must be a non-empty relative path or glob without parent components".to_owned(),
        );
    }

    Ok(RunOptions {
        repository,
        file,
        operators: operators.ok_or_else(|| "--operators is required".to_owned())?,
        test_command: test_command.ok_or_else(|| "--test is required".to_owned())?,
        timeout_ms,
        concurrency,
        line_range,
        sarif,
        skip_baseline,
        fail_on_survivors,
    })
}

fn parse_line_range(value: &str) -> Result<LineRange, String> {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

    let Some((start, end)) = value.split_once('-') else {
        return Err(format!("invalid line range {value:?}; expected START-END"));
    };
    if start.is_empty()
        || end.is_empty()
        || !start.bytes().all(|byte| byte.is_ascii_digit())
        || !end.bytes().all(|byte| byte.is_ascii_digit())
        || end.contains('-')
    {
        return Err(format!("invalid line range {value:?}; expected START-END"));
    }

    let start = start.parse::<u64>().ok();
    let end = end.parse::<u64>().ok();
    let (Some(start), Some(end)) = (start, end) else {
        return Err(format!(
            "invalid line range {value:?}; expected 1 <= START <= END"
        ));
    };
    if start > MAX_SAFE_INTEGER || end > MAX_SAFE_INTEGER || start < 1 || start > end {
        return Err(format!(
            "invalid line range {value:?}; expected 1 <= START <= END"
        ));
    }

    Ok(LineRange { start, end })
}

fn select_line_range(
    mutants: Vec<bughunter_engine::Mutant>,
    line_range: Option<LineRange>,
) -> Vec<bughunter_engine::Mutant> {
    let Some(line_range) = line_range else {
        return mutants;
    };
    mutants
        .into_iter()
        .filter(|mutant| {
            let line = u64::from(mutant.line);
            line >= line_range.start && line <= line_range.end
        })
        .collect()
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    arguments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{option} requires a non-empty value"))
}

fn parse_operators(value: &str) -> Result<Vec<Operator>, String> {
    value
        .split(',')
        .map(|id| Operator::from_id(id).ok_or_else(|| format!("unknown operator {id}")))
        .collect()
}

fn parse_positive_u64(value: &str, option: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{option} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{option} must be a positive integer"));
    }
    Ok(parsed)
}

fn parse_positive_usize(value: &str, option: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{option} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{option} must be a positive integer"));
    }
    Ok(parsed)
}

fn is_relative_selector(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.is_relative()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

fn prepare_file_run(options: &RunOptions, file: PathBuf) -> Result<FilePlan, String> {
    let source_path = options.repository.join(&file);
    let source = fs::read_to_string(&source_path)
        .map_err(|error| format!("failed to read {}: {error}", source_path.display()))?;
    let mutants = select_line_range(
        try_mutants(&source, &file.to_string_lossy(), &options.operators)
            .map_err(|failure| failure.to_string())?,
        options.line_range,
    );
    Ok(FilePlan {
        file,
        source,
        mutants,
    })
}

fn discover_source_files(repository: &Path, selector: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    if has_glob_pattern(selector) {
        collect_source_files(repository, repository, &mut files)?;
        files.retain(|file| glob_matches(selector, file));
    } else {
        let selected_path = repository.join(selector);
        let metadata = fs::metadata(&selected_path).map_err(|error| {
            format!(
                "failed to inspect --file {}: {error}",
                selected_path.display()
            )
        })?;
        if metadata.is_file() {
            if !is_test_file(selector) && !has_ignored_directory_component(selector) {
                files.push(selector.to_path_buf());
            }
        } else if metadata.is_dir() {
            collect_source_files(&selected_path, repository, &mut files)?;
        } else {
            return Err(format!(
                "--file {} must be a regular file, directory, or glob",
                selector.display()
            ));
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "no TypeScript source files matched --file {}",
            selector.display()
        ));
    }
    Ok(files)
}

fn collect_source_files(
    directory: &Path,
    repository: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if is_ignored_directory(directory) {
        return Ok(());
    }
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("failed to read directory {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read directory entry in {}: {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "failed to inspect directory entry {}: {error}",
                path.display()
            )
        })?;
        if file_type.is_dir() {
            if !is_ignored_directory(&path) {
                collect_source_files(&path, repository, files)?;
            }
        } else if file_type.is_file() && is_source_file(&path) {
            let relative_path = path.strip_prefix(repository).map_err(|error| {
                format!(
                    "failed to make discovered file {} relative to {}: {error}",
                    path.display(),
                    repository.display()
                )
            })?;
            files.push(relative_path.to_path_buf());
        }
    }
    Ok(())
}

fn is_ignored_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_ignored_directory_name)
}

fn has_ignored_directory_component(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::Normal(name) if name.to_str().is_some_and(is_ignored_directory_name))
    })
}

fn is_ignored_directory_name(name: &str) -> bool {
    matches!(name, "node_modules" | "dist" | "build" | ".git")
}

fn is_source_file(path: &Path) -> bool {
    let extension = path.extension().and_then(|value| value.to_str());
    matches!(extension, Some("ts" | "tsx")) && !is_test_file(path)
}

fn is_test_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(name)
            if name.ends_with(".test.ts")
                || name.ends_with(".spec.ts")
                || name.ends_with(".test.tsx")
                || name.ends_with(".spec.tsx")
    )
}

fn has_glob_pattern(path: &Path) -> bool {
    path.to_string_lossy()
        .chars()
        .any(|character| matches!(character, '*' | '?' | '['))
}

fn glob_matches(pattern: &Path, candidate: &Path) -> bool {
    let pattern = pattern.to_string_lossy();
    let pattern_segments = pattern
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();
    let candidate_segments = candidate
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => segment.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    glob_segments_match(&pattern_segments, &candidate_segments)
}

fn glob_segments_match(pattern: &[&str], candidate: &[&str]) -> bool {
    match pattern.split_first() {
        None => candidate.is_empty(),
        Some((&"**", remaining_pattern)) => {
            glob_segments_match(remaining_pattern, candidate)
                || (!candidate.is_empty() && glob_segments_match(pattern, &candidate[1..]))
        }
        Some((segment, remaining_pattern)) => {
            candidate
                .split_first()
                .is_some_and(|(candidate_segment, remaining_candidate)| {
                    glob_segment_matches(segment, candidate_segment)
                        && glob_segments_match(remaining_pattern, remaining_candidate)
                })
        }
    }
}

fn glob_segment_matches(pattern: &str, candidate: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let candidate = candidate.chars().collect::<Vec<_>>();
    glob_characters_match(&pattern, &candidate)
}

fn glob_characters_match(pattern: &[char], candidate: &[char]) -> bool {
    match pattern.split_first() {
        None => candidate.is_empty(),
        Some(('*', remaining_pattern)) => {
            glob_characters_match(remaining_pattern, candidate)
                || (!candidate.is_empty() && glob_characters_match(pattern, &candidate[1..]))
        }
        Some(('?', remaining_pattern)) => {
            candidate
                .split_first()
                .is_some_and(|(_, remaining_candidate)| {
                    glob_characters_match(remaining_pattern, remaining_candidate)
                })
        }
        Some(('[', remaining_pattern)) => {
            match remaining_pattern
                .iter()
                .position(|character| *character == ']')
            {
                Some(closing_index) => candidate.split_first().is_some_and(
                    |(candidate_character, remaining_candidate)| {
                        glob_character_class_matches(
                            &remaining_pattern[..closing_index],
                            *candidate_character,
                        ) && glob_characters_match(
                            &remaining_pattern[closing_index + 1..],
                            remaining_candidate,
                        )
                    },
                ),
                None => candidate.split_first().is_some_and(
                    |(candidate_character, remaining_candidate)| {
                        *candidate_character == '['
                            && glob_characters_match(remaining_pattern, remaining_candidate)
                    },
                ),
            }
        }
        Some((character, remaining_pattern)) => {
            candidate
                .split_first()
                .is_some_and(|(candidate_character, remaining_candidate)| {
                    character == candidate_character
                        && glob_characters_match(remaining_pattern, remaining_candidate)
                })
        }
    }
}

fn glob_character_class_matches(class: &[char], candidate: char) -> bool {
    let (is_negated, class) = match class.split_first() {
        Some(('!' | '^', remaining_class)) => (true, remaining_class),
        _ => (false, class),
    };
    let mut index = 0;
    let mut matches = false;
    while index < class.len() {
        if index + 2 < class.len() && class[index + 1] == '-' {
            matches |= class[index] <= candidate && candidate <= class[index + 2];
            index += 3;
        } else {
            matches |= class[index] == candidate;
            index += 1;
        }
    }
    matches != is_negated
}

fn write_json(file: &Path, source: &str, results: &[MutantResult]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    write_json_to(file, source, results, &mut output)
}

fn write_multi_json(file_runs: &[FileRun]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    write_multi_json_to(file_runs, &mut output)
}

fn write_multi_json_to<W: Write>(file_runs: &[FileRun], output: &mut W) -> io::Result<()> {
    let summary = ResultSummary::from_file_runs(file_runs);
    write!(
        output,
        "{{\"schema_version\":2,\"total\":{},\"killed\":{},\"survived\":{},\"timeout\":{},\"error\":{},\"evaluated\":{},\"score\":",
        summary.total,
        summary.killed,
        summary.survived,
        summary.timeout,
        summary.error,
        summary.evaluated,
    )?;
    match summary.score {
        Some(score) => write!(output, "{score}")?,
        None => write!(output, "null")?,
    }
    write!(output, ",\"files\":[")?;
    for (index, file_run) in file_runs.iter().enumerate() {
        if index > 0 {
            write!(output, ",")?;
        }
        write_json_file_entry(file_run, output)?;
    }
    writeln!(output, "]}}")
}

fn write_json_file_entry<W: Write>(file_run: &FileRun, output: &mut W) -> io::Result<()> {
    let summary = ResultSummary::from_results(&file_run.results);
    let generated = try_mutants(
        &file_run.source,
        &file_run.file.to_string_lossy(),
        &Operator::ALL,
    )
    .unwrap_or_default();
    write!(output, "{{\"file\":")?;
    write_json_string(output, &file_run.file.to_string_lossy())?;
    write!(
        output,
        ",\"total\":{},\"killed\":{},\"survived\":{},\"timeout\":{},\"error\":{},\"evaluated\":{},\"score\":",
        summary.total,
        summary.killed,
        summary.survived,
        summary.timeout,
        summary.error,
        summary.evaluated,
    )?;
    match summary.score {
        Some(score) => write!(output, "{score}")?,
        None => write!(output, "null")?,
    }
    write!(output, ",\"mutants\":[")?;
    for (index, result) in file_run.results.iter().enumerate() {
        if index > 0 {
            write!(output, ",")?;
        }
        write!(output, "{{\"id\":")?;
        let occurrence_index = operator_occurrence_index(&generated, &result.mutant);
        write_json_string(
            output,
            &stable_mutant_id(
                &file_run.file,
                &file_run.source,
                &result.mutant,
                occurrence_index,
            ),
        )?;
        write!(output, ",\"line\":{},\"operator\":", result.mutant.line)?;
        write_json_string(output, result.mutant.operator.id())?;
        write!(output, ",\"status\":")?;
        write_json_string(output, status_id(result.status))?;
        if let Some(detail) = &result.detail {
            write!(output, ",\"detail\":")?;
            write_json_string(output, detail)?;
        }
        write!(output, "}}")?;
    }
    write!(output, "]}}")
}

fn write_json_to<W: Write>(
    file: &Path,
    source: &str,
    results: &[MutantResult],
    output: &mut W,
) -> io::Result<()> {
    let summary = ResultSummary::from_results(results);
    let generated =
        try_mutants(source, &file.to_string_lossy(), &Operator::ALL).unwrap_or_default();
    write!(
        output,
        "{{\"schema_version\":2,\"total\":{},\"killed\":{},\"survived\":{},\"timeout\":{},\"error\":{},\"evaluated\":{},\"score\":",
        summary.total,
        summary.killed,
        summary.survived,
        summary.timeout,
        summary.error,
        summary.evaluated,
    )?;
    match summary.score {
        Some(score) => write!(output, "{score}")?,
        None => write!(output, "null")?,
    }
    write!(output, ",\"mutants\":[")?;
    for (index, result) in results.iter().enumerate() {
        if index > 0 {
            write!(output, ",")?;
        }
        write!(output, "{{\"id\":")?;
        let occurrence_index = operator_occurrence_index(&generated, &result.mutant);
        write_json_string(
            output,
            &stable_mutant_id(file, source, &result.mutant, occurrence_index),
        )?;
        write!(output, ",\"line\":{},\"operator\":", result.mutant.line)?;
        write_json_string(output, result.mutant.operator.id())?;
        write!(output, ",\"status\":")?;
        write_json_string(output, status_id(result.status))?;
        if let Some(detail) = &result.detail {
            write!(output, ",\"detail\":")?;
            write_json_string(output, detail)?;
        }
        write!(output, "}}")?;
    }
    writeln!(output, "]}}")
}

fn write_sarif(path: &Path, file: &Path, source: &str, results: &[MutantResult]) -> io::Result<()> {
    let mut output = fs::File::create(path)?;
    write_sarif_to(file, source, results, &mut output)
}

fn write_multi_sarif(path: &Path, file_runs: &[FileRun]) -> io::Result<()> {
    let mut output = fs::File::create(path)?;
    write_multi_sarif_to(file_runs, &mut output)
}

fn write_multi_sarif_to<W: Write>(file_runs: &[FileRun], output: &mut W) -> io::Result<()> {
    let mut operators = Vec::new();
    for file_run in file_runs {
        for operator in surviving_operators(&file_run.results) {
            if !operators.contains(&operator) {
                operators.push(operator);
            }
        }
    }

    output.write_all(b"{\"$schema\":")?;
    write_json_string(output, "https://json.schemastore.org/sarif-2.1.0.json")?;
    output.write_all(
        b",\"version\":\"2.1.0\",\"runs\":[{\"tool\":{\"driver\":{\"name\":\"bughunter\",\"informationUri\":\"https://github.com/acoyfellow/bughunter\",\"semanticVersion\":",
    )?;
    write_json_string(output, env!("CARGO_PKG_VERSION"))?;
    output.write_all(b",\"rules\":[")?;
    for (index, operator) in operators.iter().enumerate() {
        if index > 0 {
            output.write_all(b",")?;
        }
        output.write_all(b"{\"id\":")?;
        write_json_string(output, operator.id())?;
        output.write_all(b"}")?;
    }
    output.write_all(b"]}},\"results\":[")?;
    let mut result_count = 0;
    for file_run in file_runs {
        let generated = try_mutants(
            &file_run.source,
            &file_run.file.to_string_lossy(),
            &Operator::ALL,
        )
        .unwrap_or_default();
        for result in &file_run.results {
            if result.status != MutantStatus::Survived {
                continue;
            }
            if result_count > 0 {
                output.write_all(b",")?;
            }
            result_count += 1;
            let occurrence_index = operator_occurrence_index(&generated, &result.mutant);
            let mutant_id = stable_mutant_id(
                &file_run.file,
                &file_run.source,
                &result.mutant,
                occurrence_index,
            );
            output.write_all(b"{\"ruleId\":")?;
            write_json_string(output, result.mutant.operator.id())?;
            output.write_all(b",\"message\":{\"text\":\"Surviving mutant\"},\"locations\":[{\"physicalLocation\":{\"artifactLocation\":{\"uri\":")?;
            write_json_string(output, &file_run.file.to_string_lossy())?;
            output.write_all(b"},\"region\":")?;
            write!(
                output,
                "{{\"startLine\":{},\"startColumn\":{}}}",
                result.mutant.line, result.mutant.column
            )?;
            output.write_all(b"}}],\"partialFingerprints\":{\"bughunterMutantId/v1\":")?;
            write_json_string(output, &mutant_id)?;
            output.write_all(b"}}")?;
        }
    }
    output.write_all(b"]}]}")
}

fn write_sarif_to<W: Write>(
    file: &Path,
    source: &str,
    results: &[MutantResult],
    output: &mut W,
) -> io::Result<()> {
    let generated =
        try_mutants(source, &file.to_string_lossy(), &Operator::ALL).unwrap_or_default();
    let operators = surviving_operators(results);

    output.write_all(b"{\"$schema\":")?;
    write_json_string(output, "https://json.schemastore.org/sarif-2.1.0.json")?;
    output.write_all(
        b",\"version\":\"2.1.0\",\"runs\":[{\"tool\":{\"driver\":{\"name\":\"bughunter\",\"informationUri\":\"https://github.com/acoyfellow/bughunter\",\"semanticVersion\":",
    )?;
    write_json_string(output, env!("CARGO_PKG_VERSION"))?;
    output.write_all(b",\"rules\":[")?;
    for (index, operator) in operators.iter().enumerate() {
        if index > 0 {
            output.write_all(b",")?;
        }
        output.write_all(b"{\"id\":")?;
        write_json_string(output, operator.id())?;
        output.write_all(b"}")?;
    }
    output.write_all(b"]}},\"results\":[")?;
    let mut result_count = 0;
    for result in results {
        if result.status != MutantStatus::Survived {
            continue;
        }
        if result_count > 0 {
            output.write_all(b",")?;
        }
        result_count += 1;
        let occurrence_index = operator_occurrence_index(&generated, &result.mutant);
        let mutant_id = stable_mutant_id(file, source, &result.mutant, occurrence_index);
        output.write_all(b"{\"ruleId\":")?;
        write_json_string(output, result.mutant.operator.id())?;
        output.write_all(b",\"message\":{\"text\":\"Surviving mutant\"},\"locations\":[{\"physicalLocation\":{\"artifactLocation\":{\"uri\":")?;
        write_json_string(output, &file.to_string_lossy())?;
        output.write_all(b"},\"region\":")?;
        write!(
            output,
            "{{\"startLine\":{},\"startColumn\":{}}}",
            result.mutant.line, result.mutant.column
        )?;
        output.write_all(b"}}],\"partialFingerprints\":{\"bughunterMutantId/v1\":")?;
        write_json_string(output, &mutant_id)?;
        output.write_all(b"}}")?;
    }
    output.write_all(b"]}]}")
}

fn surviving_operators(results: &[MutantResult]) -> Vec<Operator> {
    let mut operators = Vec::new();
    for result in results {
        if result.status == MutantStatus::Survived && !operators.contains(&result.mutant.operator) {
            operators.push(result.mutant.operator);
        }
    }
    operators
}

#[derive(Debug, PartialEq)]
struct ResultSummary {
    total: usize,
    killed: usize,
    survived: usize,
    timeout: usize,
    error: usize,
    evaluated: usize,
    score: Option<f64>,
}

impl ResultSummary {
    fn from_results(results: &[MutantResult]) -> Self {
        let mut summary = Self {
            total: results.len(),
            killed: 0,
            survived: 0,
            timeout: 0,
            error: 0,
            evaluated: 0,
            score: None,
        };
        for result in results {
            match result.status {
                MutantStatus::Killed => summary.killed += 1,
                MutantStatus::Survived => summary.survived += 1,
                MutantStatus::Timeout => summary.timeout += 1,
                MutantStatus::Error => summary.error += 1,
            }
        }
        summary.evaluated = summary.killed + summary.survived;
        summary.score =
            (summary.evaluated != 0).then(|| summary.killed as f64 / summary.evaluated as f64);
        summary
    }

    fn from_file_runs(file_runs: &[FileRun]) -> Self {
        let mut summary = Self {
            total: 0,
            killed: 0,
            survived: 0,
            timeout: 0,
            error: 0,
            evaluated: 0,
            score: None,
        };
        for file_run in file_runs {
            let file_summary = Self::from_results(&file_run.results);
            summary.total += file_summary.total;
            summary.killed += file_summary.killed;
            summary.survived += file_summary.survived;
            summary.timeout += file_summary.timeout;
            summary.error += file_summary.error;
            summary.evaluated += file_summary.evaluated;
        }
        summary.score =
            (summary.evaluated != 0).then(|| summary.killed as f64 / summary.evaluated as f64);
        summary
    }
}

fn stable_mutant_id(
    file: &Path,
    source: &str,
    mutant: &bughunter_engine::Mutant,
    occurrence_index: u64,
) -> String {
    let original = source
        .get(mutant.span_start as usize..mutant.span_end as usize)
        .unwrap_or_default();
    let occurrence_index = occurrence_index.to_le_bytes();
    let hash = stable_hash(&[
        file.to_string_lossy().as_bytes(),
        mutant.operator.id().as_bytes(),
        original.as_bytes(),
        mutant.replacement.as_bytes(),
        &occurrence_index,
    ]);
    format!("{hash:016x}")
}

fn operator_occurrence_index(
    generated: &[bughunter_engine::Mutant],
    mutant: &bughunter_engine::Mutant,
) -> u64 {
    generated
        .iter()
        .filter(|candidate| candidate.operator == mutant.operator)
        .position(|candidate| candidate == mutant)
        .map_or(0, |index| index as u64 + 1)
}

fn stable_hash(fields: &[&[u8]]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x00000100000001b3;

    let mut hash = OFFSET_BASIS;
    for field in fields {
        for byte in (field.len() as u64)
            .to_le_bytes()
            .iter()
            .chain(field.iter())
        {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

fn write_json_string<W: Write>(output: &mut W, value: &str) -> io::Result<()> {
    output.write_all(b"\"")?;
    for character in value.chars() {
        if let Some(escape) = json_escape(character) {
            output.write_all(escape)?;
            continue;
        }
        if character <= '\u{001f}' {
            write!(output, r"\u{:04x}", character as u32)?;
        } else {
            write!(output, "{character}")?;
        }
    }
    output.write_all(b"\"")
}

fn json_escape(character: char) -> Option<&'static [u8]> {
    match character {
        '"' => Some(&[92, 34]),
        '\u{005c}' => Some(&[92, 92]),
        '\u{0008}' => Some(&[92, 98]),
        '\u{000c}' => Some(&[92, 102]),
        '\n' => Some(&[92, 110]),
        '\r' => Some(&[92, 114]),
        '\t' => Some(&[92, 116]),
        _ => None,
    }
}

fn status_id(status: MutantStatus) -> &'static str {
    match status {
        MutantStatus::Killed => "killed",
        MutantStatus::Survived => "survived",
        MutantStatus::Timeout => "timeout",
        MutantStatus::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        discover_source_files, exit_code_for_results, parse_line_range, parse_run_arguments,
        select_line_range, stable_mutant_id, verify_baseline, version_output, write_json_to,
        write_multi_sarif_to, write_sarif_to, FileRun, LineRange, ResultSummary, RunOptions,
    };
    use bughunter_engine::{mutants, Mutant, Operator};
    use bughunter_runner::{MutantResult, MutantStatus};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn unknown_operator_is_an_error() {
        let result = parse_run_arguments(vec![
            "run".to_owned(),
            "--repo".to_owned(),
            "fixture".to_owned(),
            "--file".to_owned(),
            "source.ts".to_owned(),
            "--operators".to_owned(),
            "not-an-operator".to_owned(),
            "--test".to_owned(),
            "true".to_owned(),
            "--json".to_owned(),
        ]);
        assert!(matches!(
            result,
            Err(message) if message == "unknown operator not-an-operator"
        ));
    }

    #[test]
    fn line_range_keeps_both_inclusive_boundaries() {
        let source = "\nconst lower = left && right;\nconst upper = left && right;\n";
        let selected = select_line_range(
            mutants(source, "fixture.ts", &[Operator::LogicalAndToOr]),
            Some(LineRange { start: 2, end: 3 }),
        );
        assert_eq!(
            selected
                .iter()
                .map(|mutant| mutant.line)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[test]
    fn line_range_drops_mutants_outside_the_boundaries() {
        let source = "const before = left && right;\nconst inside = left && right;\nconst after = left && right;\n";
        let selected = select_line_range(
            mutants(source, "fixture.ts", &[Operator::LogicalAndToOr]),
            Some(LineRange { start: 2, end: 2 }),
        );
        assert_eq!(
            selected
                .iter()
                .map(|mutant| mutant.line)
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn usage_lists_every_operator_and_status() {
        for operator in Operator::ALL {
            assert!(super::USAGE.contains(operator.id()), "{}", operator.id());
        }
        for status in ["killed", "survived", "timeout", "error"] {
            assert!(super::USAGE.contains(status), "{status}");
        }
    }

    #[test]
    fn skip_baseline_defaults_to_off_and_is_opt_in() {
        let base = vec![
            "run".to_owned(),
            "--repo".to_owned(),
            "fixture".to_owned(),
            "--file".to_owned(),
            "source.ts".to_owned(),
            "--operators".to_owned(),
            "logical-and-to-or".to_owned(),
            "--test".to_owned(),
            "true".to_owned(),
            "--json".to_owned(),
        ];
        assert!(!parse_run_arguments(base.clone()).unwrap().skip_baseline);
        let mut opted_in = base;
        opted_in.push("--skip-baseline".to_owned());
        assert!(parse_run_arguments(opted_in).unwrap().skip_baseline);
    }

    #[test]
    fn unparseable_source_is_an_error_not_an_empty_result() {
        let broken = "export function f(a: number) {\n  if (a > 1) return true;\n";
        let failure = bughunter_engine::try_mutants(broken, "broken.ts", &Operator::ALL)
            .expect_err("a truncated file must not parse");
        assert!(failure.to_string().contains("failed to parse broken.ts"));
        assert!(!failure.diagnostics.is_empty());
    }

    #[test]
    fn malformed_line_ranges_are_errors() {
        for value in ["not-a-range", "0-1", "4-3"] {
            assert!(parse_line_range(value).is_err(), "{value}");
        }
    }

    #[test]
    fn version_output_uses_the_package_version() {
        assert_eq!(
            version_output(),
            format!("bughunter {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn json_serialization_includes_details_only_when_present() {
        let results = [
            result(
                MutantStatus::Error,
                Some("could not execute: \"missing\"\n"),
            ),
            result(MutantStatus::Killed, None),
        ];
        let mut output = Vec::new();

        write_json_to(Path::new("fixture.ts"), "&&", &results, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("{\"schema_version\":2,"));
        assert!(output.contains("\"id\":\""));
        assert!(output.contains("\"detail\":\"could not execute: \\\"missing\\\"\\n\""));
        assert_eq!(output.matches("\"detail\":").count(), 1);
    }

    #[test]
    fn result_summary_reports_counts_and_score_for_mixed_statuses() {
        let results = [
            result(MutantStatus::Killed, None),
            result(MutantStatus::Killed, None),
            result(MutantStatus::Survived, None),
            result(MutantStatus::Timeout, None),
            result(MutantStatus::Error, Some("disk full")),
        ];
        let summary = ResultSummary::from_results(&results);

        assert_eq!(summary.total, 5);
        assert_eq!(summary.killed, 2);
        assert_eq!(summary.survived, 1);
        assert_eq!(summary.timeout, 1);
        assert_eq!(summary.error, 1);
        assert_eq!(summary.evaluated, 3);
        assert_eq!(summary.score, Some(2.0 / 3.0));
        assert_eq!(
            summary.total,
            summary.killed + summary.survived + summary.timeout + summary.error
        );
        let mut output = Vec::new();
        write_json_to(Path::new("fixture.ts"), "&&", &results, &mut output).unwrap();
        assert!(String::from_utf8(output).unwrap().starts_with(
            "{\"schema_version\":2,\"total\":5,\"killed\":2,\"survived\":1,\"timeout\":1,\"error\":1,\"evaluated\":3,\"score\":0.6666666666666666,"
        ));
    }

    #[test]
    fn result_summary_uses_null_score_when_no_mutants_were_evaluated() {
        let results = [
            result(MutantStatus::Timeout, None),
            result(MutantStatus::Error, Some("disk full")),
        ];
        let summary = ResultSummary::from_results(&results);
        let mut output = Vec::new();

        write_json_to(Path::new("fixture.ts"), "&&", &results, &mut output).unwrap();

        assert_eq!(summary.evaluated, 0);
        assert_eq!(summary.score, None);
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("\"evaluated\":0,\"score\":null"));
    }

    #[test]
    fn sarif_serialization_emits_one_result_for_a_surviving_mutant() {
        let source = "const ready = left && right;\n";
        let mutant = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr]).remove(0);
        let results = [MutantResult {
            mutant,
            status: MutantStatus::Survived,
            detail: None,
        }];
        let mut output = Vec::new();

        write_sarif_to(Path::new("src/ready.ts"), source, &results, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("\"partialFingerprints\":").count(), 1);
        assert!(output.contains("\"ruleId\":\"logical-and-to-or\""));
        assert!(output.contains("\"bughunterMutantId/v1\":\""));
    }

    #[test]
    fn sarif_serialization_omits_killed_mutants() {
        let source = "const ready = left && right;\n";
        let mutant = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr]).remove(0);
        let results = [MutantResult {
            mutant,
            status: MutantStatus::Killed,
            detail: None,
        }];
        let mut output = Vec::new();

        write_sarif_to(Path::new("src/ready.ts"), source, &results, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\"version\":\"2.1.0\""));
        assert!(output.contains("\"$schema\":\""));
        assert!(output.contains("\"rules\":[]"));
        assert!(output.contains("\"results\":[]"));
    }

    #[test]
    fn same_operator_survivors_have_distinct_sarif_fingerprints() {
        let source = "const first = left && right;\nconst second = top && bottom;\n";
        let results = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr])
            .into_iter()
            .map(|mutant| MutantResult {
                mutant,
                status: MutantStatus::Survived,
                detail: None,
            })
            .collect::<Vec<_>>();
        let mut output = Vec::new();

        write_sarif_to(Path::new("src/ready.ts"), source, &results, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        let fingerprints = output
            .split("\"bughunterMutantId/v1\":\"")
            .skip(1)
            .map(|entry| entry.split('\"').next().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(fingerprints.len(), 2);
        assert_ne!(fingerprints[0], fingerprints[1]);
    }

    #[test]
    fn sarif_artifact_uris_are_relative() {
        let source = "const ready = left && right;\n";
        let mutant = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr]).remove(0);
        let results = [MutantResult {
            mutant,
            status: MutantStatus::Survived,
            detail: None,
        }];
        let mut output = Vec::new();

        write_sarif_to(Path::new("src/ready.ts"), source, &results, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\"uri\":\"src/ready.ts\""));
        assert!(!output.contains("\"uri\":\"/"));
        assert!(!output.contains("Users"));
    }

    #[test]
    fn sarif_and_json_options_compose() {
        let result = parse_run_arguments(vec![
            "run".to_owned(),
            "--repo".to_owned(),
            "fixture".to_owned(),
            "--file".to_owned(),
            "source.ts".to_owned(),
            "--operators".to_owned(),
            "logical-and-to-or".to_owned(),
            "--test".to_owned(),
            "true".to_owned(),
            "--json".to_owned(),
            "--sarif".to_owned(),
            "results.sarif".to_owned(),
        ])
        .unwrap();

        assert_eq!(result.sarif, Some(PathBuf::from("results.sarif")));
    }

    #[test]
    fn same_operator_mutants_have_distinct_json_ids() {
        let source = "const first = left === right;\nconst second = top === bottom;\n";
        let results = mutants(
            source,
            "src/ready.ts",
            &[Operator::EqualityStrictToLooseNeg],
        )
        .into_iter()
        .map(|mutant| MutantResult {
            mutant,
            status: MutantStatus::Killed,
            detail: None,
        })
        .collect::<Vec<_>>();
        let mut output = Vec::new();

        write_json_to(Path::new("src/ready.ts"), source, &results, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        let ids = output
            .split("\"id\":\"")
            .skip(1)
            .map(|entry| entry.split('\"').next().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn stable_mutant_id_survives_a_line_shift() {
        let source = "const ready = left && right;\n";
        let shifted_source = "\n\nconst ready = left && right;\n";
        let original = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr]);
        let shifted = mutants(shifted_source, "src/ready.ts", &[Operator::LogicalAndToOr]);

        assert_ne!(original[0].line, shifted[0].line);
        assert_eq!(
            stable_mutant_id(Path::new("src/ready.ts"), source, &original[0], 1),
            stable_mutant_id(Path::new("src/ready.ts"), shifted_source, &shifted[0], 1)
        );
    }

    #[test]
    fn different_operators_at_the_same_location_have_different_ids() {
        let source = "const ready = left && right;\n";
        let original = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr]);
        let same_location_different_operator = Mutant {
            operator: Operator::LogicalOrToAnd,
            replacement: "&&".to_owned(),
            ..original[0].clone()
        };

        assert_eq!(
            original[0].span_start,
            same_location_different_operator.span_start
        );
        assert_eq!(
            original[0].span_end,
            same_location_different_operator.span_end
        );
        assert_ne!(
            stable_mutant_id(Path::new("src/ready.ts"), source, &original[0], 1),
            stable_mutant_id(
                Path::new("src/ready.ts"),
                source,
                &same_location_different_operator,
                1
            )
        );
    }

    #[test]
    fn survivors_with_the_flag_exit_two() {
        assert_eq!(
            exit_code_for_results(true, &[result(MutantStatus::Survived, None)]),
            2
        );
    }

    #[test]
    fn survivors_without_the_flag_exit_zero() {
        assert_eq!(
            exit_code_for_results(false, &[result(MutantStatus::Survived, None)]),
            0
        );
    }

    #[test]
    fn all_errors_with_the_flag_exit_three() {
        assert_eq!(
            exit_code_for_results(
                true,
                &[
                    result(MutantStatus::Error, Some("command unavailable")),
                    result(MutantStatus::Error, Some("disk full")),
                ],
            ),
            3
        );
    }

    #[test]
    fn all_timeouts_with_the_flag_exit_three() {
        assert_eq!(
            exit_code_for_results(
                true,
                &[
                    result(MutantStatus::Timeout, None),
                    result(MutantStatus::Timeout, None),
                ],
            ),
            3
        );
    }

    #[test]
    fn survivor_takes_precedence_over_an_error() {
        assert_eq!(
            exit_code_for_results(
                true,
                &[
                    result(MutantStatus::Survived, None),
                    result(MutantStatus::Error, Some("command unavailable")),
                ],
            ),
            2
        );
    }

    #[test]
    fn all_errors_without_the_flag_exit_zero() {
        assert_eq!(
            exit_code_for_results(
                false,
                &[result(MutantStatus::Error, Some("command unavailable"))],
            ),
            0
        );
    }

    #[test]
    fn no_survivors_with_the_flag_exit_zero() {
        assert_eq!(
            exit_code_for_results(true, &[result(MutantStatus::Killed, None)]),
            0
        );
    }

    #[test]
    fn discovery_skips_test_files_and_ignored_directories() {
        let fixture = crawl_fixture_directory();
        write_crawl_fixture_file(&fixture, "src/keep.ts");
        write_crawl_fixture_file(&fixture, "src/view.tsx");
        write_crawl_fixture_file(&fixture, "src/keep.test.ts");
        write_crawl_fixture_file(&fixture, "src/keep.spec.ts");
        write_crawl_fixture_file(&fixture, "src/node_modules/dependency.ts");
        write_crawl_fixture_file(&fixture, "src/dist/output.ts");
        write_crawl_fixture_file(&fixture, "src/build/output.ts");

        let discovered = discover_source_files(&fixture, Path::new("src")).unwrap();

        fs::remove_dir_all(&fixture).unwrap();
        assert_eq!(
            discovered,
            vec![PathBuf::from("src/keep.ts"), PathBuf::from("src/view.tsx")]
        );
    }

    #[test]
    fn discovery_is_deterministically_ordered() {
        let fixture = crawl_fixture_directory();
        write_crawl_fixture_file(&fixture, "src/zeta.ts");
        write_crawl_fixture_file(&fixture, "src/alpha.ts");
        write_crawl_fixture_file(&fixture, "src/nested/beta.ts");

        let first = discover_source_files(&fixture, Path::new("src")).unwrap();
        let second = discover_source_files(&fixture, Path::new("src/**/*.ts")).unwrap();

        fs::remove_dir_all(&fixture).unwrap();
        let expected = vec![
            PathBuf::from("src/alpha.ts"),
            PathBuf::from("src/nested/beta.ts"),
            PathBuf::from("src/zeta.ts"),
        ];
        assert_eq!(first, expected);
        assert_eq!(second, expected);
    }

    #[test]
    fn glob_with_zero_matches_is_an_error() {
        let fixture = crawl_fixture_directory();
        write_crawl_fixture_file(&fixture, "src/keep.ts");

        let error = discover_source_files(&fixture, Path::new("src/*.tsx")).unwrap_err();

        fs::remove_dir_all(&fixture).unwrap();
        assert_eq!(error, "no TypeScript source files matched --file src/*.tsx");
    }

    #[test]
    fn per_file_roll_up_counts_sum_to_the_overall_totals() {
        let file_runs = vec![
            FileRun {
                file: PathBuf::from("src/first.ts"),
                source: "const first = left && right;\n".to_owned(),
                results: vec![
                    result(MutantStatus::Killed, None),
                    result(MutantStatus::Survived, None),
                ],
            },
            FileRun {
                file: PathBuf::from("src/second.ts"),
                source: "const second = left && right;\n".to_owned(),
                results: vec![
                    result(MutantStatus::Timeout, None),
                    result(MutantStatus::Error, Some("disk full")),
                ],
            },
        ];
        let overall = ResultSummary::from_file_runs(&file_runs);
        let per_file = file_runs
            .iter()
            .map(|file_run| ResultSummary::from_results(&file_run.results))
            .collect::<Vec<_>>();

        assert_eq!(
            overall.total,
            per_file.iter().map(|summary| summary.total).sum::<usize>()
        );
        assert_eq!(
            overall.killed,
            per_file.iter().map(|summary| summary.killed).sum::<usize>()
        );
        assert_eq!(
            overall.survived,
            per_file
                .iter()
                .map(|summary| summary.survived)
                .sum::<usize>()
        );
        assert_eq!(
            overall.timeout,
            per_file
                .iter()
                .map(|summary| summary.timeout)
                .sum::<usize>()
        );
        assert_eq!(
            overall.error,
            per_file.iter().map(|summary| summary.error).sum::<usize>()
        );
        assert_eq!(
            overall.evaluated,
            per_file
                .iter()
                .map(|summary| summary.evaluated)
                .sum::<usize>()
        );
    }

    #[test]
    fn multi_file_sarif_uses_relative_artifact_uris() {
        let first_source = "const first = left && right;\n";
        let second_source = "const second = top && bottom;\n";
        let first_mutant =
            mutants(first_source, "src/first.ts", &[Operator::LogicalAndToOr]).remove(0);
        let second_mutant =
            mutants(second_source, "src/second.ts", &[Operator::LogicalAndToOr]).remove(0);
        let file_runs = vec![
            FileRun {
                file: PathBuf::from("src/first.ts"),
                source: first_source.to_owned(),
                results: vec![MutantResult {
                    mutant: first_mutant,
                    status: MutantStatus::Survived,
                    detail: None,
                }],
            },
            FileRun {
                file: PathBuf::from("src/second.ts"),
                source: second_source.to_owned(),
                results: vec![MutantResult {
                    mutant: second_mutant,
                    status: MutantStatus::Survived,
                    detail: None,
                }],
            },
        ];
        let mut output = Vec::new();

        write_multi_sarif_to(&file_runs, &mut output).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\"uri\":\"src/first.ts\""));
        assert!(output.contains("\"uri\":\"src/second.ts\""));
        assert!(!output.contains("\"uri\":\"/"));
    }

    #[tokio::test]
    async fn hanging_baseline_times_out_and_kills_its_process_group() {
        let options = RunOptions {
            repository: std::env::current_dir().unwrap(),
            file: PathBuf::from("unused.ts"),
            operators: Vec::new(),
            test_command: "perl -e 'alarm 5; exec @ARGV' sleep 30".to_owned(),
            timeout_ms: 1_500,
            concurrency: 1,
            line_range: None,
            sarif: None,
            skip_baseline: false,
            fail_on_survivors: false,
        };
        let started = Instant::now();

        let error = verify_baseline(&options)
            .await
            .expect_err("baseline should time out");
        let elapsed = started.elapsed();

        println!(
            "baseline timeout elapsed seconds: {:.3}",
            elapsed.as_secs_f64()
        );
        assert!(error.contains("timed out"), "{error}");
        assert!(error.contains("--timeout-ms"), "{error}");
        assert!(error.contains("--skip-baseline"), "{error}");
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    }

    fn crawl_fixture_directory() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bughunter-crawl-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn write_crawl_fixture_file(root: &Path, relative_path: &str) {
        let path = root.join(relative_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "export const value = true;\n").unwrap();
    }

    fn result(status: MutantStatus, detail: Option<&str>) -> MutantResult {
        MutantResult {
            mutant: Mutant {
                line: 1,
                column: 1,
                operator: Operator::LogicalAndToOr,
                span_start: 0,
                span_end: 2,
                replacement: "||".to_owned(),
            },
            status,
            detail: detail.map(str::to_owned),
        }
    }
}
