use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use punk_core::{find_object_path, read_json, relative_ref, validate_check_command, write_json};
use punk_domain::{
    now_rfc3339, CheckStatus, CommandEvidence, Contract, ContractStatus, Decision, DecisionObject,
    DeclaredHarnessEvidence, DeterministicStatus, EventEnvelope, HarnessEvidence, ModeId, Receipt,
};
use punk_events::EventStore;
use punk_orch::project_id;

pub struct GateService {
    repo_root: PathBuf,
    events: EventStore,
}

struct CheckRunSummary {
    status: CheckStatus,
    reasons: Vec<String>,
    refs: Vec<String>,
    command_evidence: Vec<CommandEvidence>,
}

struct HarnessRunSummary {
    status: CheckStatus,
    reasons: Vec<String>,
    harness_evidence: Vec<HarnessEvidence>,
}

impl GateService {
    pub fn new(repo_root: impl AsRef<Path>, global_root: impl AsRef<Path>) -> Self {
        let repo_root = repo_root.as_ref().to_path_buf();
        let events = EventStore::new(global_root.as_ref());
        Self { repo_root, events }
    }

    pub fn gate_run(&self, run_id: &str) -> Result<DecisionObject> {
        let project = project_id(&self.repo_root)?;
        let run_path = find_object_path(&self.repo_root.join(".punk/runs"), run_id)?;
        let receipt_path = run_path
            .parent()
            .ok_or_else(|| anyhow!("invalid run path"))?
            .join("receipt.json");
        let run: punk_domain::Run = read_json(&run_path)?;
        let receipt: Receipt = read_json(&receipt_path)?;
        let contract_path =
            find_object_path(&self.repo_root.join(".punk/contracts"), &run.contract_id)?;
        let contract: Contract = read_json(&contract_path)?;
        if contract.status != ContractStatus::Approved {
            return Err(anyhow!("gate requires an approved contract"));
        }
        let declared_harness_evidence = load_declared_harness_evidence(&self.repo_root);
        let harness = run_harness_recipes(&self.repo_root)?;

        let mut decision_basis = Vec::new();
        let mut check_refs = Vec::new();
        let mut command_evidence = Vec::new();
        let scope_ok = validate_scope(&contract.allowed_scope, &receipt.changed_files);
        if !scope_ok {
            decision_basis.push("scope violation: changed files outside allowed_scope".to_string());
        }
        let target = run_checks(&self.repo_root, &run.id, "target", &contract.target_checks)?;
        let integrity = run_checks(
            &self.repo_root,
            &run.id,
            "integrity",
            &contract.integrity_checks,
        )?;
        check_refs.extend(target.refs.iter().cloned());
        check_refs.extend(integrity.refs.iter().cloned());
        decision_basis.extend(target.reasons.clone());
        decision_basis.extend(integrity.reasons.clone());
        decision_basis.extend(harness.reasons.clone());
        command_evidence.extend(target.command_evidence);
        command_evidence.extend(integrity.command_evidence);

        let (decision, deterministic_status, confidence_estimate) = if !scope_ok
            || target.status == CheckStatus::Fail
            || integrity.status == CheckStatus::Fail
            || harness.status == CheckStatus::Fail
        {
            (Decision::Block, DeterministicStatus::Fail, 0.9)
        } else if target.status == CheckStatus::Pass && integrity.status == CheckStatus::Pass {
            (Decision::Accept, DeterministicStatus::Pass, 1.0)
        } else {
            (Decision::Escalate, DeterministicStatus::Mixed, 0.5)
        };

        let decision_object = DecisionObject {
            id: format!("dec_{}", run.id.trim_start_matches("run_")),
            run_id: run.id.clone(),
            contract_id: contract.id.clone(),
            decision,
            deterministic_status,
            target_status: target.status,
            integrity_status: integrity.status,
            confidence_estimate,
            decision_basis,
            contract_ref: relative_ref(&self.repo_root, &contract_path)?,
            receipt_ref: relative_ref(&self.repo_root, &receipt_path)?,
            check_refs,
            command_evidence,
            declared_harness_evidence,
            harness_evidence: harness.harness_evidence,
            created_at: now_rfc3339(),
        };
        let decisions_dir = self.repo_root.join(".punk/decisions");
        fs::create_dir_all(&decisions_dir)?;
        let decision_path = decisions_dir.join(format!("{}.json", decision_object.id));
        write_json(&decision_path, &decision_object)?;
        let event = EventEnvelope {
            event_id: format!("evt_decision_{}", run.id.trim_start_matches("run_")),
            ts: now_rfc3339(),
            project_id: project,
            feature_id: Some(run.feature_id.clone()),
            task_id: Some(run.task_id.clone()),
            run_id: Some(run.id.clone()),
            actor: "gate".to_string(),
            mode: ModeId::Gate,
            kind: "decision.written".to_string(),
            payload_ref: Some(relative_ref(&self.repo_root, &decision_path)?),
            payload_sha256: Some(self.events.file_sha256(&decision_path)?),
        };
        self.events.append(&event)?;
        Ok(decision_object)
    }
}

