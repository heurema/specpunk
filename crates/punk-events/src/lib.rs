use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use punk_domain::EventEnvelope;
use sha2::{Digest, Sha256};

pub struct EventStore {
    root: PathBuf,
}

impl EventStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.events_dir())?;
        fs::create_dir_all(self.views_dir())?;
        Ok(())
    }

    pub fn append(&self, event: &EventEnvelope) -> Result<()> {
        self.ensure_dirs()?;
        let path = self.month_file();
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        serde_json::to_writer(&mut file, event)?;
        writeln!(&mut file)?;
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<EventEnvelope>> {
        let (events, warnings) = self.load_all_with_warnings()?;
        if !warnings.is_empty() {
            eprintln!(
                "punk: warning: skipped {} malformed event line(s) under {}",
                warnings.len(),
                self.events_dir().display()
            );
            for warning in warnings {
                eprintln!("punk: warning: {warning}");
            }
        }
        Ok(events)
    }

    pub fn load_all_with_warnings(&self) -> Result<(Vec<EventEnvelope>, Vec<String>)> {
        self.ensure_dirs()?;
        let mut entries: Vec<PathBuf> = fs::read_dir(self.events_dir())?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
            .collect();
        entries.sort();
        let mut events = Vec::new();
        let mut warnings = Vec::new();
        for path in entries {
            let file =
                File::open(&path).with_context(|| format!("open event log {}", path.display()))?;
            for (line_no, line) in BufReader::new(file).lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<EventEnvelope>(&line) {
                    Ok(event) => events.push(event),
                    Err(error) => warnings.push(format!(
                        "parse {}:{}: {}",
                        path.display(),
                        line_no + 1,
                        error
                    )),
                }
            }
        }
        Ok((events, warnings))
    }

    pub fn file_sha256(&self, path: impl AsRef<Path>) -> Result<String> {
        let bytes = fs::read(path.as_ref())?;
        Ok(hash_bytes(&bytes))
    }

    pub fn hash_bytes(bytes: &[u8]) -> String {
        hash_bytes(bytes)
    }

    fn month_file(&self) -> PathBuf {
        self.events_dir()
            .join(format!("{}.jsonl", Utc::now().format("%Y-%m")))
    }

    fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }

    fn views_dir(&self) -> PathBuf {
        self.root.join("views")
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use punk_domain::{EventEnvelope, ModeId};

    #[test]
    fn append_and_load_roundtrip() {
        let root = std::env::temp_dir().join(format!("punk-events-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = EventStore::new(&root);
        let event = EventEnvelope {
            event_id: "evt_1".into(),
            ts: "2026-03-29T00:00:00Z".into(),
            project_id: "demo".into(),
            feature_id: None,
            task_id: None,
            run_id: None,
            actor: "operator".into(),
            mode: ModeId::Plot,
            kind: "feature.created".into(),
            payload_ref: None,
            payload_sha256: None,
        };
        store.append(&event).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].event_id, "evt_1");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_all_skips_malformed_lines() {
        let root =
            std::env::temp_dir().join(format!("punk-events-skip-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = EventStore::new(&root);
        store.ensure_dirs().unwrap();
        let path = store.month_file();
        fs::write(
            &path,
            concat!(
                "{\"event_id\":\"evt_ok\",\"ts\":\"2026-03-29T00:00:00Z\",\"project_id\":\"demo\",\"feature_id\":null,\"task_id\":null,\"run_id\":null,\"actor\":\"operator\",\"mode\":\"plot\",\"kind\":\"feature.created\",\"payload_ref\":null,\"payload_sha256\":null}\n",
                "{not-json}\n",
                "{\"event_id\":\"evt_ok_2\",\"ts\":\"2026-03-29T00:00:01Z\",\"project_id\":\"demo\",\"feature_id\":null,\"task_id\":null,\"run_id\":null,\"actor\":\"operator\",\"mode\":\"plot\",\"kind\":\"feature.created\",\"payload_ref\":null,\"payload_sha256\":null}\n"
            ),
        )
        .unwrap();

        let (loaded, warnings) = store.load_all_with_warnings().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("parse"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_all_accepts_legacy_events_without_actor() {
        let root =
            std::env::temp_dir().join(format!("punk-events-legacy-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let store = EventStore::new(&root);
        store.ensure_dirs().unwrap();
        let path = store.month_file();
        fs::write(
            &path,
            "{\"event_id\":\"evt_legacy\",\"ts\":\"2026-03-29T00:00:00Z\",\"project_id\":\"demo\",\"feature_id\":null,\"task_id\":null,\"run_id\":null,\"mode\":\"plot\",\"kind\":\"feature.created\",\"payload_ref\":null,\"payload_sha256\":null}\n",
        )
        .unwrap();

        let (loaded, warnings) = store.load_all_with_warnings().unwrap();
        assert_eq!(warnings.len(), 0);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].actor, "unknown");
        let _ = fs::remove_dir_all(&root);
    }
}
