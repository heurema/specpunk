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
pub enum ResearchArtifactKind {
    Note,
    Hypothesis,
    Comparison,
    Critique,
    SynthesisInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Frozen,
    Completed,
    Escalated,
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
    pub synthesis_path: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchOutcome {
    Answer,
    CandidatePatch,
    ContractPatch,
    AdrDraft,
    RiskMemo,
    EvalSuitePatch,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchSynthesis {
    pub research_id: String,
    pub outcome: ResearchOutcome,
    pub title: String,
    pub findings: Vec<String>,
    pub recommendations: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub unresolved_questions: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SynthesizeResearchRequest {
    pub outcome: ResearchOutcome,
    pub title: String,
    pub findings: Vec<String>,
    pub recommendations: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub unresolved_questions: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct ResearchSynthesisWrite {
    pub synthesis: ResearchSynthesis,
    pub synthesis_path: PathBuf,
    pub record: ResearchRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchArtifact {
    pub research_id: String,
    pub kind: ResearchArtifactKind,
    pub title: String,
    pub content: String,
    pub evidence_refs: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct WriteResearchArtifactRequest {
    pub kind: ResearchArtifactKind,
    pub title: String,
    pub content: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResearchArtifactWrite {
    pub artifact: ResearchArtifact,
    pub artifact_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResearchSummary {
    pub research_id: String,
    pub project_id: String,
    pub status: ResearchStatus,
    pub kind: ResearchKind,
    pub created_at: DateTime<Utc>,
    pub artifact_count: usize,
    pub has_synthesis: bool,
}

#[derive(Debug, Clone)]
pub struct ResearchInspect {
    pub root_dir: PathBuf,
    pub record: ResearchRecord,
    pub packet: ResearchPacket,
    pub artifacts: Vec<ResearchArtifact>,
    pub synthesis: Option<ResearchSynthesis>,
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

fn research_dir(cwd: &Path, research_id: &str) -> Result<PathBuf, String> {
    let repo_root = detect_repo_root(cwd)?;
    Ok(research_root(&repo_root).join(research_id))
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

fn artifact_kind_str(kind: &ResearchArtifactKind) -> &'static str {
    match kind {
        ResearchArtifactKind::Note => "note",
        ResearchArtifactKind::Hypothesis => "hypothesis",
        ResearchArtifactKind::Comparison => "comparison",
        ResearchArtifactKind::Critique => "critique",
        ResearchArtifactKind::SynthesisInput => "synthesis-input",
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

fn load_record(root_dir: &Path) -> Result<ResearchRecord, String> {
    let record_path = root_dir.join("record.json");
    let record_content = fs::read_to_string(&record_path).map_err(|e| e.to_string())?;
    serde_json::from_str::<ResearchRecord>(&record_content).map_err(|e| e.to_string())
}

fn save_record(root_dir: &Path, record: &ResearchRecord) -> Result<(), String> {
    let record_path = root_dir.join("record.json");
    fs::write(
        &record_path,
        serde_json::to_string_pretty(record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn load_packet(root_dir: &Path) -> Result<ResearchPacket, String> {
    let packet_path = root_dir.join("packet.json");
    let packet_content = fs::read_to_string(&packet_path).map_err(|e| e.to_string())?;
    serde_json::from_str::<ResearchPacket>(&packet_content).map_err(|e| e.to_string())
}

fn load_artifacts(root_dir: &Path) -> Result<Vec<ResearchArtifact>, String> {
    let artifacts_dir = root_dir.join("artifacts");
    if !artifacts_dir.exists() {
        return Ok(Vec::new());
    }

    let mut artifacts = Vec::new();
    for entry in fs::read_dir(&artifacts_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let artifact =
            serde_json::from_str::<ResearchArtifact>(&content).map_err(|e| e.to_string())?;
        artifacts.push(artifact);
    }
    artifacts.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(artifacts)
}

fn load_synthesis(root_dir: &Path) -> Result<Option<ResearchSynthesis>, String> {
    let synthesis_path = root_dir.join("synthesis.json");
    if !synthesis_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&synthesis_path).map_err(|e| e.to_string())?;
    let synthesis =
        serde_json::from_str::<ResearchSynthesis>(&content).map_err(|e| e.to_string())?;
    Ok(Some(synthesis))
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
        synthesis_path: None,
        completed_at: None,
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

pub fn summarize_research_runs(cwd: &Path) -> Result<Vec<ResearchSummary>, String> {
    let repo_root = detect_repo_root(cwd)?;
    let records = list_research_runs(cwd)?;
    let mut summaries = Vec::new();
    for record in records {
        let root_dir = research_root(&repo_root).join(&record.research_id);
        let artifacts = load_artifacts(&root_dir)?;
        summaries.push(ResearchSummary {
            research_id: record.research_id.clone(),
            project_id: record.project_id.clone(),
            status: record.status.clone(),
            kind: record.kind.clone(),
            created_at: record.created_at,
            artifact_count: artifacts.len(),
            has_synthesis: record.synthesis_path.is_some(),
        });
    }
    Ok(summaries)
}

pub fn inspect_research(cwd: &Path, research_id: &str) -> Result<ResearchInspect, String> {
    let root_dir = research_dir(cwd, research_id)?;
    if !root_dir.join("record.json").exists() {
        return Err(format!("research run not found: {research_id}"));
    }

    let record = load_record(&root_dir)?;
    let packet = load_packet(&root_dir)?;
    let artifacts = load_artifacts(&root_dir)?;
    let synthesis = load_synthesis(&root_dir)?;

    Ok(ResearchInspect {
        root_dir,
        record,
        packet,
        artifacts,
        synthesis,
    })
}

pub fn write_research_artifact(
    cwd: &Path,
    research_id: &str,
    request: WriteResearchArtifactRequest,
) -> Result<ResearchArtifactWrite, String> {
    if request.title.trim().is_empty() {
        return Err("artifact title must be non-empty".to_string());
    }
    if request.content.trim().is_empty() {
        return Err("artifact content must be non-empty".to_string());
    }

    let root_dir = research_dir(cwd, research_id)?;
    let record = load_record(&root_dir)?;
    if record.status != ResearchStatus::Frozen {
        return Err(format!(
            "research run is not open for artifact writes: {}",
            record.research_id
        ));
    }
    let packet = load_packet(&root_dir)?;
    let artifacts_dir = root_dir.join("artifacts");
    let existing_count = fs::read_dir(&artifacts_dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .count() as u32;
    if existing_count >= packet.budget.max_artifacts {
        return Err(format!(
            "artifact budget exhausted for {} (max_artifacts={})",
            packet.research_id, packet.budget.max_artifacts
        ));
    }

    let artifact = ResearchArtifact {
        research_id: research_id.to_string(),
        kind: request.kind.clone(),
        title: request.title,
        content: request.content,
        evidence_refs: request.evidence_refs,
        created_at: Utc::now(),
    };
    let artifact_name = format!(
        "{}-{}.json",
        artifact.created_at.format("%Y%m%d%H%M%S%3f"),
        artifact_kind_str(&artifact.kind)
    );
    let artifact_path = artifacts_dir.join(artifact_name);
    fs::write(
        &artifact_path,
        serde_json::to_string_pretty(&artifact).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(ResearchArtifactWrite {
        artifact,
        artifact_path,
    })
}

pub fn synthesize_research(
    cwd: &Path,
    research_id: &str,
    request: SynthesizeResearchRequest,
) -> Result<ResearchSynthesisWrite, String> {
    if request.title.trim().is_empty() {
        return Err("synthesis title must be non-empty".to_string());
    }
    if request.findings.is_empty() {
        return Err("at least one finding is required".to_string());
    }

    let root_dir = research_dir(cwd, research_id)?;
    let record_path = root_dir.join("record.json");
    if !record_path.exists() {
        return Err(format!("research run not found: {research_id}"));
    }

    let mut record = load_record(&root_dir)?;
    if record.status != ResearchStatus::Frozen {
        return Err(format!(
            "research run is not open for synthesis: {}",
            record.research_id
        ));
    }

    let synthesis = ResearchSynthesis {
        research_id: research_id.to_string(),
        outcome: request.outcome.clone(),
        title: request.title,
        findings: request.findings,
        recommendations: request.recommendations,
        evidence_refs: request.evidence_refs,
        unresolved_questions: request.unresolved_questions,
        created_at: Utc::now(),
    };
    let synthesis_path = root_dir.join("synthesis.json");
    fs::write(
        &synthesis_path,
        serde_json::to_string_pretty(&synthesis).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    record.status = match synthesis.outcome {
        ResearchOutcome::Escalate => ResearchStatus::Escalated,
        _ => ResearchStatus::Completed,
    };
    record.synthesis_path = Some(synthesis_path.display().to_string());
    record.completed_at = Some(Utc::now());
    save_record(&root_dir, &record)?;

    Ok(ResearchSynthesisWrite {
        synthesis,
        synthesis_path,
        record,
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
        assert_eq!(started.record.synthesis_path, None);
        assert_eq!(started.record.completed_at, None);
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

    #[test]
    fn write_research_artifact_writes_note_json() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::CleanupImpact,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question".into(),
                goal: "Goal".into(),
                constraints: vec![],
                success_criteria: vec!["note".into()],
                budget: ResearchBudget::default(),
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap();

        let written = write_research_artifact(
            &repo,
            &started.record.research_id,
            WriteResearchArtifactRequest {
                kind: ResearchArtifactKind::Note,
                title: "Observed cleanup dependency".into(),
                content: "Module A still imports deprecated helper B.".into(),
                evidence_refs: vec!["receipt:abc".into()],
            },
        )
        .unwrap();

        assert!(written.artifact_path.exists());
        assert_eq!(written.artifact.kind, ResearchArtifactKind::Note);
        assert!(fs::read_to_string(&written.artifact_path)
            .unwrap()
            .contains("Observed cleanup dependency"));
    }

    #[test]
    fn write_research_artifact_respects_max_artifacts_budget() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let mut budget = ResearchBudget::default();
        budget.max_artifacts = 1;
        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::Architecture,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question".into(),
                goal: "Goal".into(),
                constraints: vec![],
                success_criteria: vec!["artifact".into()],
                budget,
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap();

        write_research_artifact(
            &repo,
            &started.record.research_id,
            WriteResearchArtifactRequest {
                kind: ResearchArtifactKind::Hypothesis,
                title: "First".into(),
                content: "One hypothesis".into(),
                evidence_refs: vec![],
            },
        )
        .unwrap();

        let err = write_research_artifact(
            &repo,
            &started.record.research_id,
            WriteResearchArtifactRequest {
                kind: ResearchArtifactKind::Critique,
                title: "Second".into(),
                content: "Another artifact".into(),
                evidence_refs: vec![],
            },
        )
        .unwrap_err();
        assert!(err.contains("artifact budget exhausted"));
    }

    #[test]
    fn synthesize_research_writes_synthesis_and_updates_record() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::Architecture,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question".into(),
                goal: "Goal".into(),
                constraints: vec!["bounded".into()],
                success_criteria: vec!["draft adr".into()],
                budget: ResearchBudget::default(),
                context_refs: vec!["receipt:abc".into()],
                output_schema_ref: None,
            },
        )
        .unwrap();

        let write = synthesize_research(
            &repo,
            &started.record.research_id,
            SynthesizeResearchRequest {
                outcome: ResearchOutcome::AdrDraft,
                title: "Adopt council-first architecture".into(),
                findings: vec!["Existing slices already rely on councils".into()],
                recommendations: vec!["Draft ADR with bounded rollout".into()],
                evidence_refs: vec!["receipt:abc".into()],
                unresolved_questions: vec!["Migration sequencing".into()],
            },
        )
        .unwrap();

        assert!(write.synthesis_path.exists());
        assert_eq!(write.record.status, ResearchStatus::Completed);
        assert_eq!(write.synthesis.outcome, ResearchOutcome::AdrDraft);
        assert!(write.record.synthesis_path.is_some());
        assert!(write.record.completed_at.is_some());
    }

    #[test]
    fn synthesize_research_escalate_marks_record_escalated() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::MigrationRisk,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question".into(),
                goal: "Goal".into(),
                constraints: vec![],
                success_criteria: vec!["risk memo".into()],
                budget: ResearchBudget::default(),
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap();

        let write = synthesize_research(
            &repo,
            &started.record.research_id,
            SynthesizeResearchRequest {
                outcome: ResearchOutcome::Escalate,
                title: "Need deeper migration review".into(),
                findings: vec!["Ambiguity remains in compatibility matrix".into()],
                recommendations: vec![],
                evidence_refs: vec![],
                unresolved_questions: vec!["Which downstream repos depend on v1?".into()],
            },
        )
        .unwrap();

        assert_eq!(write.record.status, ResearchStatus::Escalated);
    }

    #[test]
    fn summarize_research_runs_includes_artifact_and_synthesis_counts() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::SkillImprovement,
                project_id: "specpunk".into(),
                subject_ref: None,
                question: "Question".into(),
                goal: "Goal".into(),
                constraints: vec![],
                success_criteria: vec!["done".into()],
                budget: ResearchBudget::default(),
                context_refs: vec![],
                output_schema_ref: None,
            },
        )
        .unwrap();
        write_research_artifact(
            &repo,
            &started.record.research_id,
            WriteResearchArtifactRequest {
                kind: ResearchArtifactKind::Comparison,
                title: "Compare two paths".into(),
                content: "Path A vs path B".into(),
                evidence_refs: vec![],
            },
        )
        .unwrap();
        synthesize_research(
            &repo,
            &started.record.research_id,
            SynthesizeResearchRequest {
                outcome: ResearchOutcome::CandidatePatch,
                title: "Use candidate patch".into(),
                findings: vec!["A repeatable failure exists".into()],
                recommendations: vec!["Draft skill candidate".into()],
                evidence_refs: vec![],
                unresolved_questions: vec![],
            },
        )
        .unwrap();

        let summaries = summarize_research_runs(&repo).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].artifact_count, 1);
        assert!(summaries[0].has_synthesis);
    }

    #[test]
    fn inspect_research_returns_packet_artifacts_and_synthesis() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        init_repo(&repo);

        let started = start_research(
            &repo,
            StartResearchRequest {
                kind: ResearchKind::Architecture,
                project_id: "specpunk".into(),
                subject_ref: Some("contract:abc".into()),
                question: "Question".into(),
                goal: "Goal".into(),
                constraints: vec!["bounded".into()],
                success_criteria: vec!["adr".into()],
                budget: ResearchBudget::default(),
                context_refs: vec!["receipt:1".into()],
                output_schema_ref: None,
            },
        )
        .unwrap();
        write_research_artifact(
            &repo,
            &started.record.research_id,
            WriteResearchArtifactRequest {
                kind: ResearchArtifactKind::Note,
                title: "Architecture note".into(),
                content: "We already have council primitives.".into(),
                evidence_refs: vec!["receipt:1".into()],
            },
        )
        .unwrap();
        synthesize_research(
            &repo,
            &started.record.research_id,
            SynthesizeResearchRequest {
                outcome: ResearchOutcome::AdrDraft,
                title: "Council-first architecture".into(),
                findings: vec!["Council pattern already exists".into()],
                recommendations: vec!["Write ADR".into()],
                evidence_refs: vec!["receipt:1".into()],
                unresolved_questions: vec!["Migration plan".into()],
            },
        )
        .unwrap();

        let inspect = inspect_research(&repo, &started.record.research_id).unwrap();
        assert_eq!(inspect.packet.question.project_id, "specpunk");
        assert_eq!(inspect.artifacts.len(), 1);
        assert_eq!(inspect.artifacts[0].title, "Architecture note");
        assert_eq!(
            inspect.synthesis.as_ref().unwrap().title,
            "Council-first architecture"
        );
    }
}