fn validate_scope(allowed_scope: &[String], changed_files: &[String]) -> bool {
    if allowed_scope.is_empty() {
        return false;
    }
    changed_files.iter().all(|file| {
        allowed_scope
            .iter()
            .any(|prefix| file == prefix || file.starts_with(&format!("{prefix}/")))
    })
}

fn run_checks(
    repo_root: &Path,
    run_id: &str,
    kind: &str,
    commands: &[String],
) -> Result<CheckRunSummary> {
    if commands.is_empty() {
        return Ok(CheckRunSummary {
            status: CheckStatus::Unverified,
            reasons: vec![format!("{kind} checks missing")],
            refs: Vec::new(),
            command_evidence: Vec::new(),
        });
    }
    let checks_dir = repo_root.join(".punk/runs").join(run_id).join("checks");
    fs::create_dir_all(&checks_dir)?;
    let mut failed = false;
    let mut reasons = Vec::new();
    let mut refs = Vec::new();
    let mut command_evidence = Vec::new();
    for (index, command_str) in commands.iter().enumerate() {
        let stdout_path = checks_dir.join(format!("{}-{:02}.stdout.log", kind, index + 1));
        let stderr_path = checks_dir.join(format!("{}-{:02}.stderr.log", kind, index + 1));

        let args = split_command_args(command_str);
        if args.is_empty() {
            failed = true;
            let summary = format!("{kind} check failed: empty command");
            reasons.push(summary.clone());
            command_evidence.push(CommandEvidence {
                evidence_type: "command".to_string(),
                lane: kind.to_string(),
                command: command_str.clone(),
                status: CheckStatus::Fail,
                summary,
                stdout_ref: None,
                stderr_ref: None,
            });
            continue;
        }

        if let Err(msg) = validate_check_command(repo_root, command_str) {
            failed = true;
            let summary = format!("{kind} check failed: invalid command: {msg}");
            reasons.push(summary.clone());
            command_evidence.push(CommandEvidence {
                evidence_type: "command".to_string(),
                lane: kind.to_string(),
                command: command_str.clone(),
                status: CheckStatus::Fail,
                summary,
                stdout_ref: None,
                stderr_ref: None,
            });
            continue;
        }

        let output = std::process::Command::new(&args[0])
            .args(&args[1..])
            .current_dir(repo_root)
            .output()?;

        fs::write(&stdout_path, &output.stdout)?;
        fs::write(&stderr_path, &output.stderr)?;
        let stdout_ref = relative_ref(repo_root, &stdout_path)?;
        let stderr_ref = relative_ref(repo_root, &stderr_path)?;
        refs.push(stdout_ref.clone());
        refs.push(stderr_ref.clone());
        let (status, summary) = if output.status.success() {
            (
                CheckStatus::Pass,
                format!("{kind} check passed: {command_str}"),
            )
        } else {
            failed = true;
            (
                CheckStatus::Fail,
                format!("{kind} check failed: {command_str}"),
            )
        };
        reasons.push(summary.clone());
        command_evidence.push(CommandEvidence {
            evidence_type: "command".to_string(),
            lane: kind.to_string(),
            command: command_str.clone(),
            status,
            summary,
            stdout_ref: Some(stdout_ref),
            stderr_ref: Some(stderr_ref),
        });
    }
    Ok(CheckRunSummary {
        status: if failed {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        },
        reasons,
        refs,
        command_evidence,
    })
}

fn load_declared_harness_evidence(repo_root: &Path) -> Vec<DeclaredHarnessEvidence> {
    let harness_spec_path = repo_root.join(".punk/project/harness.json");
    if !harness_spec_path.exists() {
        return Vec::new();
    }
    let spec: serde_json::Value = match read_json(&harness_spec_path) {
        Ok(spec) => spec,
        Err(_) => return Vec::new(),
    };
    let harness_ref = relative_ref(repo_root, &harness_spec_path)
        .unwrap_or_else(|_| ".punk/project/harness.json".to_string());
    let mut pairs = BTreeSet::new();
    let Some(profiles) = spec.get("profiles").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    for profile in profiles {
        let Some(profile_name) = profile.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(validation_surfaces) = profile
            .get("validation_surfaces")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        for surface in validation_surfaces {
            let Some(surface_name) = surface.as_str() else {
                continue;
            };
            if surface_name == "command" {
                continue;
            }
            pairs.insert((profile_name.to_string(), surface_name.to_string()));
        }
    }
    pairs
        .into_iter()
        .map(|(profile, evidence_type)| DeclaredHarnessEvidence {
            summary: format!("declared harness surface {evidence_type} from profile {profile}"),
            evidence_type,
            profile,
            source_ref: Some(harness_ref.clone()),
        })
        .collect()
}

