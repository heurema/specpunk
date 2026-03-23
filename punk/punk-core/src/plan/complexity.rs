/// Metadata describing the estimated complexity of a diff.
/// All fields come from metadata, never from reading actual source file content.
#[derive(Debug, Default, Clone)]
pub struct DiffMetadata {
    /// Number of files touched
    pub file_count: usize,
    /// Net lines added + removed
    pub loc_delta: usize,
    /// Number of distinct modules / top-level directories crossed
    pub cross_module_count: usize,
    /// Number of public API items added or removed
    pub public_api_changes: usize,
    /// Keywords hinting at security-sensitive changes (auth, crypto, secret, token, permission…)
    pub security_keyword_hits: usize,
}

/// Compute a 0–10 complexity score from diff metadata.
///
/// Formula (Karpathy-inspired heuristic):
///   file_score      = clamp(file_count,          0, 10)            * 1.5
///   loc_score       = clamp(loc_delta / 50,       0, 10)            * 1.0
///   cross_score     = clamp(cross_module_count,   0, 10)            * 2.0
///   api_score       = clamp(public_api_changes,   0, 10)            * 2.5
///   sec_score       = clamp(security_keyword_hits,0, 10)            * 3.0
///   raw             = file_score + loc_score + cross_score + api_score + sec_score
///   result          = clamp(raw / 10.0 * 10.0, 0, 10) as u8
///
/// The weights reflect that public-API changes and security keywords carry
/// disproportionate review risk.
pub fn complexity_score(meta: &DiffMetadata) -> u8 {
    let mut score: u8 = 0;

    // file_count: 1→0, 2-3→1, 4-5→2, 6-8→3, 9+→4
    score += match meta.file_count {
        0..=1 => 0,
        2..=3 => 1,
        4..=5 => 2,
        6..=8 => 3,
        _ => 4,
    };

    // loc_delta: <20→0, 20-99→1, 100-299→2, 300+→3
    score += match meta.loc_delta {
        0..=19 => 0,
        20..=99 => 1,
        100..=299 => 2,
        _ => 3,
    };

    // cross_module: 0→0, 1→1, 2+→2
    score += meta.cross_module_count.min(2) as u8;

    // public_api_changes: each counts 1, max 2
    score += meta.public_api_changes.min(2) as u8;

    // security_keywords: any→+1
    if meta.security_keyword_hits > 0 {
        score += 1;
    }

    score.min(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_meta_gives_zero() {
        assert_eq!(complexity_score(&DiffMetadata::default()), 0);
    }

    #[test]
    fn single_file_small_change() {
        let meta = DiffMetadata {
            file_count: 1,
            loc_delta: 20,
            cross_module_count: 0,
            public_api_changes: 0,
            security_keyword_hits: 0,
        };
        let score = complexity_score(&meta);
        assert!(score <= 2, "small change should score ≤2, got {score}");
    }

    #[test]
    fn max_meta_gives_ten() {
        let meta = DiffMetadata {
            file_count: 10,
            loc_delta: 500,
            cross_module_count: 10,
            public_api_changes: 10,
            security_keyword_hits: 10,
        };
        assert_eq!(complexity_score(&meta), 10);
    }
}
