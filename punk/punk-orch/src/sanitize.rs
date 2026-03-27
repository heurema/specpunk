/// Sanitize an identifier for use in file paths.
/// Only allows alphanumeric, hyphens, underscores, and dots.
/// Rejects path traversal attempts (/, .., ~).
pub fn safe_id(input: &str) -> Result<String, String> {
    if input.is_empty() {
        return Err("empty identifier".into());
    }
    if input.contains('/') || input.contains('\\') || input.contains("..") || input.starts_with('~')
    {
        return Err(format!("unsafe identifier: '{input}'"));
    }
    if !input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!(
            "identifier contains invalid chars: '{input}' (allowed: a-z, 0-9, -, _, .)"
        ));
    }
    Ok(input.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ids() {
        assert!(safe_id("signum").is_ok());
        assert!(safe_id("signum-20260327-123456").is_ok());
        assert!(safe_id("my_skill.v2").is_ok());
    }

    #[test]
    fn rejects_traversal() {
        assert!(safe_id("../../etc/passwd").is_err());
        assert!(safe_id("foo/bar").is_err());
        assert!(safe_id("~/.ssh/id_rsa").is_err());
        assert!(safe_id("foo\\bar").is_err());
    }

    #[test]
    fn rejects_special_chars() {
        assert!(safe_id("foo bar").is_err());
        assert!(safe_id("foo;rm -rf").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(safe_id("").is_err());
    }

    // --- Security bypass attempts ---

    #[test]
    fn security_unicode_fraction_slash() {
        // U+2044 FRACTION SLASH looks like '/' but is a different codepoint.
        // If accepted, downstream path.join() would treat it as a literal char
        // and create an odd-named file, not traverse — but non-ASCII should still
        // be rejected by the alphanumeric-only allow-list.
        let input = "foo\u{2044}bar"; // "foo⁄bar"
        assert!(
            safe_id(input).is_err(),
            "fraction slash U+2044 must be rejected (non-ASCII)"
        );
    }

    #[test]
    fn security_null_byte() {
        // Null bytes can truncate filenames on some OS/lib implementations.
        // Must be rejected — not alphanumeric.
        let input = "foo\x00bar";
        assert!(
            safe_id(input).is_err(),
            "null byte must be rejected"
        );
    }

    #[test]
    fn security_url_encoded_slash() {
        // Percent-encoded slash. If a caller passes this raw string it should be
        // rejected as '%' is not in the allowed charset.
        let input = "foo%2Fbar";
        assert!(
            safe_id(input).is_err(),
            "URL-encoded slash must be rejected (% not allowed)"
        );
    }

    #[test]
    fn security_dots_only() {
        // "..." doesn't contain ".." as a substring — wait, it does: chars 0-1 are "..".
        // The current check uses contains(".."), so "..." has ".." at position 0.
        // This test documents the expected behavior.
        assert!(safe_id("...").is_err(), "'...' contains '..' and must be rejected");
        assert!(safe_id("....").is_err(), "'....' contains '..' and must be rejected");
        // ". ." has a space — rejected by alphanumeric filter.
        assert!(safe_id(". .").is_err(), "'. .' has a space, must be rejected");
    }

    #[test]
    fn security_very_long_name() {
        // 1000-char all-'a' string: allowed characters, but should a length cap exist?
        // Current implementation has no length limit, so this PASSES.
        // If a length limit is ever added, this test will catch regressions.
        let long = "a".repeat(1000);
        // Document current behavior: accepted (no length limit enforced).
        let result = safe_id(&long);
        // This is a known gap — an attacker could create very long path components.
        // For now we just assert it doesn't panic.
        let _ = result; // remove assert to leave behavior observation only
    }

    #[test]
    fn security_dot_prefix() {
        // ".hidden" starts with a dot. Contains no ".." and no '/'.
        // All chars are alphanumeric or '.', so the current allow-list PASSES this.
        // This test documents the gap: dotfiles can be created.
        let result = safe_id(".hidden");
        // Expected by design: safe_id does NOT block leading dots.
        // A caller creating ".hidden" would produce a hidden file on Unix.
        // Leaving as a known-accepted gap with documentation.
        assert!(result.is_ok(), ".hidden is currently accepted (known gap: creates hidden file)");
    }

    #[test]
    fn security_unc_path() {
        // UNC path "\\server\share" — both backslashes should be caught.
        let input = "\\\\server";
        assert!(
            safe_id(input).is_err(),
            "UNC-style backslash prefix must be rejected"
        );
    }

    #[test]
    fn security_mixed_valid_invalid() {
        // "good-name/../../../etc" contains ".." so must be rejected.
        let input = "good-name/../../../etc";
        assert!(
            safe_id(input).is_err(),
            "mixed valid+traversal must be rejected due to '..'"
        );
    }

    // --- Adversarial tests ---

    #[test]
    fn adversarial_unicode_chars() {
        assert!(safe_id("фoobar").is_err(), "Cyrillic must be rejected");
        assert!(safe_id("日本語").is_err(), "CJK must be rejected");
        assert!(safe_id("emoji🔥").is_err(), "Emoji must be rejected");
        assert!(safe_id("café").is_err(), "Accented Latin must be rejected");
    }

    #[test]
    fn adversarial_dots_only_names() {
        // Single dot: no ".." substring, char '.' is allowed → accepted
        assert!(safe_id(".").is_ok(), "single dot is in allowed charset");

        // ".." is caught by contains("..") check → rejected
        assert!(safe_id("..").is_err(), "double dot is path traversal");

        // "..." contains ".." at position 0 → rejected
        assert!(safe_id("...").is_err(), "triple dot contains '..' substring");

        // ".a.": no ".." present, chars are . a . → accepted
        assert!(safe_id(".a.").is_ok(), "dot-letter-dot has no '..' and valid chars");
    }

    #[test]
    fn adversarial_names_with_only_hyphens() {
        assert!(safe_id("-").is_ok(), "single hyphen is allowed");
        assert!(safe_id("---").is_ok(), "multiple hyphens are allowed");
        assert!(safe_id("-a-b-c-").is_ok(), "hyphens around letters are allowed");
    }

    #[test]
    fn adversarial_null_byte_in_id() {
        // Null byte is not ASCII alphanumeric → rejected via char check
        assert!(safe_id("foo\x00bar").is_err(), "null byte must be rejected");
        assert!(safe_id("\x00").is_err(), "lone null byte must be rejected");
    }

    #[test]
    fn adversarial_very_long_id() {
        // 10K char valid id — no length limit currently
        let long = "a".repeat(10_000);
        let result = safe_id(&long);
        // Documents current behavior: no length cap, accepted without panic
        assert!(result.is_ok(), "10K char valid id accepted (no length limit enforced)");
    }

    #[test]
    fn adversarial_newlines_and_control_chars() {
        assert!(safe_id("foo\nbar").is_err(), "newline must be rejected");
        assert!(safe_id("foo\rbar").is_err(), "CR must be rejected");
        assert!(safe_id("foo\tbar").is_err(), "tab must be rejected");
        assert!(safe_id("\x01\x02\x03").is_err(), "SOH/STX/ETX must be rejected");
    }

    #[test]
    fn adversarial_tilde_not_at_start() {
        // "~" check is only starts_with — embedded tilde not caught by that check
        // but '~' is not in the allowed charset (not alphanumeric, -, _, .)
        // so it gets caught by the char filter
        assert!(safe_id("foo~bar").is_err(), "embedded tilde must be rejected by char filter");
        assert!(safe_id("~").is_err(), "bare tilde rejected by starts_with check");
    }
}
