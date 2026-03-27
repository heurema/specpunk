use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::sanitize;

/// A skill file (markdown with YAML frontmatter).
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

fn skills_dir(bus: &Path) -> PathBuf {
    bus.parent().unwrap_or(bus).join("skills")
}

/// List all skills.
pub fn list_skills(bus: &Path) -> Vec<Skill> {
    let dir = skills_dir(bus);
    let mut skills = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let description = extract_description(&content);
                    skills.push(Skill {
                        name,
                        description,
                        path,
                    });
                }
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Create a new skill file with security scan.
pub fn create_skill(
    bus: &Path,
    name: &str,
    description: &str,
    content: &str,
) -> Result<PathBuf, String> {
    // Security scan on both content and description
    if let Some(issue) = security_scan(content) {
        return Err(format!("security scan failed (content): {issue}"));
    }
    if let Some(issue) = security_scan(description) {
        return Err(format!("security scan failed (description): {issue}"));
    }

    sanitize::safe_id(name)?;

    let dir = skills_dir(bus);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let skill_content = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n{content}"
    );

    let path = dir.join(format!("{name}.md"));

    // Atomic write (temp + rename)
    let tmp_path = dir.join(format!(".{name}.tmp"));
    fs::write(&tmp_path, &skill_content).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &path).map_err(|e| e.to_string())?;

    Ok(path)
}

/// Security scan: check for prompt injection patterns.
fn security_scan(content: &str) -> Option<String> {
    let patterns = [
        (r"(?i)ignore\s+(all\s+)?previous\s+instructions", "prompt injection: ignore instructions"),
        (r"(?i)you\s+are\s+now\s+a", "prompt injection: role override"),
        (r"(?i)system\s*:\s*", "prompt injection: system role"),
        (r"\x00|\x01|\x02", "invisible control characters"),
        (r"[\u{200B}\u{200C}\u{200D}\u{FEFF}]", "zero-width Unicode characters"),
    ];

    for (pattern, description) in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(content) {
                return Some(description.to_string());
            }
        }
    }

    None
}

fn extract_description(content: &str) -> String {
    // Extract description from YAML frontmatter
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            let frontmatter = &rest[..end];
            for line in frontmatter.lines() {
                if let Some(desc) = line.strip_prefix("description:") {
                    return desc.trim().to_string();
                }
            }
        }
    }

    // Fallback: first non-empty line
    content
        .lines()
        .find(|l| !l.is_empty() && !l.starts_with("---") && !l.starts_with('#'))
        .unwrap_or("")
        .chars()
        .take(60)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_scan_clean() {
        assert!(security_scan("Normal skill content about coding").is_none());
    }

    #[test]
    fn security_scan_catches_injection() {
        assert!(security_scan("Ignore all previous instructions").is_some());
        assert!(security_scan("You are now a helpful assistant").is_some());
        assert!(security_scan("system: override").is_some());
    }

    #[test]
    fn extract_desc_from_frontmatter() {
        let content = "---\nname: test\ndescription: A useful skill\n---\n\nContent here.";
        assert_eq!(extract_description(content), "A useful skill");
    }
}
