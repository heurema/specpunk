use punk_domain::council::{CouncilProposalScore, CouncilReview, CouncilScoreboard};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
struct ProposalAccumulator {
    weighted_score_sum: f32,
    weight_sum: f32,
    raw_review_scores: Vec<f32>,
    blocker_count: u32,
    review_count: u32,
}

pub fn score_reviews(reviews: &[CouncilReview]) -> CouncilScoreboard {
    let mut proposals = BTreeMap::<&str, ProposalAccumulator>::new();

    for review in reviews {
        let review_score = review_mean_score(review);
        let weight = review_weight(review);
        let proposal = proposals.entry(review.proposal_label.as_str()).or_default();

        proposal.weighted_score_sum += review_score * weight;
        proposal.weight_sum += weight;
        proposal.raw_review_scores.push(review_score);
        proposal.blocker_count += review.blockers.len() as u32;
        proposal.review_count += 1;
    }

    let proposal_scores = score_proposals(proposals);
    let top_label = proposal_scores
        .first()
        .map(|score| score.proposal_label.clone());
    let second_label = proposal_scores
        .get(1)
        .map(|score| score.proposal_label.clone());
    let top_gap = top_gap(&proposal_scores);
    let high_disagreement = reviews_have_high_disagreement(reviews);

    CouncilScoreboard {
        proposal_scores,
        top_label,
        second_label,
        top_gap,
        high_disagreement,
    }
}

fn score_proposals(proposals: BTreeMap<&str, ProposalAccumulator>) -> Vec<CouncilProposalScore> {
    let mut proposal_scores: Vec<CouncilProposalScore> = proposals
        .into_iter()
        .map(|(proposal_label, proposal)| CouncilProposalScore {
            proposal_label: proposal_label.to_string(),
            weighted_score: normalize_score(&proposal),
            blocker_count: proposal.blocker_count,
            review_count: proposal.review_count,
        })
        .collect();

    proposal_scores.sort_by(compare_proposal_scores);
    proposal_scores
}

fn review_mean_score(review: &CouncilReview) -> f32 {
    let score_sum: f32 = review
        .criterion_scores
        .values()
        .map(|score| *score as f32)
        .sum();
    let score_count = review.criterion_scores.len();

    if score_count == 0 {
        0.0
    } else {
        score_sum / score_count as f32
    }
}

fn review_weight(review: &CouncilReview) -> f32 {
    let confidence = review.confidence;
    if confidence.is_finite() {
        confidence.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn normalize_score(proposal: &ProposalAccumulator) -> f32 {
    if proposal.weight_sum > 0.0 {
        proposal.weighted_score_sum / proposal.weight_sum
    } else if proposal.review_count > 0 {
        proposal.raw_review_scores.iter().sum::<f32>() / proposal.review_count as f32
    } else {
        0.0
    }
}

fn compare_proposal_scores(
    left: &CouncilProposalScore,
    right: &CouncilProposalScore,
) -> std::cmp::Ordering {
    right
        .weighted_score
        .total_cmp(&left.weighted_score)
        .then_with(|| left.blocker_count.cmp(&right.blocker_count))
        .then_with(|| right.review_count.cmp(&left.review_count))
        .then_with(|| left.proposal_label.cmp(&right.proposal_label))
}

fn top_gap(proposal_scores: &[CouncilProposalScore]) -> Option<f32> {
    match (proposal_scores.first(), proposal_scores.get(1)) {
        (Some(top), Some(second)) => Some(top.weighted_score - second.weighted_score),
        _ => None,
    }
}

fn reviews_have_high_disagreement(reviews: &[CouncilReview]) -> bool {
    let mut per_proposal = BTreeMap::<&str, Vec<&CouncilReview>>::new();

    for review in reviews {
        per_proposal
            .entry(review.proposal_label.as_str())
            .or_default()
            .push(review);
    }

    per_proposal.values().any(|proposal_reviews| {
        let scores: Vec<f32> = proposal_reviews
            .iter()
            .map(|review| review_mean_score(review))
            .collect();
        let has_blocker_conflict = proposal_reviews
            .iter()
            .any(|review| !review.blockers.is_empty())
            && proposal_reviews
                .iter()
                .any(|review| review.blockers.is_empty());

        score_range(&scores) >= 2.0
            || score_standard_deviation(&scores) >= 1.25
            || has_blocker_conflict
    })
}

fn score_range(scores: &[f32]) -> f32 {
    let min = scores.iter().copied().reduce(f32::min).unwrap_or(0.0);
    let max = scores.iter().copied().reduce(f32::max).unwrap_or(0.0);
    max - min
}

fn score_standard_deviation(scores: &[f32]) -> f32 {
    if scores.len() < 2 {
        return 0.0;
    }

    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
    let variance = scores
        .iter()
        .map(|score| {
            let diff = score - mean;
            diff * diff
        })
        .sum::<f32>()
        / scores.len() as f32;

    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_type_name<T, F>(_field: fn(&T) -> &F) -> &'static str {
        std::any::type_name::<F>()
    }

    #[test]
    fn introspect_council_types() {
        println!(
            "CouncilReview.blockers={}",
            field_type_name(|value: &CouncilReview| &value.blockers)
        );
        println!(
            "CouncilReview.confidence={}",
            field_type_name(|value: &CouncilReview| &value.confidence)
        );
        println!(
            "CouncilReview.council_id={}",
            field_type_name(|value: &CouncilReview| &value.council_id)
        );
        println!(
            "CouncilReview.criterion_scores={}",
            field_type_name(|value: &CouncilReview| &value.criterion_scores)
        );
        println!(
            "CouncilReview.findings={}",
            field_type_name(|value: &CouncilReview| &value.findings)
        );
        println!(
            "CouncilReview.proposal_label={}",
            field_type_name(|value: &CouncilReview| &value.proposal_label)
        );
        println!(
            "CouncilReview.reviewer_slot_id={}",
            field_type_name(|value: &CouncilReview| &value.reviewer_slot_id)
        );
        println!(
            "CouncilScoreboard.high_disagreement={}",
            field_type_name(|value: &CouncilScoreboard| &value.high_disagreement)
        );
        println!(
            "CouncilScoreboard.proposal_scores={}",
            field_type_name(|value: &CouncilScoreboard| &value.proposal_scores)
        );
        println!(
            "CouncilScoreboard.second_label={}",
            field_type_name(|value: &CouncilScoreboard| &value.second_label)
        );
        println!(
            "CouncilScoreboard.top_gap={}",
            field_type_name(|value: &CouncilScoreboard| &value.top_gap)
        );
        println!(
            "CouncilScoreboard.top_label={}",
            field_type_name(|value: &CouncilScoreboard| &value.top_label)
        );
        println!(
            "CouncilProposalScore.blocker_count={}",
            field_type_name(|value: &CouncilProposalScore| &value.blocker_count)
        );
        println!(
            "CouncilProposalScore.proposal_label={}",
            field_type_name(|value: &CouncilProposalScore| &value.proposal_label)
        );
        println!(
            "CouncilProposalScore.review_count={}",
            field_type_name(|value: &CouncilProposalScore| &value.review_count)
        );
        println!(
            "CouncilProposalScore.weighted_score={}",
            field_type_name(|value: &CouncilProposalScore| &value.weighted_score)
        );
    }
}