fn run_harness_recipes(repo_root: &Path) -> Result<HarnessRunSummary> {
    let harness_spec_path = repo_root.join(".punk/project/harness.json");
    if !harness_spec_path.exists() {
        return Ok(HarnessRunSummary {
            status: CheckStatus::Unverified,
            reasons: Vec::new(),
            harness_evidence: Vec::new(),
        });
    }
    let spec: serde_json::Value = read_json(&harness_spec_path)?;
    let harness_ref = relative_ref(repo_root, &harness_spec_path)
        .unwrap_or_else(|_| ".punk/project/harness.json".to_string());
    let Some(profiles) = spec.get("profiles").and_then(|value| value.as_array()) else {
        return Ok(HarnessRunSummary {
            status: CheckStatus::Unverified,
            reasons: Vec::new(),
            harness_evidence: Vec::new(),
        });
    };

    let mut failed = false;
    let mut executed = false;
    let mut reasons = Vec::new();
    let mut harness_evidence = Vec::new();

    for profile in profiles {
        let Some(profile_name) = profile.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(validation_recipes) = profile
            .get("validation_recipes")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        for recipe in validation_recipes {
            let Some(kind) = recipe.get("kind").and_then(|value| value.as_str()) else {
                continue;
            };
            if kind != "artifact_assertion" {
                continue;
            }
            let Some(path) = recipe.get("path").and_then(|value| value.as_str()) else {
                continue;
            };
            executed = true;
            let (status, summary, artifact_ref) =
                run_artifact_assertion(repo_root, profile_name, path)?;
            if status == CheckStatus::Fail {
                failed = true;
            }
            reasons.push(summary.clone());
            harness_evidence.push(HarnessEvidence {
                evidence_type: "artifact_assertion".to_string(),
                profile: profile_name.to_string(),
                status,
                summary,
                source_ref: Some(harness_ref.clone()),
                artifact_ref,
            });
        }
    }

    Ok(HarnessRunSummary {
        status: if failed {
            CheckStatus::Fail
        } else if executed {
            CheckStatus::Pass
        } else {
            CheckStatus::Unverified
        },
        reasons,
        harness_evidence,
    })
}

fn run_artifact_assertion(
    repo_root: &Path,
    profile_name: &str,
    path: &str,
) -> Result<(CheckStatus, String, Option<String>)> {
    if !is_safe_repo_relative_path(path) {
        return Ok((
            CheckStatus::Fail,
            format!(
                "artifact_assertion failed for profile {profile_name}: invalid repo-relative path {path}"
            ),
            None,
        ));
    }
    let artifact_path = repo_root.join(path);
    if artifact_path.exists() {
        let artifact_ref = relative_ref(repo_root, &artifact_path).ok();
        Ok((
            CheckStatus::Pass,
            format!("artifact_assertion passed for profile {profile_name}: {path} exists"),
            artifact_ref,
        ))
    } else {
        Ok((
            CheckStatus::Fail,
            format!("artifact_assertion failed for profile {profile_name}: {path} is missing"),
            None,
        ))
    }
}

