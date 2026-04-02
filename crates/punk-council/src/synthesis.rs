use anyhow::Result;
use punk_domain::council::{CouncilOutcome, CouncilScoreboard, CouncilSynthesis};

// Approved new entry-point file for this bounded contract.
// Path: crates/punk-council/src/synthesis.rs
// Replace this scaffold in place. Do not delete or rename this file.

pub fn synthesize_from_scoreboard(
    council_id: &str,
    scoreboard: &CouncilScoreboard,
) -> Result<CouncilSynthesis> {
    let outcome = choose_outcome(scoreboard);
    let selected_labels = choose_selected_labels(scoreboard, &outcome);
    let confidence = choose_confidence(scoreboard, &outcome);
    let rationale = build_rationale(scoreboard, &outcome, &selected_labels);
    let unresolved_risks = build_unresolved_risks(scoreboard, &selected_labels);

    Ok(CouncilSynthesis {
        council_id: council_id.to_string(),
        outcome,
        selected_labels,
        rationale,
        must_keep: Vec::new(),
        must_fix: Vec::new(),
        unresolved_risks,
        confidence,
    })
}

fn choose_outcome(scoreboard: &CouncilScoreboard) -> CouncilOutcome {
    if scoreboard.top_label.is_none() {
        CouncilOutcome::Escalate
    } else if scoreboard.high_disagreement {
        CouncilOutcome::Escalate
    } else if scoreboard.top_gap.unwrap_or_default() >= 1.0 {
        CouncilOutcome::Leader
    } else if scoreboard.second_label.is_some() {
        CouncilOutcome::Hybrid
    } else {
        CouncilOutcome::Leader
    }
}

fn choose_selected_labels(scoreboard: &CouncilScoreboard, outcome: &CouncilOutcome) -> Vec<String> {
    match outcome {
        CouncilOutcome::Leader => scoreboard.top_label.iter().cloned().collect(),
        CouncilOutcome::Hybrid => scoreboard
            .top_label
            .iter()
            .chain(scoreboard.second_label.iter())
            .cloned()
            .collect(),
        CouncilOutcome::Escalate => scoreboard.top_label.iter().cloned().collect(),
    }
}

fn choose_confidence(scoreboard: &CouncilScoreboard, outcome: &CouncilOutcome) -> f32 {
    let base = match outcome {
        CouncilOutcome::Leader => 0.75,
        CouncilOutcome::Hybrid => 0.6,
        CouncilOutcome::Escalate => 0.35,
    };
    let gap_bonus = scoreboard.top_gap.unwrap_or_default().clamp(0.0, 1.0) * 0.15;
    let disagreement_penalty = if scoreboard.high_disagreement {
        0.2
    } else {
        0.0
    };
    (base + gap_bonus - disagreement_penalty).clamp(0.0, 1.0)
}

fn build_rationale(
    scoreboard: &CouncilScoreboard,
    outcome: &CouncilOutcome,
    selected_labels: &[String],
) -> String {
    let top_label = scoreboard.top_label.as_deref().unwrap_or("none");
    let second_label = scoreboard.second_label.as_deref().unwrap_or("none");
    let gap = scoreboard.top_gap.unwrap_or_default();
    let selection = if selected_labels.is_empty() {
        "none".to_string()
    } else {
        selected_labels.join(", ")
    };

    format!(
        "Outcome {:?} from deterministic scoreboard: top={top_label}, second={second_label}, selected={selection}, gap={gap:.2}, high_disagreement={}.",
        outcome, scoreboard.high_disagreement
    )
}

fn build_unresolved_risks(
    scoreboard: &CouncilScoreboard,
    selected_labels: &[String],
) -> Vec<String> {
    let mut risks = Vec::new();

    if scoreboard.top_label.is_none() {
        risks.push("scoreboard did not produce a leading proposal label".to_string());
    }

    if scoreboard.high_disagreement {
        risks.push("high review disagreement requires human follow-up before acting on the advisory result".to_string());
    }

    if matches!(scoreboard.top_gap, Some(gap) if gap < 1.0) && selected_labels.len() <= 1 {
        risks.push(
            "leader margin is narrow, so downstream approval should verify the selected direction"
                .to_string(),
        );
    }

    risks
}
