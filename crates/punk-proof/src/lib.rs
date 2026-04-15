use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use punk_domain::{
    now_rfc3339, EventEnvelope, ModeId, ProofExecutorIdentity, ProofReproducibilityClaim,
    Proofpack, Receipt, Run,
};
use punk_events::EventStore;
use punk_orch::{find_object_path, project_id, read_json, relative_ref, write_json};

pub struct ProofService {
    repo_root: PathBuf,
    events: EventStore,
}

impl ProofService {
    pub fn new(repo_root: impl AsRef<Path>, global_root: impl AsRef<Path>) -> Self {
        Self {
            repo_root: repo_root.as_ref().to_path_buf(),
            events: EventStore::new(global_root.as_ref()),
        }
    }

    pub fn write_proofpack(&self, run_or_decision_id: &str) -> Result<Proofpack> {
        let decision_path = if run_or_decision_id.starts_with("dec_") {
            find_object_path(&self.repo_root.join(".punk/decisions"), run_or_decision_id)?
        } else {
            let decisions_dir = self.repo_root.join(".punk/decisions");
            let mut found = None;
            for entry in fs::read_dir(&decisions_dir)? {
                let path = entry?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let value: serde_json::Value = read_json(&path)?;
                if value.get("run_id").and_then(|v| v.as_str()) == Some(run_or_decision_id) {
                    found = Some(path);
                    break;
                }
            }
            found.ok_or_else(|| anyhow!("no decision found for run {run_or_decision_id}"))?
        };
        let decision: punk_domain::DecisionObject = read_json(&decision_path)?;
        let receipt_path = self.repo_root.join(&decision.receipt_ref);
        let receipt: Receipt = read_json(&receipt_path)?;
        let run_path = self
            .repo_root
            .join(".punk/runs")
            .join(&decision.run_id)
            .join("run.json");
        let run = if run_path.exists() {
            Some(read_json::<Run>(&run_path)?)
        } else {
            None
        };
        let mut hashes = BTreeMap::new();
        let contract_path = self.repo_root.join(&decision.contract_ref);
        let decision_rel = relative_ref(&self.repo_root, &decision_path)?;
        hashes.insert(
            decision.contract_ref.clone(),
            self.events.file_sha256(&contract_path)?,
        );
        hashes.insert(
            decision.receipt_ref.clone(),
            self.events.file_sha256(&receipt_path)?,
        );
        hashes.insert(
            decision_rel.clone(),
            self.events.file_sha256(&decision_path)?,
        );
        let run_ref = if run_path.exists() {
            let run_ref = relative_ref(&self.repo_root, &run_path)?;
            hashes.insert(run_ref.clone(), self.events.file_sha256(&run_path)?);
            Some(run_ref)
        } else {
            None
        };
        for check_ref in &decision.check_refs {
            hashes.insert(
                check_ref.clone(),
                self.events.file_sha256(self.repo_root.join(check_ref))?,
            );
        }
        if let Some(context_ref) = &decision.verification_context_ref {
            let context_path = self.repo_root.join(context_ref);
            if context_path.exists() {
                hashes.insert(context_ref.clone(), self.events.file_sha256(&context_path)?);
                if let Ok(context) = read_json::<punk_domain::VerificationContext>(&context_path) {
                    if let Some(capability_ref) = context.capability_resolution_ref {
                        let capability_path = self.repo_root.join(&capability_ref);
                        if capability_path.exists() {
                            hashes
                                .insert(capability_ref, self.events.file_sha256(&capability_path)?);
                        }
                    }
                }
            }
        }
        for evidence in &decision.harness_evidence {
            if let Some(source_ref) = &evidence.source_ref {
                hashes.insert(
                    source_ref.clone(),
                    self.events.file_sha256(self.repo_root.join(source_ref))?,
                );
            }
            if let Some(artifact_ref) = &evidence.artifact_ref {
                hashes.insert(
                    artifact_ref.clone(),
                    self.events.file_sha256(self.repo_root.join(artifact_ref))?,
                );
            }
        }
        let executor_identity = Some(ProofExecutorIdentity {
            name: receipt.executor_name.clone(),
            version: None,
        });
        let reproducibility_claim = Some(build_reproducibility_claim(
            run_ref.is_some(),
            decision.verification_context_identity.as_ref(),
            executor_identity
                .as_ref()
                .and_then(|identity| identity.version.as_deref())
                .is_some(),
        ));
        let proofpack = Proofpack {
            id: format!("proof_{}", decision.id.trim_start_matches("dec_")),
            decision_id: decision.id.clone(),
            run_id: decision.run_id.clone(),
            run_ref,
            contract_ref: decision.contract_ref.clone(),
            receipt_ref: decision.receipt_ref.clone(),
            decision_ref: decision_rel.clone(),
            check_refs: decision.check_refs.clone(),
            workspace_lineage: run.as_ref().map(|run| run.vcs.clone()),
            verification_context_ref: decision.verification_context_ref.clone(),
            verification_context_identity: decision.verification_context_identity.clone(),
            executor_identity,
            reproducibility_claim,
            command_evidence: decision.command_evidence.clone(),
            declared_harness_evidence: decision.declared_harness_evidence.clone(),
            harness_evidence: decision.harness_evidence.clone(),
            hashes,
            summary: format!("proof for {}", decision.id),
            created_at: now_rfc3339(),
        };
        let proof_dir = self.repo_root.join(".punk/proofs").join(&decision.id);
        fs::create_dir_all(&proof_dir)?;
        let proof_path = proof_dir.join("proofpack.json");
        write_json(&proof_path, &proofpack)?;
        let event = EventEnvelope {
            event_id: format!("evt_proof_{}", decision.run_id.trim_start_matches("run_")),
            ts: now_rfc3339(),
            project_id: project_id(&self.repo_root)?,
            feature_id: None,
            task_id: None,
            run_id: Some(decision.run_id.clone()),
            actor: "gate".to_string(),
            mode: ModeId::Gate,
            kind: "proofpack.written".to_string(),
            payload_ref: Some(relative_ref(&self.repo_root, &proof_path)?),
            payload_sha256: Some(self.events.file_sha256(&proof_path)?),
        };
        self.events.append(&event)?;
        Ok(proofpack)
    }
}

