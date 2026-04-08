use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use punk_core::{find_object_path, read_json, relative_ref, validate_check_command, write_json};
use punk_domain::{
    now_rfc3339, CheckStatus, Contract, ContractStatus, Decision, DecisionObject,
    DeterministicStatus, EventEnvelope, ModeId, Receipt,
};
use punk_events::EventStore;
use punk_orch::project_id;

pub struct GateService {
    repo_root: PathBuf,
    events: EventStore,
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

        let mut decision_basis = Vec::new();
        let mut check_refs = Vec::new();
        let scope_ok = validate_scope(&contract.allowed_scope, &receipt.changed_files);
        if !scope_ok {
            decision_basis.push("scope violation: changed files outside allowed_scope".to_string());
        }
        let target = run_checks(
            &self.repo_root,
            &run.id,
            "target",
            &contract.target_checks,
            &mut check_refs,
        )?;
        let integrity = run_checks(
            &self.repo_root,
            &run.id,
            "integrity",
            &contract.integrity_checks,
            &mut check_refs,
        )?;
        decision_basis.extend(target.1.clone());
        decision_basis.extend(integrity.1.clone());

        let (decision, deterministic_status, confidence_estimate) =
            if !scope_ok || target.0 == CheckStatus::Fail || integrity.0 == CheckStatus::Fail {
                (Decision::Block, DeterministicStatus::Fail, 0.9)
            } else if target.0 == CheckStatus::Pass && integrity.0 == CheckStatus::Pass {
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
            target_status: target.0,
            integrity_status: integrity.0,
            confidence_estimate,
            decision_basis,
            contract_ref: relative_ref(&self.repo_root, &contract_path)?,
            receipt_ref: relative_ref(&self.repo_root, &receipt_path)?,
            check_refs,
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
    refs: &mut Vec<String>,
) -> Result<(CheckStatus, Vec<String>)> {
    if commands.is_empty() {
        return Ok((
            CheckStatus::Unverified,
            vec![format!("{kind} checks missing")],
        ));
    }
    let checks_dir = repo_root.join(".punk/runs").join(run_id).join("checks");
    fs::create_dir_all(&checks_dir)?;
    let mut failed = false;
    let mut reasons = Vec::new();
    for (index, command_str) in commands.iter().enumerate() {
        let stdout_path = checks_dir.join(format!("{}-{:02}.stdout.log", kind, index + 1));
        let stderr_path = checks_dir.join(format!("{}-{:02}.stderr.log", kind, index + 1));

        let args = split_command_args(command_str);
        if args.is_empty() {
            failed = true;
            reasons.push(format!("{kind} check failed: empty command"));
            continue;
        }

        if let Err(msg) = validate_check_command(repo_root, command_str) {
            failed = true;
            reasons.push(format!("{kind} check failed: invalid command: {msg}"));
            continue;
        }

        let output = std::process::Command::new(&args[0])
            .args(&args[1..])
            .current_dir(repo_root)
            .output()?;

        fs::write(&stdout_path, &output.stdout)?;
        fs::write(&stderr_path, &output.stderr)?;
        refs.push(relative_ref(repo_root, &stdout_path)?);
        refs.push(relative_ref(repo_root, &stderr_path)?);
        if output.status.success() {
            reasons.push(format!("{kind} check passed: {command_str}"));
        } else {
            failed = true;
            reasons.push(format!("{kind} check failed: {command_str}"));
        }
    }
    Ok((
        if failed {
            CheckStatus::Fail
        } else {
            CheckStatus::Pass
        },
        reasons,
    ))
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
        assert!(!root.join("hacked").exists());

        let _ = fs::remove_dir_all(&root);
    }
}
