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
}
