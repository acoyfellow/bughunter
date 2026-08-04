use bughunter_engine::{try_mutants, Operator};
use bughunter_runner::{MutantResult, MutantStatus, Runner, RunnerConfig};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_CONCURRENCY: usize = 4;

#[tokio::main]
async fn main() {
    if let Err(error) = run(env::args().skip(1).collect()).await {
        eprintln!("error: {error}");
        std::process::exit(1);
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
  0  the run completed and JSON was written
  1  a usage, parse, or baseline error occurred

A surviving mutant is a fact about your tests, not a proven bug.
";

async fn run(arguments: Vec<String>) -> Result<(), String> {
    if arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "--help" | "-h" | "help" | "--version"))
    {
        print!("{USAGE}");
        return Ok(());
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
    write_json(generated.len(), &results).map_err(|error| format!("failed to write JSON: {error}"))
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
}

async fn verify_baseline(options: &RunOptions) -> Result<(), String> {
    let status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("({}) 1>&2", options.test_command))
        .current_dir(&options.repository)
        .status()
        .await
        .map_err(|error| format!("failed to run the baseline test command: {error}"))?;
    if !status.success() {
        return Err(format!(
            "baseline test command failed with {status} in {}; mutation results would be meaningless because every mutant would be reported killed. Fix the suite, or pass --skip-baseline to override",
            options.repository.display()
        ));
    }
    Ok(())
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

fn write_json(total: usize, results: &[MutantResult]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    write!(output, "{{\"total\":{total},\"mutants\":[")?;
    for (index, result) in results.iter().enumerate() {
        if index > 0 {
            write!(output, ",")?;
        }
        write!(
            output,
            "{{\"line\":{},\"operator\":\"{}\",\"status\":\"{}\"}}",
            result.mutant.line,
            result.mutant.operator.id(),
            status_id(result.status),
        )?;
    }
    writeln!(output, "]}}")
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
    use super::{parse_line_range, parse_run_arguments, select_line_range, LineRange};
    use bughunter_engine::{mutants, Operator};

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
}
