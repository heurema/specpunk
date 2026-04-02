use std::path::Path;

use anyhow::Result;
use punk_core::artifacts::relative_ref;
use punk_domain::council::{CouncilPacket, CouncilRecord};
use punk_domain::{EventEnvelope, ModeId};
use punk_events::EventStore;

const COUNCIL_PROPOSAL_WRITTEN: &str = "council.proposal_written";
const COUNCIL_REVIEW_WRITTEN: &str = "council.review_written";
const COUNCIL_SYNTHESIS_WRITTEN: &str = "council.synthesis_written";

pub fn emit_started(
    events: &EventStore,
    repo_root: &Path,
    packet: &CouncilPacket,
    packet_path: &Path,
) -> Result<()> {
    let payload_ref = relative_ref(repo_root, packet_path)?;
    let payload_sha256 = Some(events.file_sha256(packet_path)?);
    let event = EventEnvelope {
        event_id: format!("evt_council_started_{}", packet.id),
        ts: punk_domain::now_rfc3339(),
        project_id: packet.project_id.clone(),
        feature_id: packet.subject.feature_id.clone(),
        task_id: None,
        run_id: None,
        actor: "operator".to_string(),
        mode: ModeId::Plot,
        kind: "council.started".to_string(),
        payload_ref: Some(payload_ref),
        payload_sha256,
    };
    events.append(&event)
}

pub fn emit_proposal_written(
    events: &EventStore,
    repo_root: &Path,
    packet: &CouncilPacket,
    slot_id: &str,
    proposal_path: &Path,
) -> Result<()> {
    emit_artifact_written(
        events,
        repo_root,
        packet,
        format!("evt_council_proposal_written_{}_{}", packet.id, slot_id),
        COUNCIL_PROPOSAL_WRITTEN,
        proposal_path,
    )
}

pub fn emit_review_written(
    events: &EventStore,
    repo_root: &Path,
    packet: &CouncilPacket,
    slot_id: &str,
    review_path: &Path,
) -> Result<()> {
    emit_artifact_written(
        events,
        repo_root,
        packet,
        format!("evt_council_review_written_{}_{}", packet.id, slot_id),
        COUNCIL_REVIEW_WRITTEN,
        review_path,
    )
}

pub fn emit_synthesis_written(
    events: &EventStore,
    repo_root: &Path,
    packet: &CouncilPacket,
    synthesis_path: &Path,
) -> Result<()> {
    emit_artifact_written(
        events,
        repo_root,
        packet,
        format!("evt_council_synthesis_written_{}", packet.id),
        COUNCIL_SYNTHESIS_WRITTEN,
        synthesis_path,
    )
}

pub fn emit_completed(
    events: &EventStore,
    repo_root: &Path,
    packet: &CouncilPacket,
    record: &CouncilRecord,
    record_path: &Path,
) -> Result<()> {
    let payload_ref = relative_ref(repo_root, record_path)?;
    let payload_sha256 = Some(events.file_sha256(record_path)?);
    let event = EventEnvelope {
        event_id: format!("evt_council_completed_{}", record.id),
        ts: punk_domain::now_rfc3339(),
        project_id: packet.project_id.clone(),
        feature_id: packet.subject.feature_id.clone(),
        task_id: None,
        run_id: None,
        actor: "operator".to_string(),
        mode: ModeId::Plot,
        kind: "council.completed".to_string(),
        payload_ref: Some(payload_ref),
        payload_sha256,
    };
    events.append(&event)
}

fn emit_artifact_written(
    events: &EventStore,
    repo_root: &Path,
    packet: &CouncilPacket,
    event_id: String,
    kind: &str,
    artifact_path: &Path,
) -> Result<()> {
    let payload_ref = relative_ref(repo_root, artifact_path)?;
    let payload_sha256 = Some(events.file_sha256(artifact_path)?);
    let event = EventEnvelope {
        event_id,
        ts: punk_domain::now_rfc3339(),
        project_id: packet.project_id.clone(),
        feature_id: packet.subject.feature_id.clone(),
        task_id: None,
        run_id: None,
        actor: "operator".to_string(),
        mode: ModeId::Plot,
        kind: kind.to_string(),
        payload_ref: Some(payload_ref),
        payload_sha256,
    };
    events.append(&event)
}
