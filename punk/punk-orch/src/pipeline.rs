use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Pipeline opportunity (current state = last event per ID).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: u32,
    pub project: String,
    pub contact: String,
    pub stage: Stage,
    pub next_step: String,
    pub due: String,
    #[serde(default)]
    pub value_usd: Option<u32>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
    Won,
    Lost,
}

fn pipeline_path(bus: &Path) -> PathBuf {
    bus.parent().unwrap_or(bus).join("pipeline.jsonl")
}

/// Load current pipeline state (last event per ID).
pub fn load_pipeline(bus: &Path) -> Vec<Opportunity> {
    let path = pipeline_path(bus);
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut map = std::collections::HashMap::new();
    for line in content.lines() {
        if let Ok(opp) = serde_json::from_str::<Opportunity>(line) {
            map.insert(opp.id, opp);
        }
    }

    let mut opps: Vec<_> = map.into_values().collect();
    opps.sort_by_key(|o| o.id);
    opps
}

/// Append a new event to pipeline.jsonl.
fn append_event(bus: &Path, opp: &Opportunity) -> std::io::Result<()> {
    let path = pipeline_path(bus);
    let line = serde_json::to_string(opp).map_err(std::io::Error::other)?;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "{line}")
        })
}

/// Add a new opportunity.
pub fn add(
    bus: &Path,
    project: &str,
    contact: &str,
    next_step: &str,
    due: &str,
    value_usd: Option<u32>,
) -> std::io::Result<Opportunity> {
    let existing = load_pipeline(bus);
    let next_id = existing.iter().map(|o| o.id).max().unwrap_or(0) + 1;

    let opp = Opportunity {
        id: next_id,
        project: project.to_string(),
        contact: contact.to_string(),
        stage: Stage::Lead,
        next_step: next_step.to_string(),
        due: due.to_string(),
        value_usd,
        updated_at: Utc::now(),
    };

    append_event(bus, &opp)?;
    Ok(opp)
}

/// Advance an opportunity to the next stage.
pub fn advance(bus: &Path, id: u32) -> Result<Opportunity, String> {
    let opps = load_pipeline(bus);
    let mut opp = opps
        .into_iter()
        .find(|o| o.id == id)
        .ok_or_else(|| format!("opportunity {id} not found"))?;

    opp.stage = match opp.stage {
        Stage::Lead => Stage::Qualified,
        Stage::Qualified => Stage::Proposal,
        Stage::Proposal => Stage::Negotiation,
        Stage::Negotiation => Stage::Won,
        Stage::Won => return Err("already won".into()),
        Stage::Lost => return Err("already lost".into()),
    };
    opp.updated_at = Utc::now();

    append_event(bus, &opp).map_err(|e| e.to_string())?;
    Ok(opp)
}

/// Mark as won or lost.
pub fn set_stage(bus: &Path, id: u32, stage: Stage) -> Result<Opportunity, String> {
    let opps = load_pipeline(bus);
    let mut opp = opps
        .into_iter()
        .find(|o| o.id == id)
        .ok_or_else(|| format!("opportunity {id} not found"))?;

    opp.stage = stage;
    opp.updated_at = Utc::now();

    append_event(bus, &opp).map_err(|e| e.to_string())?;
    Ok(opp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pipeline_lifecycle() {
        let tmp = TempDir::new().unwrap();
        let bus = tmp.path().join("bus");
        fs::create_dir_all(&bus).unwrap();

        let opp = add(&bus, "signum", "Alice", "Send intro", "2026-04-01", Some(5000)).unwrap();
        assert_eq!(opp.id, 1);
        assert_eq!(opp.stage, Stage::Lead);

        let opp = advance(&bus, 1).unwrap();
        assert_eq!(opp.stage, Stage::Qualified);

        let opp = advance(&bus, 1).unwrap();
        assert_eq!(opp.stage, Stage::Proposal);

        let pipeline = load_pipeline(&bus);
        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline[0].stage, Stage::Proposal);
    }
}
