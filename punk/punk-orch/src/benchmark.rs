use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkOutcome {
    Pass,
    Fail,
    Flaky,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkMetric {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkResult {
    pub benchmark_id: String,
    pub suite: String,
    pub project_id: String,
    pub subject_ref: Option<String>,
    pub outcome: BenchmarkOutcome,
    pub score: f64,
    pub metrics: Vec<BenchmarkMetric>,
    pub notes: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBenchmarkSummary {
    pub project_id: String,
    pub total: usize,
    pub pass_count: usize,
    pub fail_count: usize,
    pub flaky_count: usize,
    pub avg_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SuiteBenchmarkSummary {
    pub suite: String,
    pub total: usize,
    pub pass_count: usize,
    pub fail_count: usize,
    pub flaky_count: usize,
    pub avg_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeakBenchmarkResult {
    pub benchmark_id: String,
    pub suite: String,
    pub project_id: String,
    pub outcome: BenchmarkOutcome,
    pub score: f64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BenchmarkSummary {
    pub total: usize,
    pub pass_count: usize,
    pub fail_count: usize,
    pub flaky_count: usize,
    pub avg_score: f64,
    pub projects: Vec<ProjectBenchmarkSummary>,
    pub suites: Vec<SuiteBenchmarkSummary>,
    pub weakest: Vec<WeakBenchmarkResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordBenchmarkRequest {
    pub suite: String,
    pub project_id: String,
    pub subject_ref: Option<String>,
    pub outcome: BenchmarkOutcome,
    pub score: f64,
    pub metrics: Vec<BenchmarkMetric>,
    pub notes: Vec<String>,
}

fn detect_repo_root(cwd: &Path) -> Result<PathBuf, String> {
    for (bin, args) in [
        ("jj", vec!["root"]),
        ("git", vec!["rev-parse", "--show-toplevel"]),
    ] {
        let output = std::process::Command::new(bin)
            .args(&args)
            .current_dir(cwd)
            .output();
        if let Ok(output) = output {
            if output.status.success() {
                let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !root.is_empty() {
                    let root_path = PathBuf::from(root);
                    return Ok(root_path.canonicalize().unwrap_or(root_path));
                }
            }
        }
    }
    Err("benchmark requires running inside a Git/jj repository".to_string())
}

fn benchmark_results_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".punk/eval/benchmarks")
}

fn benchmark_result_path(repo_root: &Path, benchmark_id: &str) -> PathBuf {
    benchmark_results_dir(repo_root).join(format!("{benchmark_id}.json"))
}

fn sanitize_component(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn make_benchmark_id(project_id: &str, suite: &str) -> String {
    let project = sanitize_component(project_id);
    let suite = sanitize_component(suite);
    format!(
        "{}-{}-{}",
        if project.is_empty() {
            "project"
        } else {
            &project
        },
        if suite.is_empty() { "suite" } else { &suite },
        Utc::now().format("%Y%m%d%H%M%S")
    )
}

pub fn record_benchmark(
    cwd: &Path,
    request: RecordBenchmarkRequest,
) -> Result<BenchmarkResult, String> {
    if request.project_id.trim().is_empty() {
        return Err("project id must be non-empty".to_string());
    }
    if request.suite.trim().is_empty() {
        return Err("suite must be non-empty".to_string());
    }
    if !(0.0..=1.0).contains(&request.score) {
        return Err("score must be between 0.0 and 1.0".to_string());
    }
    for metric in &request.metrics {
        if metric.name.trim().is_empty() {
            return Err("metric name must be non-empty".to_string());
        }
    }

    let repo_root = detect_repo_root(cwd)?;
    fs::create_dir_all(benchmark_results_dir(&repo_root)).map_err(|e| e.to_string())?;

    let result = BenchmarkResult {
        benchmark_id: make_benchmark_id(&request.project_id, &request.suite),
        suite: request.suite,
        project_id: request.project_id,
        subject_ref: request.subject_ref,
        outcome: request.outcome,
        score: request.score,
        metrics: request.metrics,
        notes: request.notes,
        created_at: Utc::now(),
    };
    let path = benchmark_result_path(&repo_root, &result.benchmark_id);
    fs::write(
        &path,
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn list_benchmarks(cwd: &Path) -> Result<Vec<BenchmarkResult>, String> {
    let repo_root = detect_repo_root(cwd)?;
    let dir = benchmark_results_dir(&repo_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let result =
            serde_json::from_str::<BenchmarkResult>(&content).map_err(|e| e.to_string())?;
        results.push(result);
    }
    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(results)
}

pub fn load_benchmark(cwd: &Path, benchmark_id: &str) -> Result<BenchmarkResult, String> {
    let repo_root = detect_repo_root(cwd)?;
    let path = benchmark_result_path(&repo_root, benchmark_id);
    if !path.exists() {
        return Err(format!("benchmark result not found: {benchmark_id}"));
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str::<BenchmarkResult>(&content).map_err(|e| e.to_string())
}

fn outcome_counts(results: &[&BenchmarkResult]) -> (usize, usize, usize) {
    let pass_count = results
        .iter()
        .filter(|result| result.outcome == BenchmarkOutcome::Pass)
        .count();
    let fail_count = results
        .iter()
        .filter(|result| result.outcome == BenchmarkOutcome::Fail)
        .count();
    let flaky_count = results.len() - pass_count - fail_count;
    (pass_count, fail_count, flaky_count)
}

pub fn summarize_benchmarks(
    cwd: &Path,
    limit: Option<usize>,
    project_filter: Option<&str>,
    suite_filter: Option<&str>,
) -> Result<BenchmarkSummary, String> {
    let mut results = list_benchmarks(cwd)?;
    if let Some(project) = project_filter {
        results.retain(|result| result.project_id == project);
    }
    if let Some(suite) = suite_filter {
        results.retain(|result| result.suite == suite);
    }
    if let Some(limit) = limit {
        results.truncate(limit);
    }
    if results.is_empty() {
        return Err("no benchmark results found for summary".to_string());
    }

    let total = results.len();
    let pass_count = results
        .iter()
        .filter(|result| result.outcome == BenchmarkOutcome::Pass)
        .count();
    let fail_count = results
        .iter()
        .filter(|result| result.outcome == BenchmarkOutcome::Fail)
        .count();
    let flaky_count = total - pass_count - fail_count;
    let avg_score = results.iter().map(|result| result.score).sum::<f64>() / total as f64;

    let mut project_ids = results
        .iter()
        .map(|result| result.project_id.clone())
        .collect::<Vec<_>>();
    project_ids.sort();
    project_ids.dedup();

    let mut projects = Vec::new();
    for project_id in project_ids {
        let project_results = results
            .iter()
            .filter(|result| result.project_id == project_id)
            .collect::<Vec<_>>();
        let (pass_count, fail_count, flaky_count) = outcome_counts(&project_results);
        let avg_score = project_results
            .iter()
            .map(|result| result.score)
            .sum::<f64>()
            / project_results.len() as f64;
        projects.push(ProjectBenchmarkSummary {
            project_id,
            total: project_results.len(),
            pass_count,
            fail_count,
            flaky_count,
            avg_score,
        });
    }
    projects.sort_by(|a, b| {
        a.avg_score
            .partial_cmp(&b.avg_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.project_id.cmp(&b.project_id))
    });

    let mut suite_ids = results
        .iter()
        .map(|result| result.suite.clone())
        .collect::<Vec<_>>();
    suite_ids.sort();
    suite_ids.dedup();

    let mut suites = Vec::new();
    for suite in suite_ids {
        let suite_results = results
            .iter()
            .filter(|result| result.suite == suite)
            .collect::<Vec<_>>();
        let (pass_count, fail_count, flaky_count) = outcome_counts(&suite_results);
        let avg_score = suite_results.iter().map(|result| result.score).sum::<f64>()
            / suite_results.len() as f64;
        suites.push(SuiteBenchmarkSummary {
            suite,
            total: suite_results.len(),
            pass_count,
            fail_count,
            flaky_count,
            avg_score,
        });
    }
    suites.sort_by(|a, b| {
        a.avg_score
            .partial_cmp(&b.avg_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.suite.cmp(&b.suite))
    });

    let mut weakest = results
        .iter()
        .map(|result| WeakBenchmarkResult {
            benchmark_id: result.benchmark_id.clone(),
            suite: result.suite.clone(),
            project_id: result.project_id.clone(),
            outcome: result.outcome.clone(),
            score: result.score,
            created_at: result.created_at,
        })
        .collect::<Vec<_>>();
    weakest.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    weakest.truncate(5);

    Ok(BenchmarkSummary {
        total,
        pass_count,
        fail_count,
        flaky_count,
        avg_score,
        projects,
        suites,
        weakest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo(path: &Path) {
        std::process::Command::new("git")
            .arg("init")
            .arg(path)
            .output()
            .unwrap();
    }

    fn sample_request() -> RecordBenchmarkRequest {
        RecordBenchmarkRequest {
            suite: "task-eval-smoke".into(),
            project_id: "specpunk".into(),
            subject_ref: Some("task:task-123".into()),
            outcome: BenchmarkOutcome::Pass,
            score: 0.91,
            metrics: vec![
                BenchmarkMetric {
                    name: "accuracy".into(),
                    value: 0.9,
                },
                BenchmarkMetric {
                    name: "stability".into(),
                    value: 1.0,
                },
            ],
            notes: vec!["smoke benchmark".into()],
        }
    }

    #[test]
    fn record_benchmark_writes_repo_local_json() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let result = record_benchmark(&repo, sample_request()).unwrap();
        let path = benchmark_result_path(&repo.canonicalize().unwrap(), &result.benchmark_id);
        assert!(path.exists());
        assert_eq!(result.project_id, "specpunk");
        assert_eq!(result.metrics.len(), 2);
    }

    #[test]
    fn list_benchmarks_returns_newest_first() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let mut first = sample_request();
        first.suite = "alpha".into();
        let first = record_benchmark(&repo, first).unwrap();

        let mut second = sample_request();
        second.suite = "beta".into();
        let second = record_benchmark(&repo, second).unwrap();

        let listed = list_benchmarks(&repo).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].benchmark_id, second.benchmark_id);
        assert_eq!(listed[1].benchmark_id, first.benchmark_id);
    }

    #[test]
    fn record_benchmark_rejects_invalid_score() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let mut request = sample_request();
        request.score = 1.5;
        let err = record_benchmark(&repo, request).unwrap_err();
        assert!(err.contains("score must be between 0.0 and 1.0"));
    }

    #[test]
    fn summarize_benchmarks_aggregates_results() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let mut first = sample_request();
        first.suite = "alpha".into();
        first.score = 0.95;
        first.outcome = BenchmarkOutcome::Pass;
        let _ = record_benchmark(&repo, first).unwrap();

        let mut second = sample_request();
        second.suite = "beta".into();
        second.score = 0.4;
        second.outcome = BenchmarkOutcome::Fail;
        let _ = record_benchmark(&repo, second).unwrap();

        let summary = summarize_benchmarks(&repo, None, None, None).unwrap();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.pass_count, 1);
        assert_eq!(summary.fail_count, 1);
        assert_eq!(summary.flaky_count, 0);
        assert_eq!(summary.projects.len(), 1);
        assert_eq!(summary.suites.len(), 2);
        assert_eq!(summary.weakest[0].suite, "beta");
    }

    #[test]
    fn summarize_benchmarks_applies_filters_and_limit() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let mut first = sample_request();
        first.project_id = "alpha".into();
        first.suite = "smoke".into();
        let _ = record_benchmark(&repo, first).unwrap();

        let mut second = sample_request();
        second.project_id = "beta".into();
        second.suite = "stress".into();
        second.score = 0.5;
        let _ = record_benchmark(&repo, second).unwrap();

        let mut third = sample_request();
        third.project_id = "alpha".into();
        third.suite = "stress".into();
        let _ = record_benchmark(&repo, third).unwrap();

        let summary = summarize_benchmarks(&repo, Some(1), Some("alpha"), Some("stress")).unwrap();
        assert_eq!(summary.total, 1);
        assert_eq!(summary.projects.len(), 1);
        assert_eq!(summary.projects[0].project_id, "alpha");
        assert_eq!(summary.suites.len(), 1);
        assert_eq!(summary.suites[0].suite, "stress");
    }
}
