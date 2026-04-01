use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchKind {
    Architecture,
    MigrationRisk,
    CleanupImpact,
    SkillImprovement,
    ModelProtocolComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchBudget {
    pub max_rounds: u32,
    pub max_worker_slots: u32,
    pub max_cost_usd: Option<f64>,
    pub max_duration_minutes: u32,
    pub max_artifacts: u32,
}

impl Default for ResearchBudget {
    fn default() -> Self {
        Self {
            max_rounds: 3,
            max_worker_slots: 5,
            max_cost_usd: None,
            max_duration_minutes: 30,
            max_artifacts: 12,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchQuestion {
    pub kind: ResearchKind,
    pub project_id: String,
    pub subject_ref: Option<String>,
    pub question: String,
    pub goal: String,
    pub constraints: Vec<String>,
    pub success_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchPacket {
    pub research_id: String,
    pub question: ResearchQuestion,
    pub repo_snapshot_ref: Option<String>,
    pub context_refs: Vec<String>,
    pub budget: ResearchBudget,
    pub stop_rules: Vec<String>,
    pub output_schema_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Frozen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchRecord {
    pub research_id: String,
    pub project_id: String,
    pub status: ResearchStatus,
    pub kind: ResearchKind,
    pub created_at: DateTime<Utc>,
    pub packet_path: String,
    pub artifacts_dir: String,
}

#[derive(Debug, Clone)]
pub struct StartResearchRequest {
    pub kind: ResearchKind,
    pub project_id: String,
    pub subject_ref: Option<String>,
    pub question: String,
    pub goal: String,
    pub constraints: Vec<String>,
    pub success_criteria: Vec<String>,
    pub budget: ResearchBudget,
    pub context_refs: Vec<String>,
    pub output_schema_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResearchStart {
    pub record: ResearchRecord,
    pub packet: ResearchPacket,
    pub root_dir: PathBuf,
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
    Err("research requires running inside a Git/jj repository".to_string())
}

fn research_root(project_root: &Path) -> PathBuf {
    project_root.join(".punk/research")
}

fn default_stop_rules() -> Vec<String> {
    vec![
        "max_rounds".to_string(),
        "max_worker_slots".to_string(),
        "max_cost_usd".to_string(),
        "max_duration_minutes".to_string(),
        "max_artifacts".to_string(),
        "enough_evidence_for_synthesis".to_string(),
        "ambiguity_remains_escalate".to_string(),
    ]
}

fn default_output_schema_ref(kind: &ResearchKind) -> &'static str {
    match kind {
        ResearchKind::Architecture => "adr_draft|risk_memo|escalate",
        ResearchKind::MigrationRisk => "risk_memo|contract_patch|escalate",
        ResearchKind::CleanupImpact => "contract_patch|risk_memo|escalate",
        ResearchKind::SkillImprovement => "candidate_patch|eval_suite_patch|escalate",
        ResearchKind::ModelProtocolComparison => "risk_memo|eval_suite_patch|escalate",
    }
}

fn sanitize_fragment(raw: &str) -> String {
    let mut out = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_ascii_lowercase()
}

fn make_research_id(project_id: &str, kind: &ResearchKind) -> String {
    let kind_str = match kind {
        ResearchKind::Architecture => "architecture",
        ResearchKind::MigrationRisk => "migration-risk",
        ResearchKind::CleanupImpact => "cleanup-impact",
        ResearchKind::SkillImprovement => "skill-improvement",
        ResearchKind::ModelProtocolComparison => "model-protocol-comparison",
    };
    format!(
        "research-{}-{}-{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        sanitize_fragment(project_id),
        kind_str
    )
}

pub fn start_research(cwd: &Path, request: StartResearchRequest) -> Result<ResearchStart, String> {
    if request.project_id.trim().is_empty() {
        return Err("project id must be non-empty".to_string());
    }
    if request.question.trim().is_empty() {
        return Err("question must be non-empty".to_string());
    }
    if request.goal.trim().is_empty() {
        return Err("goal must be non-empty".to_string());
    }
    if request.success_criteria.is_empty() {
        return Err("at least one success criterion is required".to_string());
    }

    let repo_root = detect_repo_root(cwd)?;
    let research_id = make_research_id(&request.project_id, &request.kind);
    let root_dir = research_root(&repo_root).join(&research_id);
    let artifacts_dir = root_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).map_err(|e| e.to_string())?;

    let packet = ResearchPacket {
        research_id: research_id.clone(),
        question: ResearchQuestion {
            kind: request.kind.clone(),
            project_id: request.project_id.clone(),
            subject_ref: request.subject_ref,
            question: request.question,
            goal: request.goal,
            constraints: request.constraints,
            success_criteria: request.success_criteria,
        },
        repo_snapshot_ref: None,
        context_refs: request.context_refs,
        budget: request.budget,
        stop_rules: default_stop_rules(),
        output_schema_ref: request
            .output_schema_ref
            .unwrap_or_else(|| default_output_schema_ref(&request.kind).to_string()),
    };

    let packet_path = root_dir.join("packet.json");
    fs::write(
        &packet_path,
        serde_json::to_string_pretty(&packet).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let record = ResearchRecord {
        research_id: research_id.clone(),
        project_id: packet.question.project_id.clone(),
        status: ResearchStatus::Frozen,
        kind: packet.question.kind.clone(),
        created_at: Utc::now(),
        packet_path: packet_path.display().to_string(),
        artifacts_dir: artifacts_dir.display().to_string(),
    };
    let record_path = root_dir.join("record.json");
    fs::write(
        &record_path,
        serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(ResearchStart {
        record,
        packet,
        root_dir,
    })
}

pub fn list_research_runs(cwd: &Path) -> Result<Vec<ResearchRecord>, String> {
    let repo_root = detect_repo_root(cwd)?;
    let root = research_root(&repo_root);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().join("record.json");
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let record = serde_json::from_str::<ResearchRecord>(&content).map_err(|e| e.to_string())?;
        records.push(record);
    }
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(records)
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

    #[test]
    fn start_research_writes_packet_and_record() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::SkillImprovement,
                project_id: "specpunk".into(),
                subject_ref: Some("task:123".into()),
                question: "Why do helper slices stall?".into(),
                goal: "Produce a candidate patch proposal".into(),
                constraints: vec!["bounded".into()],
                success_criteria: vec!["at least one candidate patch".into()],
                budget: ResearchBudget::default(),
                context_refs: vec!["receipt:abc".into()],
                output_schema_ref: None,
            },
        )
        .unwrap();

        assert!(started.root_dir.join("packet.json").exists());
        assert!(started.root_dir.join("record.json").exists());
        assert!(started.root_dir.join("artifacts").exists());
        assert_eq!(started.record.status, ResearchStatus::Frozen);
        assert_eq!(started.packet.question.project_id, "specpunk");
        assert_eq!(
            started.packet.output_schema_ref,
            "candidate_patch|eval_suite_patch|escalate"
        );
    }

    #[test]
    fn start_research_requires_success_criteria() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let err = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::Architecture,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "What architecture?".into(),
                goal: "Write ADR".into(),
                constraints: vec![],
                success_criteria: vec![],
                budget: ResearchBudget::default(),
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap_err();

        assert!(err.contains("success criterion"));
    }

    #[test]
    fn list_research_runs_returns_newest_first() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let first = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::Architecture,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question 1".into(),
                goal: "Goal 1".into(),
                constraints: vec![],
                success_criteria: vec!["done".into()],
                budget: ResearchBudget::default(),
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::CleanupImpact,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question 2".into(),
                goal: "Goal 2".into(),
                constraints: vec![],
                success_criteria: vec!["done".into()],
                budget: ResearchBudget::default(),
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap();

        let listed = list_research_runs(&repo).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].research_id, second.record.research_id);
        assert_eq!(listed[1].research_id, first.record.research_id);
    }
}