fn is_safe_repo_relative_path(path: &str) -> bool {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return false;
    }
    candidate.components().all(|component| {
        !matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

fn split_command_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_core::write_json;
    use punk_domain::{ReceiptArtifacts, RunStatus, VcsKind};

    #[test]
    fn gate_blocks_scope_violation() {
        let root = std::env::temp_dir().join(format!("punk-gate-{}", std::process::id()));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["allowed.txt".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["not-allowed.txt".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.command_evidence.len(), 2);
        assert!(decision
            .command_evidence
            .iter()
            .all(|item| item.evidence_type == "command"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gate_persists_declared_non_command_harness_evidence_from_packet() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-harness-declared-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        fs::create_dir_all(root.join("artifacts")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(root.join("artifacts/result.txt"), "ok\n").unwrap();
        fs::write(
            root.join(".punk/project/harness.json"),
            r#"{
  "project_id": "demo",
  "inspect_ready": true,
  "bootable_per_workspace": true,
  "capabilities": {
    "ui_legible": true,
    "logs_legible": true,
    "metrics_legible": false,
    "traces_legible": false
  },
  "profiles": [
    {
      "name": "default",
      "validation_surfaces": ["command", "ui_snapshot", "log_query"],
      "validation_recipes": [
        {
          "kind": "artifact_assertion",
          "path": "artifacts/result.txt"
        }
      ]
    }
  ],
  "derivation_source": "repo_markers_v1",
  "updated_at": "2026-04-08T00:00:00Z"
}"#,
        )
        .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["src".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Accept);
        assert_eq!(
            decision.declared_harness_evidence,
            vec![
                DeclaredHarnessEvidence {
                    evidence_type: "log_query".into(),
                    profile: "default".into(),
                    source_ref: Some(".punk/project/harness.json".into()),
                    summary: "declared harness surface log_query from profile default".into(),
                },
                DeclaredHarnessEvidence {
                    evidence_type: "ui_snapshot".into(),
                    profile: "default".into(),
                    source_ref: Some(".punk/project/harness.json".into()),
                    summary: "declared harness surface ui_snapshot from profile default".into(),
                },
            ]
        );
        assert_eq!(
            decision.harness_evidence,
            vec![HarnessEvidence {
                evidence_type: "artifact_assertion".into(),
                profile: "default".into(),
                status: CheckStatus::Pass,
                summary:
                    "artifact_assertion passed for profile default: artifacts/result.txt exists"
                        .into(),
                source_ref: Some(".punk/project/harness.json".into()),
                artifact_ref: Some("artifacts/result.txt".into()),
            }]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn failing_artifact_assertion_blocks_gate() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-harness-artifact-block-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        fs::write(
            root.join(".punk/project/harness.json"),
            r#"{
  "project_id": "demo",
  "profiles": [
    {
      "name": "default",
      "validation_surfaces": ["command"],
      "validation_recipes": [
        {
          "kind": "artifact_assertion",
          "path": "artifacts/missing.txt"
        }
      ]
    }
  ]
}"#,
        )
        .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["src".into()],
            target_checks: vec!["true".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec![],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();
        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();
        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.harness_evidence.len(), 1);
        assert_eq!(decision.harness_evidence[0].status, CheckStatus::Fail);
        assert!(decision.decision_basis.iter().any(|reason| {
            reason.contains(
                "artifact_assertion failed for profile default: artifacts/missing.txt is missing",
            )
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_scope_rejects_string_prefix_escape() {
        assert!(validate_scope(
            &["src/lib.rs".into(), "foo".into()],
            &["src/lib.rs".into(), "foo/bar.rs".into()]
        ));
        assert!(!validate_scope(
            &["src/lib.rs".into()],
            &["src/lib.rs.bak".into()]
        ));
        assert!(!validate_scope(&["foo".into()], &["foobar".into()]));
    }

    #[test]
    fn gate_blocks_invalid_check_command_without_running_shell_payload() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-gate-invalid-check-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&root)
            .output()
            .unwrap();
        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["allowed.txt".into()],
            target_checks: vec!["true; touch hacked".into()],
            integrity_checks: vec!["true".into()],
            risk_level: "low".into(),
            created_at: now_rfc3339(),
            approved_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/contracts/feat_1/v1.json"), &contract).unwrap();
        let run = punk_domain::Run {
            id: "run_1".into(),
            task_id: "task_1".into(),
            feature_id: "feat_1".into(),
            contract_id: "ct_1".into(),
            attempt: 1,
            status: RunStatus::Finished,
            mode_origin: ModeId::Cut,
            vcs: punk_domain::RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "head".into(),
                base_ref: None,
            },
            started_at: now_rfc3339(),
            ended_at: Some(now_rfc3339()),
        };
        write_json(&root.join(".punk/runs/run_1/run.json"), &run).unwrap();
        let receipt = Receipt {
            id: "rcpt_1".into(),
            run_id: "run_1".into(),
            task_id: "task_1".into(),
            status: "success".into(),
            executor_name: "fake".into(),
            changed_files: vec!["allowed.txt".into()],
            artifacts: ReceiptArtifacts {
                stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                stderr_ref: ".punk/runs/run_1/stderr.log".into(),
            },
            checks_run: vec![],
            duration_ms: 1,
            cost_usd: None,
            summary: "done".into(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/runs/run_1/receipt.json"), &receipt).unwrap();

        let gate = GateService::new(&root, &global);
        let decision = gate.gate_run("run_1").unwrap();

        assert_eq!(decision.decision, Decision::Block);
        assert_eq!(decision.target_status, CheckStatus::Fail);
        assert!(decision
            .decision_basis
            .iter()
            .any(|reason| reason.contains("invalid command")));
        let invalid_command = decision
            .command_evidence
            .iter()
            .find(|item| item.command == "true; touch hacked")
            .expect("invalid target command evidence");
        assert_eq!(invalid_command.lane, "target");
        assert_eq!(invalid_command.status, CheckStatus::Fail);
        assert!(invalid_command.stdout_ref.is_none());
        assert!(invalid_command.stderr_ref.is_none());
        assert!(!root.join("hacked").exists());

        let _ = fs::remove_dir_all(&root);
    }
}
