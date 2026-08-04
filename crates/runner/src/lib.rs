#[cfg(not(unix))]
compile_error!("runner requires Unix process groups");

use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bughunter_engine::Mutant;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::timeout;

static RUN_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct RunnerConfig {
    pub repository: PathBuf,
    pub source_file: PathBuf,
    pub test_command: String,
    pub timeout: Duration,
    pub concurrency: usize,
}

impl RunnerConfig {
    pub fn new(
        repository: impl Into<PathBuf>,
        source_file: impl Into<PathBuf>,
        test_command: impl Into<String>,
        timeout: Duration,
        concurrency: usize,
    ) -> Self {
        Self {
            repository: repository.into(),
            source_file: source_file.into(),
            test_command: test_command.into(),
            timeout,
            concurrency,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutantStatus {
    Killed,
    Survived,
    Timeout,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutantResult {
    pub mutant: Mutant,
    pub status: MutantStatus,
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Runner {
    configuration: RunnerConfig,
}

impl Runner {
    pub fn new(configuration: RunnerConfig) -> Self {
        Self { configuration }
    }

    pub async fn run(&self, mutants: &[Mutant]) -> Vec<MutantResult> {
        run_mutants(&self.configuration, mutants).await
    }
}

pub async fn run_mutants(configuration: &RunnerConfig, mutants: &[Mutant]) -> Vec<MutantResult> {
    if configuration.concurrency == 0 {
        return sorted_results(
            mutants
                .iter()
                .cloned()
                .map(|mutant| error_result(mutant, "concurrency must be greater than zero"))
                .collect(),
        );
    }

    let semaphore = Arc::new(Semaphore::new(configuration.concurrency));
    let mut tasks = Vec::with_capacity(mutants.len());

    for mutant in mutants.iter().cloned() {
        let task_configuration = configuration.clone();
        let task_semaphore = Arc::clone(&semaphore);
        let fallback_mutant = mutant.clone();
        let task = tokio::spawn(async move {
            let permit = task_semaphore
                .acquire_owned()
                .await
                .expect("runner semaphore remains open");
            let result = run_one(&task_configuration, mutant).await;
            drop(permit);
            result
        });
        tasks.push((fallback_mutant, task));
    }

    let mut results = Vec::with_capacity(tasks.len());
    for (mutant, task) in tasks {
        match task.await {
            Ok(result) => results.push(result),
            Err(error) => results.push(error_result(mutant, error.to_string())),
        }
    }

    sorted_results(results)
}

pub fn killed_count(results: &[MutantResult]) -> usize {
    results
        .iter()
        .filter(|result| result.status == MutantStatus::Killed)
        .count()
}

async fn run_one(configuration: &RunnerConfig, mutant: Mutant) -> MutantResult {
    let materialized_tree = match materialize(configuration, &mutant) {
        Ok(tree) => tree,
        Err(error) => return error_result(mutant, error),
    };

    let result = run_command(configuration, &mutant, &materialized_tree).await;
    if let Err(error) = fs::remove_dir_all(&materialized_tree) {
        if result.status == MutantStatus::Error {
            return MutantResult {
                detail: Some(format!(
                    "{}; failed to remove materialized tree: {error}",
                    result.detail.unwrap_or_default()
                )),
                ..result
            };
        }
    }
    result
}

async fn run_command(
    configuration: &RunnerConfig,
    mutant: &Mutant,
    materialized_tree: &Path,
) -> MutantResult {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(&configuration.test_command)
        .current_dir(materialized_tree);
    configure_process_group(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return error_result(mutant.clone(), error.to_string()),
    };
    let process_group = match child.id() {
        Some(identifier) => identifier as i32,
        None => return error_result(mutant.clone(), "spawned command has no process identifier"),
    };

    match timeout(configuration.timeout, child.wait()).await {
        Ok(Ok(exit_status)) if exit_status.success() => MutantResult {
            mutant: mutant.clone(),
            status: MutantStatus::Survived,
            detail: None,
        },
        Ok(Ok(exit_status)) if exit_status.code() == Some(127) => {
            error_result(mutant.clone(), "test command could not be executed")
        }
        Ok(Ok(_)) => MutantResult {
            mutant: mutant.clone(),
            status: MutantStatus::Killed,
            detail: None,
        },
        Ok(Err(error)) => error_result(mutant.clone(), error.to_string()),
        Err(_) => {
            let kill_detail = kill_process_group(process_group)
                .err()
                .map(|error| error.to_string());
            let wait_detail = child.wait().await.err().map(|error| error.to_string());
            let detail = timeout_detail(kill_detail, wait_detail);
            MutantResult {
                mutant: mutant.clone(),
                status: MutantStatus::Timeout,
                detail,
            }
        }
    }
}

fn timeout_detail(kill_detail: Option<String>, wait_detail: Option<String>) -> Option<String> {
    match (kill_detail, wait_detail) {
        (None, None) => None,
        (Some(kill_detail), None) => Some(format!("process-group cleanup failed: {kill_detail}")),
        (None, Some(wait_detail)) => Some(format!("child reap failed: {wait_detail}")),
        (Some(kill_detail), Some(wait_detail)) => Some(format!(
            "process-group cleanup failed: {kill_detail}; child reap failed: {wait_detail}"
        )),
    }
}

fn configure_process_group(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn kill_process_group(process_group: i32) -> io::Result<()> {
    if unsafe { libc::killpg(process_group, libc::SIGKILL) } == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

fn materialize(configuration: &RunnerConfig, mutant: &Mutant) -> Result<PathBuf, String> {
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

    Ok(materialized_tree)
}

fn resolve_source_file(
    repository: &Path,
    configured_source_file: &Path,
) -> Result<PathBuf, String> {
    let candidate = if configured_source_file.is_absolute() {
        configured_source_file.to_path_buf()
    } else {
        repository.join(configured_source_file)
    };
    let source_file = fs::canonicalize(candidate)
        .map_err(|error| format!("failed to resolve source file: {error}"))?;
    if !source_file.starts_with(repository) {
        return Err("source file must be inside repository".to_owned());
    }
    Ok(source_file)
}

fn replace_mutant(source: &str, mutant: &Mutant) -> Result<String, String> {
    let start = mutant.span_start as usize;
    let end = mutant.span_end as usize;
    let Some(replaced) = source.get(start..end) else {
        return Err("mutant byte span is outside source text or splits UTF-8".to_owned());
    };

    let mut mutated =
        String::with_capacity(source.len() - replaced.len() + mutant.replacement.len());
    mutated.push_str(&source[..start]);
    mutated.push_str(&mutant.replacement);
    mutated.push_str(&source[end..]);
    Ok(mutated)
}

fn create_run_directory() -> Result<PathBuf, String> {
    let parent = Path::new("/tmp/bh-work/runner-materializations");
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create runner work directory: {error}"))?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_nanos();
    let sequence = RUN_DIRECTORY_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
    let path = parent.join(format!("{}-{timestamp}-{sequence}", std::process::id()));
    fs::create_dir(&path)
        .map_err(|error| format!("failed to create materialized tree: {error}"))?;
    Ok(path)
}

fn copy_repository(source: &Path, destination: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == OsStr::new(".git") {
            continue;
        }

        let source_path = entry.path();
        let destination_path = destination.join(&name);
        if name == OsStr::new("node_modules") {
            symlink_absolute(&source_path, &destination_path)?;
            continue;
        }

        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir(&destination_path)?;
            copy_repository(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(&source_path)?;
            std::os::unix::fs::symlink(target, destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, fs::metadata(&source_path)?.permissions())?;
        }
    }
    Ok(())
}

fn symlink_absolute(source: &Path, destination: &Path) -> io::Result<()> {
    let absolute_source = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    std::os::unix::fs::symlink(absolute_source, destination)
}

fn error_result(mutant: Mutant, detail: impl Into<String>) -> MutantResult {
    MutantResult {
        mutant,
        status: MutantStatus::Error,
        detail: Some(detail.into()),
    }
}

fn sorted_results(mut results: Vec<MutantResult>) -> Vec<MutantResult> {
    results.sort_by(compare_results);
    results
}

fn compare_results(left: &MutantResult, right: &MutantResult) -> Ordering {
    (
        left.mutant.span_start,
        left.mutant.span_end,
        left.mutant.operator,
        left.mutant.replacement.as_str(),
    )
        .cmp(&(
            right.mutant.span_start,
            right.mutant.span_end,
            right.mutant.operator,
            right.mutant.replacement.as_str(),
        ))
}

#[cfg(test)]
mod tests {
    use super::{killed_count, run_mutants, MutantResult, MutantStatus, RunnerConfig};
    use bughunter_engine::{Mutant, Operator};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = Path::new("/tmp/bh-work/runner-tests").join(format!(
                "{name}-{}-{}",
                std::process::id(),
                FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("scripts")).unwrap();
            fs::create_dir_all(root.join("node_modules")).unwrap();
            fs::write(root.join("source.txt"), "abcdef").unwrap();
            fs::write(root.join("node_modules/reachable"), "reachable").unwrap();
            Self { root }
        }

        fn configuration(
            &self,
            command: impl Into<String>,
            timeout: Duration,
            concurrency: usize,
        ) -> RunnerConfig {
            RunnerConfig::new(&self.root, "source.txt", command, timeout, concurrency)
        }

        fn write_script(&self, name: &str, body: &str) {
            fs::write(self.root.join("scripts").join(name), body).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn mutant(span_start: u32) -> Mutant {
        Mutant {
            line: 1,
            column: span_start + 1,
            operator: Operator::LogicalAndToOr,
            span_start,
            span_end: span_start + 1,
            replacement: "x".to_owned(),
        }
    }

    fn one_result(results: Vec<MutantResult>) -> MutantResult {
        assert_eq!(results.len(), 1);
        results.into_iter().next().unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn nonzero_test_command_kills_the_mutant_and_uses_a_node_modules_symlink() {
        let fixture = Fixture::new("killed");
        let configuration = fixture.configuration(
            "test -L node_modules && test -f node_modules/reachable && test \"$(cat source.txt)\" = xbcdef && exit 1",
            Duration::from_secs(2),
            1,
        );

        let result = one_result(run_mutants(&configuration, &[mutant(0)]).await);

        assert_eq!(result.status, MutantStatus::Killed);
        assert_eq!(
            fs::read_to_string(fixture.root.join("source.txt")).unwrap(),
            "abcdef"
        );
        assert_eq!(killed_count(&[result]), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zero_test_command_survives_the_mutant() {
        let fixture = Fixture::new("survived");
        let configuration = fixture.configuration(
            "test -L node_modules && test -f node_modules/reachable && test \"$(cat source.txt)\" = xbcdef",
            Duration::from_secs(2),
            1,
        );

        let result = one_result(run_mutants(&configuration, &[mutant(0)]).await);

        assert_eq!(result.status, MutantStatus::Survived);
        assert_eq!(killed_count(&[result]), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_test_command_times_out_without_being_killed_or_surviving() {
        let fixture = Fixture::new("timeout");
        let configuration = fixture.configuration("sleep 5", Duration::from_millis(120), 1);

        let result = one_result(run_mutants(&configuration, &[mutant(0)]).await);

        assert_eq!(result.status, MutantStatus::Timeout);
        assert_ne!(result.status, MutantStatus::Killed);
        assert_ne!(result.status, MutantStatus::Survived);
        assert_eq!(killed_count(&[result]), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unavailable_test_command_is_an_error() {
        let fixture = Fixture::new("error");
        let configuration = fixture.configuration(
            "definitely-not-a-real-binary-xyz",
            Duration::from_secs(2),
            1,
        );

        let result = one_result(run_mutants(&configuration, &[mutant(0)]).await);

        assert_eq!(result.status, MutantStatus::Error);
        assert_eq!(killed_count(&[result]), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_kills_background_processes_in_the_process_group() {
        let fixture = Fixture::new("orphan");
        let pid_path = fixture.root.join("background.pid");
        fixture.write_script(
            "spawn-orphan.sh",
            "#!/bin/sh\nsleep 5 &\necho $! > \"$1\"\nsleep 5\n",
        );
        let configuration = fixture.configuration(
            format!("sh scripts/spawn-orphan.sh {}", pid_path.display()),
            Duration::from_secs(2),
            1,
        );

        let result = one_result(run_mutants(&configuration, &[mutant(0)]).await);
        let background_pid = wait_for_pid(&pid_path);
        let reaped = wait_until_reaped(background_pid);
        if !reaped {
            terminate(background_pid);
        }

        assert_eq!(result.status, MutantStatus::Timeout);
        assert_ne!(result.status, MutantStatus::Killed);
        assert_ne!(result.status, MutantStatus::Survived);
        assert!(
            reaped,
            "background process {background_pid} remained alive after timeout"
        );
        assert_eq!(killed_count(&[result]), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrency_limit_bounds_parallel_test_commands() {
        let fixture = Fixture::new("concurrency");
        let state = fixture.root.join("state");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("current"), "0").unwrap();
        fs::write(state.join("maximum"), "0").unwrap();
        fixture.write_script(
            "measure-concurrency.sh",
            "#!/bin/sh\nstate=\"$1\"\nlock=\"$state/lock\"\nwhile ! mkdir \"$lock\" 2>/dev/null; do\n  sleep 0.01\ndone\ncurrent=$(cat \"$state/current\")\ncurrent=$((current + 1))\necho \"$current\" > \"$state/current\"\nmaximum=$(cat \"$state/maximum\")\nif [ \"$current\" -gt \"$maximum\" ]; then\n  echo \"$current\" > \"$state/maximum\"\nfi\nrmdir \"$lock\"\nsleep 0.2\nwhile ! mkdir \"$lock\" 2>/dev/null; do\n  sleep 0.01\ndone\ncurrent=$(cat \"$state/current\")\ncurrent=$((current - 1))\necho \"$current\" > \"$state/current\"\nrmdir \"$lock\"\n",
        );
        let limit = 2;
        let configuration = fixture.configuration(
            format!("sh scripts/measure-concurrency.sh {}", state.display()),
            Duration::from_secs(3),
            limit,
        );
        let mutants = [mutant(4), mutant(0), mutant(3), mutant(1), mutant(2)];

        let results = run_mutants(&configuration, &mutants).await;
        let maximum: usize = fs::read_to_string(state.join("maximum"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();

        assert!(results
            .iter()
            .all(|result| result.status == MutantStatus::Survived));
        assert_eq!(maximum, limit);
        assert_eq!(
            results
                .iter()
                .map(|result| result.mutant.span_start)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4]
        );
    }

    fn wait_for_pid(path: &Path) -> i32 {
        for _ in 0..100 {
            if let Ok(pid) = fs::read_to_string(path) {
                return pid.trim().parse().unwrap();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("background child did not write its PID");
    }

    fn wait_until_reaped(pid: i32) -> bool {
        for _ in 0..200 {
            if process_is_reaped(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn process_is_reaped(pid: i32) -> bool {
        let missing = unsafe { libc::kill(pid, 0) == -1 };
        missing && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    fn terminate(pid: i32) {
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}
