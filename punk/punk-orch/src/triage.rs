/// Infer category from prompt keywords.
pub fn infer_category(prompt: &str) -> &'static str {
    let p = prompt.to_lowercase();

    if p.contains("review") || p.contains("audit") || p.contains("check for") {
        return "review";
    }
    if p.contains("research")
        || p.contains("investigate")
        || p.contains("analyze")
        || p.contains("compare")
        || p.contains("find out")
    {
        return "research";
    }
    if p.contains("fix")
        || p.contains("bug")
        || p.contains("broken")
        || p.contains("error")
        || p.contains("crash")
        || p.contains("fail")
    {
        return "fix";
    }
    if p.contains("write")
        || p.contains("draft")
        || p.contains("blog")
        || p.contains("readme")
        || p.contains("document")
    {
        return "content";
    }
    if p.contains("security") || p.contains("vulnerability") || p.contains("pentest") {
        return "audit";
    }

    "codegen"
}

/// Infer priority from prompt urgency signals.
pub fn infer_priority(prompt: &str) -> &'static str {
    let p = prompt.to_lowercase();

    if p.contains("critical")
        || p.contains("urgent")
        || p.contains("production")
        || p.contains("p0")
        || p.contains("asap")
        || p.contains("hotfix")
        || p.contains("incident")
        || p.contains("outage")
    {
        return "p0";
    }
    if p.contains("cleanup")
        || p.contains("refactor")
        || p.contains("nice to have")
        || p.contains("low priority")
        || p.contains("p2")
        || p.contains("when you can")
    {
        return "p2";
    }

    "p1"
}

/// Suggest agent from agents.toml based on category.
pub fn suggest_agent(
    category: &str,
    agents_config: &std::collections::HashMap<String, crate::config::Agent>,
) -> String {
    // Find agent whose role matches the category
    let role_match = match category {
        "review" | "audit" => "reviewer",
        "research" => "scout",
        "codegen" | "fix" => "engineer",
        "content" => "engineer",
        _ => "engineer",
    };

    // Find matching agent
    for (id, agent) in agents_config {
        if agent.role == role_match {
            return id.clone();
        }
    }

    // Fallback: first claude agent
    for (id, agent) in agents_config {
        if agent.provider == "claude" {
            return id.clone();
        }
    }

    "claude".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_inference() {
        assert_eq!(infer_category("fix the auth token refresh bug"), "fix");
        assert_eq!(
            infer_category("review the pull request for security"),
            "review"
        );
        assert_eq!(infer_category("research alternatives to Redis"), "research");
        assert_eq!(
            infer_category("write a blog post about our architecture"),
            "content"
        );
        assert_eq!(infer_category("implement rate limiting"), "codegen");
        assert_eq!(
            infer_category("check for SQL injection vulnerabilities"),
            "review"
        );
    }

    #[test]
    fn priority_inference() {
        assert_eq!(
            infer_priority("critical: production database is down"),
            "p0"
        );
        assert_eq!(
            infer_priority("refactor the utils module when you can"),
            "p2"
        );
        assert_eq!(infer_priority("add input validation to the API"), "p1");
        assert_eq!(infer_priority("URGENT hotfix for payment flow"), "p0");
    }
}
