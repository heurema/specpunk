use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use punk_domain::{now_rfc3339, EventEnvelope, ModeId, Proofpack};
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
        let mut hashes = BTreeMap::new();
        let contract_path = self.repo_root.join(&decision.contract_ref);
        let receipt_path = self.repo_root.join(&decision.receipt_ref);
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
        for check_ref in &decision.check_refs {
            hashes.insert(
                check_ref.clone(),
                self.events.file_sha256(self.repo_root.join(check_ref))?,
            );
        }
        let proofpack = Proofpack {
            id: format!("proof_{}", decision.id.trim_start_matches("dec_")),
            decision_id: decision.id.clone(),
            run_id: decision.run_id.clone(),
            contract_ref: decision.contract_ref.clone(),
            receipt_ref: decision.receipt_ref.clone(),
            decision_ref: decision_rel.clone(),
            check_refs: decision.check_refs.clone(),
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
