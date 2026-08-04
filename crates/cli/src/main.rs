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
  bughunter run --repo <DIR> --file <RELATIVE.ts> --operators <IDS> --test <CMD> --json [OPTIONS]

REQUIRED:
  --repo <DIR>          repository root; the test command runs here
  --file <RELATIVE.ts>  source file to mutate, relative to --repo
  --operators <IDS>     comma-separated operator ids (see below)
  --test <CMD>          test command, run once per mutant
  --json                emit JSON on stdout

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
    let source_path = options.repository.join(&options.file);
    let source = fs::read_to_string(&source_path)
        .map_err(|error| format!("failed to read {}: {error}", source_path.display()))?;
    let generated = select_line_range(
        try_mutants(&source, &options.file.to_string_lossy(), &options.operators)
            .map_err(|failure| failure.to_string())?,
        options.line_range,
    );
    if !options.skip_baseline {
        verify_baseline(&options).await?;
    }
    let configuration = RunnerConfig::new(
        &options.repository,
        &options.file,
        format!("({}) 1>&2", options.test_command),
        Duration::from_millis(options.timeout_ms),
        options.concurrency,
    );
    let runner = Runner::new(configuration);
    let mut results = runner.run(&generated).await;
    results.sort_by_key(|result| (result.mutant.span_start, result.mutant.operator));
    write_json(&options.file, &source, &results)
        .map_err(|error| format!("failed to write JSON: {error}"))?;
    Ok(exit_code_for_results(options.fail_on_survivors, &results))
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
    skip_baseline: bool,
    fail_on_survivors: bool,
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
    if !is_relative_file(&file) {
        return Err(
            "--file must be a non-empty relative path without parent components".to_owned(),
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

fn is_relative_file(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.is_relative()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

fn write_json(file: &Path, source: &str, results: &[MutantResult]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    write_json_to(file, source, results, &mut output)
}

fn write_json_to<W: Write>(
    file: &Path,
    source: &str,
    results: &[MutantResult],
    output: &mut W,
) -> io::Result<()> {
    let summary = ResultSummary::from_results(results);
    write!(
        output,
        "{{\"schema_version\":1,\"total\":{},\"killed\":{},\"survived\":{},\"timeout\":{},\"error\":{},\"evaluated\":{},\"score\":",
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
        write_json_string(output, &stable_mutant_id(file, source, &result.mutant))?;
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
}

fn stable_mutant_id(file: &Path, source: &str, mutant: &bughunter_engine::Mutant) -> String {
    let original = source
        .get(mutant.span_start as usize..mutant.span_end as usize)
        .unwrap_or_default();
    let hash = stable_hash(&[
        file.to_string_lossy().as_bytes(),
        mutant.operator.id().as_bytes(),
        original.as_bytes(),
        mutant.replacement.as_bytes(),
    ]);
    format!("{hash:016x}")
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
        exit_code_for_results, parse_line_range, parse_run_arguments, select_line_range,
        stable_mutant_id, verify_baseline, version_output, write_json_to, LineRange, ResultSummary,
        RunOptions,
    };
    use bughunter_engine::{mutants, Mutant, Operator};
    use bughunter_runner::{MutantResult, MutantStatus};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

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
        assert!(output.starts_with("{\"schema_version\":1,"));
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
            "{\"schema_version\":1,\"total\":5,\"killed\":2,\"survived\":1,\"timeout\":1,\"error\":1,\"evaluated\":3,\"score\":0.6666666666666666,"
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
    fn stable_mutant_id_survives_a_line_shift() {
        let source = "const ready = left && right;\n";
        let shifted_source = "\n\nconst ready = left && right;\n";
        let original = mutants(source, "src/ready.ts", &[Operator::LogicalAndToOr]);
        let shifted = mutants(shifted_source, "src/ready.ts", &[Operator::LogicalAndToOr]);

        assert_ne!(original[0].line, shifted[0].line);
        assert_eq!(
            stable_mutant_id(Path::new("src/ready.ts"), source, &original[0]),
            stable_mutant_id(Path::new("src/ready.ts"), shifted_source, &shifted[0])
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
            stable_mutant_id(Path::new("src/ready.ts"), source, &original[0]),
            stable_mutant_id(
                Path::new("src/ready.ts"),
                source,
                &same_location_different_operator
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
