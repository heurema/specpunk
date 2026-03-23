use serde::{Deserialize, Serialize};

use super::complexity::{DiffMetadata, complexity_score};

/// Ceremony level controls how much overhead the plan command applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeremonyLevel {
    /// Score 0–2: receipt-only, no full contract needed.
    Skip,
    /// Score 3–5: lightweight contract, haiku-class model.
    Lightweight,
    /// Score 6–10: full plan with QA heuristic, sonnet/opus model.
    Full,
}

impl std::fmt::Display for CeremonyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CeremonyLevel::Skip => write!(f, "skip"),
            CeremonyLevel::Lightweight => write!(f, "lightweight"),
            CeremonyLevel::Full => write!(f, "full"),
        }
    }
}

/// Map a raw score to a ceremony level.
pub fn ceremony_from_score(score: u8) -> CeremonyLevel {
    match score {
        0..=2 => CeremonyLevel::Skip,
        3..=5 => CeremonyLevel::Lightweight,
        _ => CeremonyLevel::Full,
    }
}

/// Model tier label for routing metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Haiku,
    Sonnet,
    Opus,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTier::Haiku => write!(f, "haiku"),
            ModelTier::Sonnet => write!(f, "sonnet"),
            ModelTier::Opus => write!(f, "opus"),
        }
    }
}

/// Map a ceremony level to a suggested model tier.
/// This is metadata-only in Phase 2; actual dispatch happens in Phase 8+.
pub fn model_tier_for(level: &CeremonyLevel) -> ModelTier {
    match level {
        CeremonyLevel::Skip => ModelTier::Haiku,
        CeremonyLevel::Lightweight => ModelTier::Sonnet,
        CeremonyLevel::Full => ModelTier::Opus,
    }
}

/// Compute ceremony level and model tier from diff metadata.
pub fn detect_ceremony(meta: &DiffMetadata) -> (u8, CeremonyLevel, ModelTier) {
    let score = complexity_score(meta);
    let level = ceremony_from_score(score);
    let tier = model_tier_for(&level);
    (score, level, tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceremony_level_detection() {
        // Score 0-2 → Skip
        let skip_meta = DiffMetadata {
            file_count: 1,
            loc_delta: 10,
            ..Default::default()
        };
        let (score, level, tier) = detect_ceremony(&skip_meta);
        assert!(score <= 2, "expected score ≤2, got {score}");
        assert_eq!(level, CeremonyLevel::Skip);
        assert_eq!(tier, ModelTier::Haiku);

        // Score 3-5 → Lightweight
        let light_meta = DiffMetadata {
            file_count: 3,
            loc_delta: 150,
            cross_module_count: 1,
            ..Default::default()
        };
        let (score, level, tier) = detect_ceremony(&light_meta);
        assert!(
            (3..=5).contains(&score),
            "expected score 3-5, got {score}"
        );
        assert_eq!(level, CeremonyLevel::Lightweight);
        assert_eq!(tier, ModelTier::Sonnet);

        // Score 6-10 → Full
        let full_meta = DiffMetadata {
            file_count: 8,
            loc_delta: 400,
            cross_module_count: 3,
            public_api_changes: 2,
            security_keyword_hits: 1,
            ..Default::default()
        };
        let (score, level, tier) = detect_ceremony(&full_meta);
        assert!(score >= 6, "expected score ≥6, got {score}");
        assert_eq!(level, CeremonyLevel::Full);
        assert_eq!(tier, ModelTier::Opus);
    }
}