fn build_reproducibility_claim(
    has_run_ref: bool,
    verification_context_identity: Option<&punk_domain::VerificationContextIdentity>,
    has_executor_version: bool,
) -> ProofReproducibilityClaim {
    let mut limits = vec![
        "v0 proof records verdict context and evidence but does not guarantee hermetic rebuilds"
            .to_string(),
    ];
    if !has_executor_version {
        limits.push("executor version is unavailable in the current receipt schema".to_string());
    }

    match verification_context_identity {
        Some(identity) if has_run_ref => ProofReproducibilityClaim {
            level: "frozen_context_v0".to_string(),
            summary: "Proof records run lineage, executor identity, and the frozen verification context used for the gate verdict.".to_string(),
            environment_digest_sha256: Some(identity.fingerprint_sha256.clone()),
            limits,
        },
        Some(identity) => {
            limits.push("run artifact is unavailable, so execution lineage is incomplete".to_string());
            ProofReproducibilityClaim {
                level: "record_plus_context_v0".to_string(),
                summary: "Proof records executor identity and frozen verification context, but the run artifact is missing.".to_string(),
                environment_digest_sha256: Some(identity.fingerprint_sha256.clone()),
                limits,
            }
        }
        None if has_run_ref => {
            limits.push("frozen verification context identity is unavailable".to_string());
            ProofReproducibilityClaim {
                level: "run_record_v0".to_string(),
                summary: "Proof records run lineage and executor identity, but lacks a frozen verification-context digest.".to_string(),
                environment_digest_sha256: None,
                limits,
            }
        }
        None => {
            limits.push("run artifact is unavailable, so execution lineage is incomplete".to_string());
            limits.push("frozen verification context identity is unavailable".to_string());
            ProofReproducibilityClaim {
                level: "record_only_v0".to_string(),
                summary: "Proof is a hash-linked record bundle without enough execution-context detail for reconstruction.".to_string(),
                environment_digest_sha256: None,
                limits,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_domain::{
        CheckStatus, CommandEvidence, Decision, DecisionObject, DeclaredHarnessEvidence,
        DeterministicStatus, HarnessEvidence, Receipt, ReceiptArtifacts, Run, RunStatus, RunVcs,
        VcsKind, VerificationContext, VerificationContextIdentity,
    };
    use punk_orch::write_json;

    fn normalized_proofpack_value(proofpack: &Proofpack) -> serde_json::Value {
        let mut value = serde_json::to_value(proofpack).unwrap();
        value["created_at"] = serde_json::Value::String("<normalized>".into());
        value
    }

    #[test]
    fn write_proofpack_copies_typed_command_evidence_and_replays_stably() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-proof-evidence-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1/checks")).unwrap();
        fs::create_dir_all(root.join(".punk/decisions")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join(".punk/contracts/feat_1/v1.json"), "{}\n").unwrap();
        fs::write(
            root.join(".punk/runs/run_1/checks/target-01.stdout.log"),
            "ok\n",
        )
        .unwrap();
        fs::write(
            root.join(".punk/runs/run_1/checks/target-01.stderr.log"),
            "",
        )
        .unwrap();
        fs::write(root.join(".punk/project/harness.json"), "{ }\n").unwrap();
        fs::write(root.join("tracked.txt"), "ok\n").unwrap();
        fs::write(
            root.join(".punk/contracts/feat_1/capability-resolution.json"),
            "{ \"schema\": \"specpunk/contract-capability-resolution/v1\" }\n",
        )
        .unwrap();
        write_json(
            &root.join(".punk/runs/run_1/run.json"),
            &Run {
                id: "run_1".into(),
                task_id: "task_1".into(),
                feature_id: "feat_1".into(),
                contract_id: "ct_1".into(),
                attempt: 1,
                status: RunStatus::Finished,
                mode_origin: ModeId::Cut,
                vcs: RunVcs {
                    backend: VcsKind::Git,
                    workspace_ref: root.display().to_string(),
                    change_ref: "HEAD".into(),
                    base_ref: Some("HEAD~1".into()),
                },
                verification_context_ref: None,
                started_at: now_rfc3339(),
                ended_at: Some(now_rfc3339()),
            },
        )
        .unwrap();
        write_json(
            &root.join(".punk/runs/run_1/receipt.json"),
            &Receipt {
                id: "rcpt_1".into(),
                run_id: "run_1".into(),
                task_id: "task_1".into(),
                status: "success".into(),
                executor_name: "fake-executor".into(),
                changed_files: vec!["tracked.txt".into()],
                artifacts: ReceiptArtifacts {
                    stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                    stderr_ref: ".punk/runs/run_1/stderr.log".into(),
                },
                checks_run: vec!["cargo test -p punk-proof".into()],
                duration_ms: 42,
                cost_usd: None,
                summary: "ok".into(),
                created_at: now_rfc3339(),
            },
        )
        .unwrap();
        write_json(
            &root.join(".punk/runs/run_1/verification-context.json"),
            &VerificationContext {
                identity: VerificationContextIdentity {
                    backend: VcsKind::Git,
                    workspace_ref: root.display().to_string(),
                    change_ref: "HEAD".into(),
                    base_ref: Some("HEAD~1".into()),
                    changed_files: vec!["tracked.txt".into()],
                    fingerprint_sha256: "ctx-digest-123".into(),
                },
                file_states: vec![],
                capability_resolution_ref: Some(
                    ".punk/contracts/feat_1/capability-resolution.json".into(),
                ),
                capability_resolution_sha256: Some("cap-digest-123".into()),
                captured_at: now_rfc3339(),
            },
        )
        .unwrap();

        let decision = DecisionObject {
            id: "dec_1".into(),
            run_id: "run_1".into(),
            contract_id: "ct_1".into(),
            decision: Decision::Accept,
            deterministic_status: DeterministicStatus::Pass,
            target_status: CheckStatus::Pass,
            integrity_status: CheckStatus::Pass,
            confidence_estimate: 1.0,
            decision_basis: vec!["target check passed: cargo test -p punk-proof".into()],
            contract_ref: ".punk/contracts/feat_1/v1.json".into(),
            receipt_ref: ".punk/runs/run_1/receipt.json".into(),
            check_refs: vec![
                ".punk/runs/run_1/checks/target-01.stdout.log".into(),
                ".punk/runs/run_1/checks/target-01.stderr.log".into(),
            ],
            verification_context_ref: Some(".punk/runs/run_1/verification-context.json".into()),
            verification_context_identity: Some(VerificationContextIdentity {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "HEAD".into(),
                base_ref: Some("HEAD~1".into()),
                changed_files: vec!["tracked.txt".into()],
                fingerprint_sha256: "ctx-digest-123".into(),
            }),
            command_evidence: vec![CommandEvidence {
                evidence_type: "command".into(),
                lane: "target".into(),
                command: "cargo test -p punk-proof".into(),
                status: CheckStatus::Pass,
                summary: "target check passed: cargo test -p punk-proof".into(),
                stdout_ref: Some(".punk/runs/run_1/checks/target-01.stdout.log".into()),
                stderr_ref: Some(".punk/runs/run_1/checks/target-01.stderr.log".into()),
            }],
            declared_harness_evidence: vec![DeclaredHarnessEvidence {
                evidence_type: "log_query".into(),
                profile: "default".into(),
                source_ref: Some(".punk/project/harness.json".into()),
                summary: "declared harness surface log_query from profile default".into(),
            }],
            harness_evidence: vec![HarnessEvidence {
                evidence_type: "artifact_assertion".into(),
                profile: "default".into(),
                status: CheckStatus::Pass,
                summary: "artifact_assertion passed for profile default: tracked.txt exists".into(),
                source_ref: Some(".punk/project/harness.json".into()),
                artifact_ref: Some("tracked.txt".into()),
            }],
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/decisions/dec_1.json"), &decision).unwrap();

        let service = ProofService::new(&root, &global);
        let proofpack = service.write_proofpack("dec_1").unwrap();

        assert_eq!(
            proofpack.run_ref.as_deref(),
            Some(".punk/runs/run_1/run.json")
        );
        assert_eq!(
            proofpack.workspace_lineage,
            Some(RunVcs {
                backend: VcsKind::Git,
                workspace_ref: root.display().to_string(),
                change_ref: "HEAD".into(),
                base_ref: Some("HEAD~1".into()),
            })
        );
        assert_eq!(
            proofpack.executor_identity,
            Some(ProofExecutorIdentity {
                name: "fake-executor".into(),
                version: None,
            })
        );
        assert_eq!(
            proofpack
                .reproducibility_claim
                .as_ref()
                .map(|claim| claim.level.as_str()),
            Some("frozen_context_v0")
        );
        assert_eq!(
            proofpack
                .reproducibility_claim
                .as_ref()
                .and_then(|claim| claim.environment_digest_sha256.as_deref()),
            Some("ctx-digest-123")
        );
        assert_eq!(proofpack.command_evidence, decision.command_evidence);
        assert_eq!(
            proofpack.declared_harness_evidence,
            decision.declared_harness_evidence
        );
        assert_eq!(proofpack.harness_evidence, decision.harness_evidence);
        assert_eq!(proofpack.check_refs, decision.check_refs);
        assert!(proofpack.hashes.contains_key(".punk/runs/run_1/run.json"));
        assert!(proofpack
            .hashes
            .contains_key(".punk/runs/run_1/verification-context.json"));
        assert!(proofpack
            .hashes
            .contains_key(".punk/contracts/feat_1/capability-resolution.json"));
        assert!(proofpack.hashes.contains_key(".punk/project/harness.json"));
        assert!(proofpack.hashes.contains_key("tracked.txt"));

        std::thread::sleep(std::time::Duration::from_millis(5));
        let replayed = service.write_proofpack("dec_1").unwrap();
        assert_ne!(proofpack.created_at, replayed.created_at);
        assert_eq!(
            normalized_proofpack_value(&proofpack),
            normalized_proofpack_value(&replayed)
        );

        let persisted: Proofpack =
            read_json(&root.join(".punk/proofs/dec_1/proofpack.json")).unwrap();
        assert_eq!(
            normalized_proofpack_value(&replayed),
            normalized_proofpack_value(&persisted)
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn proofpack_hashes_architecture_assessment_artifact() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "punk-proof-architecture-assessment-{}-{suffix}",
            std::process::id()
        ));
        let global = root.join("global");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/contracts/feat_1")).unwrap();
        fs::create_dir_all(root.join(".punk/runs/run_1")).unwrap();
        fs::create_dir_all(root.join(".punk/decisions")).unwrap();
        fs::write(root.join(".punk/contracts/feat_1/v1.json"), "{}\n").unwrap();
        write_json(
            &root.join(".punk/runs/run_1/receipt.json"),
            &Receipt {
                id: "rcpt_1".into(),
                run_id: "run_1".into(),
                task_id: "task_1".into(),
                status: "success".into(),
                executor_name: "fake".into(),
                changed_files: vec!["tracked.txt".into()],
                artifacts: ReceiptArtifacts {
                    stdout_ref: ".punk/runs/run_1/stdout.log".into(),
                    stderr_ref: ".punk/runs/run_1/stderr.log".into(),
                },
                checks_run: vec![],
                duration_ms: 1,
                cost_usd: None,
                summary: "ok".into(),
                created_at: now_rfc3339(),
            },
        )
        .unwrap();
        fs::write(
            root.join(".punk/runs/run_1/architecture-assessment.json"),
            "{\"run_id\":\"run_1\"}\n",
        )
        .unwrap();

        let decision = DecisionObject {
            id: "dec_1".into(),
            run_id: "run_1".into(),
            contract_id: "ct_1".into(),
            decision: Decision::Accept,
            deterministic_status: DeterministicStatus::Pass,
            target_status: CheckStatus::Pass,
            integrity_status: CheckStatus::Pass,
            confidence_estimate: 1.0,
            decision_basis: vec!["checks passed".into()],
            contract_ref: ".punk/contracts/feat_1/v1.json".into(),
            receipt_ref: ".punk/runs/run_1/receipt.json".into(),
            check_refs: vec![".punk/runs/run_1/architecture-assessment.json".into()],
            verification_context_ref: None,
            verification_context_identity: None,
            command_evidence: Vec::new(),
            declared_harness_evidence: Vec::new(),
            harness_evidence: Vec::new(),
            created_at: now_rfc3339(),
        };
        write_json(&root.join(".punk/decisions/dec_1.json"), &decision).unwrap();

        let proof = ProofService::new(&root, &global)
            .write_proofpack("dec_1")
            .unwrap();
        assert!(proof
            .check_refs
            .contains(&".punk/runs/run_1/architecture-assessment.json".to_string()));
        assert!(proof
            .hashes
            .contains_key(".punk/runs/run_1/architecture-assessment.json"));
    }
}
