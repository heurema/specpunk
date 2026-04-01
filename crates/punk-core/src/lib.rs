use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use punk_domain::{DraftProposal, DraftValidationError, RepoScanSummary};

pub mod artifacts;

pub use artifacts::{find_object_path, read_json, relative_ref, write_json};

pub fn scan_repo(repo_root: &Path, prompt: &str) -> Result<RepoScanSummary> {
    let mut manifests = Vec::new();
    let mut package_manager = None;
    let mut available_scripts = BTreeMap::new();
    let mut candidate_integrity_checks = Vec::new();
    let mut candidate_target_checks = Vec::new();
    let mut notes = Vec::new();
    let tokens = prompt_tokens(prompt);
    let project_kind = if repo_root.join("Cargo.toml").exists() {
        manifests.push("Cargo.toml".to_string());
        candidate_integrity_checks.extend(rust_integrity_checks(repo_root)?);
        candidate_target_checks.extend(rust_target_checks(repo_root, &tokens));
        "rust".to_string()
    } else if repo_root.join("package.json").exists() {
        manifests.push("package.json".to_string());
        let package_json = repo_root.join("package.json");
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(&package_json).with_context(|| format!("read {}", package_json.display()))?,
        )
        .with_context(|| format!("parse {}", package_json.display()))?;
        if let Some(scripts) = value.get("scripts").and_then(|v| v.as_object()) {
            available_scripts = scripts
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_string())))
                .collect();
        }
        package_manager = detect_package_manager(repo_root);
        node_checks(
            package_manager.as_deref(),
            &available_scripts,
            &tokens,
            &mut candidate_target_checks,
            &mut candidate_integrity_checks,
        );
        "node".to_string()
    } else {
        "generic".to_string()
    };

    if repo_root.join("Makefile").exists() {
        manifests.push("Makefile".to_string());
        let makefile = fs::read_to_string(repo_root.join("Makefile"))?;
        if makefile
            .lines()
            .any(|line| line.trim_start().starts_with("test:"))
        {
            let command = "make test".to_string();
            if !candidate_integrity_checks.contains(&command) {
                candidate_integrity_checks.push(command.clone());
            }
            if !candidate_target_checks.contains(&command) {
                candidate_target_checks.push(command);
            }
        }
    }

    dedupe(&mut candidate_integrity_checks);
    dedupe(&mut candidate_target_checks);

    if candidate_integrity_checks.is_empty() {
        notes.push("no trustworthy integrity checks inferred".to_string());
    }

    let candidate_scope_paths = collect_scope_candidates(repo_root, prompt)?;
    let candidate_entry_points = infer_entry_points(repo_root, prompt, &candidate_scope_paths);

    Ok(RepoScanSummary {
        project_kind,
        manifests,
        package_manager,
        available_scripts,
        candidate_entry_points,
        candidate_scope_paths,
        candidate_target_checks,
        candidate_integrity_checks,
        notes,
    })
}

pub fn validate_draft_proposal(
    repo_root: &Path,
    proposal: &DraftProposal,
) -> Vec<DraftValidationError> {
    let mut errors = Vec::new();
    if proposal.title.trim().is_empty() {
        errors.push(error("title", "must be non-empty"));
    }
    if proposal.summary.trim().is_empty() {
        errors.push(error("summary", "must be non-empty"));
    }
    if proposal.behavior_requirements.is_empty() {
        errors.push(error("behavior_requirements", "must be non-empty"));
    }
    if proposal.allowed_scope.is_empty() {
        errors.push(error("allowed_scope", "must be non-empty"));
    }
    if proposal.target_checks.is_empty() {
        errors.push(error("target_checks", "must be non-empty"));
    }
    if proposal.integrity_checks.is_empty() {
        errors.push(error("integrity_checks", "must be non-empty"));
    }

    for (idx, path) in proposal.allowed_scope.iter().enumerate() {
        if let Err(message) = validate_scope_path(repo_root, path) {
            errors.push(error(&format!("allowed_scope[{idx}]"), &message));
        }
        if is_artifact_storage_pattern(path) {
            errors.push(error(
                &format!("allowed_scope[{idx}]"),
                "must not use artifact storage or placeholder paths",
            ));
        }
    }

    for (idx, path) in proposal.entry_points.iter().enumerate() {
        if let Err(message) = validate_scope_path(repo_root, path) {
            errors.push(error(&format!("entry_points[{idx}]"), &message));
        }
        if is_artifact_storage_pattern(path) {
            errors.push(error(
                &format!("entry_points[{idx}]"),
                "must not use artifact storage or placeholder paths",
            ));
        }
    }

    for (idx, command) in proposal
        .target_checks
        .iter()
        .chain(proposal.integrity_checks.iter())
        .enumerate()
    {
        if let Err(message) = validate_check_command(repo_root, command) {
            errors.push(error(&format!("check[{idx}]"), &message));
        }
    }

    if claims_file_level_work(repo_root, &proposal.allowed_scope)
        && proposal.entry_points.is_empty()
    {
        errors.push(error(
            "entry_points",
            "must be non-empty for file-level proposals",
        ));
    }

    for (idx, entry_point) in proposal.entry_points.iter().enumerate() {
        if !scope_covers_path(&proposal.allowed_scope, entry_point) {
            errors.push(error(
                &format!("entry_points[{idx}]"),
                "must be covered by allowed_scope",
            ));
        }
    }

    errors
}

pub fn build_bounded_fallback_proposal(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
    errors: &[DraftValidationError],
) -> Option<DraftProposal> {
    if !is_low_risk_bounded_candidate(proposal) {
        return None;
    }
    if !has_structural_invalidity(repo_root, prompt, proposal, errors) {
        return None;
    }

    let mut allowed_scope = fallback_source_paths(repo_root, prompt, proposal, scan);
    if allowed_scope.is_empty() {
        return None;
    }

    stable_dedupe(&mut allowed_scope);
    if allowed_scope
        .iter()
        .any(|path| is_artifact_storage_pattern(path))
    {
        return None;
    }

    let entry_points = allowed_scope
        .iter()
        .filter(|path| is_file_like_scope(path))
        .cloned()
        .collect::<Vec<_>>();
    if entry_points.is_empty() {
        return None;
    }

    let target_checks = fallback_target_checks(repo_root, prompt, proposal, scan);
    let integrity_checks = fallback_integrity_checks(repo_root, prompt, proposal, scan);
    if target_checks.is_empty() || integrity_checks.is_empty() {
        return None;
    }

    let mut fallback = proposal.clone();
    fallback.allowed_scope = allowed_scope;
    fallback.entry_points = entry_points;
    fallback.target_checks = target_checks;
    fallback.integrity_checks = integrity_checks;
    canonicalize_draft_proposal(repo_root, prompt, &mut fallback);
    Some(fallback)
}

pub fn canonicalize_draft_proposal(repo_root: &Path, prompt: &str, proposal: &mut DraftProposal) {
    let explicit_scope = explicit_scope_paths(prompt, repo_root);
    if !explicit_scope.is_empty() {
        proposal.allowed_scope = explicit_scope.clone();
        let explicit_entry_points = explicit_scope
            .iter()
            .filter(|path| is_file_like_scope(path))
            .cloned()
            .collect::<Vec<_>>();
        if !explicit_entry_points.is_empty() {
            proposal.entry_points = explicit_entry_points;
        }
    } else {
        stable_dedupe(&mut proposal.allowed_scope);
        stable_dedupe(&mut proposal.entry_points);
    }

    let explicit_target_checks = explicit_checks_from_prompt(
        repo_root,
        prompt,
        &[
            "target checks should include",
            "target checks to satisfy",
            "target checks:",
        ],
    );
    if !explicit_target_checks.is_empty() {
        proposal.target_checks = explicit_target_checks;
    } else {
        stable_dedupe(&mut proposal.target_checks);
    }

    let explicit_integrity_checks = explicit_checks_from_prompt(
        repo_root,
        prompt,
        &[
            "integrity checks should include",
            "integrity checks to keep passing",
            "integrity checks:",
        ],
    );
    if !explicit_integrity_checks.is_empty() {
        proposal.integrity_checks = explicit_integrity_checks;
    } else {
        stable_dedupe(&mut proposal.integrity_checks);
    }
}

fn error(field: &str, message: &str) -> DraftValidationError {
    DraftValidationError {
        field: field.to_string(),
        message: message.to_string(),
    }
}

fn detect_package_manager(repo_root: &Path) -> Option<String> {
    for (file, pm) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
        ("package-lock.json", "npm"),
    ] {
        if repo_root.join(file).exists() {
            return Some(pm.to_string());
        }
    }
    Some("npm".to_string())
}

fn package_manager_run(pm: Option<&str>, script: &str) -> String {
    match pm.unwrap_or("npm") {
        "pnpm" => format!("pnpm {script}"),
        "yarn" => format!("yarn {script}"),
        "bun" => format!("bun run {script}"),
        _ => format!("npm run {script}"),
    }
}

fn node_checks(
    package_manager: Option<&str>,
    scripts: &BTreeMap<String, String>,
    prompt_tokens: &[String],
    target: &mut Vec<String>,
    integrity: &mut Vec<String>,
) {
    for script in ["test", "lint", "typecheck", "check"] {
        if scripts.contains_key(script) {
            integrity.push(package_manager_run(package_manager, script));
        }
    }
    for script in scripts.keys() {
        if prompt_tokens.iter().any(|token| script.contains(token)) {
            target.push(package_manager_run(package_manager, script));
        }
    }
    if target.is_empty() {
        if scripts.contains_key("test") {
            target.push(package_manager_run(package_manager, "test"));
        } else if let Some(script) = scripts.keys().next() {
            target.push(package_manager_run(package_manager, script));
        }
    }
}

fn collect_scope_candidates(repo_root: &Path, prompt: &str) -> Result<Vec<String>> {
    let mut candidates = BTreeMap::new();
    let tokens = prompt_tokens(prompt);
    let mut top_level = Vec::new();
    for entry in fs::read_dir(repo_root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_name(&name) || ignored_relative_path(Path::new(&name)) {
            continue;
        }
        top_level.push(name.clone());
        if path.is_dir() {
            candidates.insert(name, 1);
        } else {
            candidates.insert(name, 1);
        }
    }

    walk_repo(repo_root, repo_root, &tokens, &mut candidates)?;

    if candidates.is_empty() {
        for name in top_level {
            candidates.insert(name, 1);
        }
    }

    let mut items: Vec<(String, i32)> = candidates.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(items.into_iter().map(|(path, _)| path).take(20).collect())
}

fn walk_repo(
    repo_root: &Path,
    current: &Path,
    tokens: &[String],
    candidates: &mut BTreeMap<String, i32>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_name(&name) {
            continue;
        }
        let relative_path = path
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("path escaped repo root"))?;
        if ignored_relative_path(relative_path) {
            continue;
        }
        if path.is_dir() {
            walk_repo(repo_root, &path, tokens, candidates)?;
            continue;
        }
        let relative = relative_path.to_string_lossy().to_string();
        let score = relevance_score(&relative, tokens);
        if score > 0 {
            candidates
                .entry(relative.clone())
                .and_modify(|value| *value = (*value).max(score))
                .or_insert(score);
            if let Some(parent) = Path::new(&relative).parent() {
                let parent = parent.to_string_lossy().to_string();
                if !parent.is_empty() && parent != "." {
                    let parent_score = score.saturating_sub(1).max(1);
                    candidates
                        .entry(parent)
                        .and_modify(|value| *value = (*value).max(parent_score))
                        .or_insert(parent_score);
                }
            }
        }
    }
    Ok(())
}

fn ignored_name(name: &str) -> bool {
    matches!(name, ".git" | ".punk" | "target" | "node_modules")
}

fn ignored_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .take(3)
        .map(component_to_string)
        .collect::<Vec<_>>();
    components.starts_with(&["docs".to_string(), "reference-repos".to_string()])
        || components.starts_with(&[
            "docs".to_string(),
            "research".to_string(),
            "_delve_runs".to_string(),
        ])
}

fn prompt_tokens(prompt: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    for token in prompt
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 3)
    {
        seen.insert(token);
    }
    seen.into_iter().collect()
}

fn infer_entry_points(repo_root: &Path, prompt: &str, candidates: &[String]) -> Vec<String> {
    let lowered = prompt.to_ascii_lowercase();
    let mut results = Vec::new();
    for candidate in candidates {
        let path = repo_root.join(candidate);
        if path.is_file() && lowered.contains(&file_stem(candidate)) {
            results.push(candidate.clone());
        }
    }
    if results.is_empty() {
        for fallback in ["src", "src/main.rs", "src/lib.rs", "lib", "app", "server"] {
            let path = repo_root.join(fallback);
            if path.exists() {
                results.push(fallback.to_string());
            }
        }
    }
    results.into_iter().take(10).collect()
}

fn rust_target_checks(repo_root: &Path, prompt_tokens: &[String]) -> Vec<String> {
    let mut checks = Vec::new();
    let mut matched_packages = matched_rust_packages(repo_root, prompt_tokens);
    for package in matched_packages.drain(..) {
        checks.push(format!("cargo test -p {package}"));
    }
    if repo_root.join("src/lib.rs").exists() {
        checks.push("cargo test --lib".to_string());
    }
    if repo_root.join("tests").exists() {
        checks.push("cargo test --tests".to_string());
    }
    if checks.is_empty() {
        checks.push("cargo test".to_string());
    }
    dedupe(&mut checks);
    checks
}

fn rust_integrity_checks(repo_root: &Path) -> Result<Vec<String>> {
    let cargo_toml = repo_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("read {}", cargo_toml.display()))?;
    if contents.lines().any(|line| line.trim() == "[workspace]") {
        Ok(vec!["cargo test --workspace".to_string()])
    } else {
        Ok(vec!["cargo test".to_string()])
    }
}

fn matched_rust_packages(repo_root: &Path, prompt_tokens: &[String]) -> Vec<String> {
    let prompt_token_set: BTreeSet<&str> = prompt_tokens.iter().map(String::as_str).collect();
    let Ok(package_names) = rust_package_names(repo_root) else {
        return Vec::new();
    };
    package_names
        .into_iter()
        .filter(|package_name| {
            package_tokens(package_name)
                .iter()
                .any(|token| prompt_token_set.contains(token.as_str()))
        })
        .collect()
}

fn rust_package_names(repo_root: &Path) -> Result<Vec<String>> {
    let mut package_names = Vec::new();
    collect_rust_package_names(repo_root, repo_root, &mut package_names)?;
    dedupe(&mut package_names);
    Ok(package_names)
}

fn collect_rust_package_names(
    repo_root: &Path,
    current: &Path,
    package_names: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_name(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("path escaped repo root"))?;
        if ignored_relative_path(relative) {
            continue;
        }
        if path.is_dir() {
            collect_rust_package_names(repo_root, &path, package_names)?;
            continue;
        }
        if name != "Cargo.toml" {
            continue;
        }
        if let Some(package_name) = cargo_package_name(&path)? {
            package_names.push(package_name);
        }
    }
    Ok(())
}

fn cargo_package_name(path: &Path) -> Result<Option<String>> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut in_package_section = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package_section = trimmed == "[package]";
            continue;
        }
        if !in_package_section || !trimmed.starts_with("name") {
            continue;
        }
        let Some((_, value)) = trimmed.split_once('=') else {
            continue;
        };
        let name = value.trim().trim_matches('"');
        if !name.is_empty() {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

fn package_tokens(package_name: &str) -> Vec<String> {
    package_name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 3)
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn component_to_string(component: Component<'_>) -> String {
    component.as_os_str().to_string_lossy().to_string()
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase()
}

fn claims_file_level_work(repo_root: &Path, allowed_scope: &[String]) -> bool {
    allowed_scope.iter().any(|scope| {
        let path = repo_root.join(scope);
        path.is_file() || Path::new(scope).extension().is_some()
    })
}

fn is_low_risk_bounded_candidate(proposal: &DraftProposal) -> bool {
    !matches!(
        proposal.risk_level.trim().to_ascii_lowercase().as_str(),
        "high" | "critical"
    )
}

fn has_structural_invalidity(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    errors: &[DraftValidationError],
) -> bool {
    if proposal
        .allowed_scope
        .iter()
        .chain(proposal.entry_points.iter())
        .any(|path| is_artifact_storage_pattern(path))
    {
        return true;
    }

    let explicit_paths = explicit_scope_paths(prompt, repo_root);
    if !explicit_paths.is_empty()
        && !explicit_paths.iter().all(|path| {
            proposal
                .allowed_scope
                .iter()
                .any(|existing| existing == path)
        })
    {
        return true;
    }

    let explicit_target_checks = explicit_checks_from_prompt(
        repo_root,
        prompt,
        &[
            "target checks should include",
            "target checks to satisfy",
            "target checks:",
        ],
    );
    if !explicit_target_checks.is_empty()
        && !explicit_target_checks.iter().all(|command| {
            proposal
                .target_checks
                .iter()
                .any(|existing| existing == command)
        })
    {
        return true;
    }

    let explicit_integrity_checks = explicit_checks_from_prompt(
        repo_root,
        prompt,
        &[
            "integrity checks should include",
            "integrity checks to keep passing",
            "integrity checks:",
        ],
    );
    if !explicit_integrity_checks.is_empty()
        && !explicit_integrity_checks.iter().all(|command| {
            proposal
                .integrity_checks
                .iter()
                .any(|existing| existing == command)
        })
    {
        return true;
    }

    if proposal
        .entry_points
        .iter()
        .any(|entry_point| !scope_covers_path(&proposal.allowed_scope, entry_point))
    {
        return true;
    }

    if let Some(recipe) = best_protocol_recipe(prompt, proposal) {
        if !proposal
            .entry_points
            .iter()
            .chain(proposal.allowed_scope.iter())
            .any(|path| path.ends_with(&format!("/{}", recipe.basename)) || path == recipe.basename)
        {
            return true;
        }
    }

    errors.iter().any(|error| {
        error.field.starts_with("allowed_scope")
            || error.field.starts_with("entry_points")
            || error.field.starts_with("check[")
    })
}

fn fallback_source_paths(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
) -> Vec<String> {
    let mut recovered_from_recipe = false;
    let mut paths = explicit_scope_paths(prompt, repo_root)
        .into_iter()
        .filter(|path| is_fallback_source_path(repo_root, path))
        .collect::<Vec<_>>();

    if paths.is_empty() {
        let recipe_paths = fallback_protocol_recipe_paths(repo_root, prompt, proposal, scan);
        if !recipe_paths.is_empty() {
            paths = recipe_paths;
            recovered_from_recipe = true;
        }
    }

    if paths.is_empty() {
        for candidate in proposal
            .entry_points
            .iter()
            .chain(proposal.allowed_scope.iter())
            .filter(|path| is_fallback_source_path(repo_root, path))
        {
            if validate_scope_path(repo_root, candidate).is_ok() {
                push_unique(&mut paths, candidate.clone());
            }
        }
    }

    if !recovered_from_recipe {
        let include_storage = prompt_needs_storage_support(prompt)
            || proposal
                .allowed_scope
                .iter()
                .any(|path| is_artifact_storage_pattern(path));
        add_supporting_source_paths(repo_root, &mut paths, include_storage);
    }

    paths
}

fn fallback_target_checks(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
) -> Vec<String> {
    let explicit = explicit_checks_from_prompt(
        repo_root,
        prompt,
        &[
            "target checks should include",
            "target checks to satisfy",
            "target checks:",
        ],
    );
    if !explicit.is_empty() {
        return explicit;
    }
    if !scan.candidate_target_checks.is_empty() {
        return scan.candidate_target_checks.clone();
    }
    proposal.target_checks.clone()
}

fn fallback_integrity_checks(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
) -> Vec<String> {
    let explicit = explicit_checks_from_prompt(
        repo_root,
        prompt,
        &[
            "integrity checks should include",
            "integrity checks to keep passing",
            "integrity checks:",
        ],
    );
    if !explicit.is_empty() {
        return explicit;
    }
    if !scan.candidate_integrity_checks.is_empty() {
        return scan.candidate_integrity_checks.clone();
    }
    proposal.integrity_checks.clone()
}

fn is_fallback_source_path(repo_root: &Path, path: &str) -> bool {
    if !is_file_like_scope(path) || is_artifact_storage_pattern(path) {
        return false;
    }
    if path.contains('/') {
        return true;
    }
    is_repo_root_file(path) || repo_root.join(path).exists()
}

fn is_repo_root_file(path: &str) -> bool {
    matches!(
        path,
        "Cargo.toml" | "Cargo.lock" | "README.md" | "rust-toolchain.toml"
    )
}

fn fallback_protocol_recipe_paths(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
) -> Vec<String> {
    let mut paths = Vec::new();
    let Some(recipe) = best_protocol_recipe(prompt, proposal) else {
        return paths;
    };
    let Some(primary) = find_recipe_source_path(repo_root, prompt, proposal, scan, recipe.basename)
    else {
        return paths;
    };
    push_unique(&mut paths, primary);
    add_supporting_source_paths(repo_root, &mut paths, recipe.include_storage);
    paths
}

#[derive(Clone, Copy)]
struct ProtocolRecipe {
    keyword: &'static str,
    basename: &'static str,
    include_storage: bool,
}

fn best_protocol_recipe(prompt: &str, proposal: &DraftProposal) -> Option<ProtocolRecipe> {
    let mut best: Option<(i32, ProtocolRecipe)> = None;
    for recipe in [
        ProtocolRecipe {
            keyword: "proposal",
            basename: "proposal.rs",
            include_storage: true,
        },
        ProtocolRecipe {
            keyword: "review",
            basename: "review.rs",
            include_storage: true,
        },
        ProtocolRecipe {
            keyword: "scoring",
            basename: "scoring.rs",
            include_storage: false,
        },
        ProtocolRecipe {
            keyword: "synthesis",
            basename: "synthesis.rs",
            include_storage: true,
        },
    ] {
        let score = recipe_signal_score(prompt, proposal, recipe.keyword, recipe.basename);
        if score <= 0 {
            continue;
        }
        match best {
            Some((best_score, _)) if best_score >= score => {}
            _ => best = Some((score, recipe)),
        }
    }
    best.map(|(_, recipe)| recipe)
}

fn recipe_signal_score(
    prompt: &str,
    proposal: &DraftProposal,
    keyword: &str,
    basename: &str,
) -> i32 {
    let lowered = prompt.to_ascii_lowercase();
    let extra_signals = if basename == "synthesis.rs" {
        [
            "final record",
            "record.json",
            "synthesis.json",
            "leader",
            "hybrid",
            "escalate",
        ]
        .as_slice()
    } else {
        &[][..]
    };
    let mut score = 0;
    if lowered.contains(keyword) {
        score += 6;
    }
    if lowered.contains(basename) {
        score += 10;
    }
    score += extra_signals
        .iter()
        .filter(|signal| lowered.contains(**signal))
        .count() as i32
        * 8;
    score
        + proposal
            .entry_points
            .iter()
            .chain(proposal.allowed_scope.iter())
            .chain(proposal.behavior_requirements.iter())
            .map(|value| {
                let lowered = value.to_ascii_lowercase();
                let mut value_score = 0;
                if lowered.contains(keyword) {
                    value_score += 3;
                }
                if lowered.contains(basename) {
                    value_score += 6;
                }
                value_score
                    + extra_signals
                        .iter()
                        .filter(|signal| lowered.contains(**signal))
                        .count() as i32
                        * 4
            })
            .sum::<i32>()
}

fn find_recipe_source_path(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
    basename: &str,
) -> Option<String> {
    let mut candidates = Vec::new();
    for candidate in scan
        .candidate_entry_points
        .iter()
        .chain(scan.candidate_scope_paths.iter())
    {
        if candidate.ends_with(&format!("/{basename}"))
            && repo_root.join(candidate).is_file()
            && validate_scope_path(repo_root, candidate).is_ok()
        {
            push_unique(&mut candidates, candidate.clone());
        }
    }
    if candidates.is_empty() {
        candidates = find_source_files_by_basename(repo_root, basename);
    }
    if candidates.is_empty() {
        candidates = infer_recipe_source_candidates(repo_root, scan, basename);
    }
    if candidates.is_empty() {
        return None;
    }
    let tokens = prompt_tokens(prompt);
    let preferred_prefixes = preferred_source_prefixes(proposal);
    candidates.sort_by(|left, right| {
        recipe_candidate_score(right, &tokens, &preferred_prefixes)
            .cmp(&recipe_candidate_score(left, &tokens, &preferred_prefixes))
            .then_with(|| left.cmp(right))
    });
    candidates.into_iter().next()
}

fn recipe_candidate_score(path: &str, tokens: &[String], preferred_prefixes: &[String]) -> i32 {
    let mut score = relevance_score(path, tokens);
    for prefix in preferred_prefixes {
        if path.starts_with(prefix) {
            score += 50;
        }
    }
    score
}

fn infer_recipe_source_candidates(
    repo_root: &Path,
    scan: &RepoScanSummary,
    basename: &str,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for candidate in scan
        .candidate_entry_points
        .iter()
        .chain(scan.candidate_scope_paths.iter())
    {
        let Some(src_dir) = infer_source_dir_from_candidate(candidate) else {
            continue;
        };
        let inferred = format!("{src_dir}/{basename}");
        if validate_scope_path(repo_root, &inferred).is_ok() {
            push_unique(&mut candidates, inferred);
        }
    }
    if candidates.is_empty() {
        for src_dir in find_source_dirs(repo_root) {
            let inferred = format!("{src_dir}/{basename}");
            if validate_scope_path(repo_root, &inferred).is_ok() {
                push_unique(&mut candidates, inferred);
            }
        }
    }
    candidates
}

fn infer_source_dir_from_candidate(path: &str) -> Option<String> {
    let relative = normalize_relative_path(path).ok()?;
    let path_str = relative.to_string_lossy();
    if path_str.ends_with("/src") {
        return Some(path_str.to_string());
    }
    if path_str.ends_with("/Cargo.toml") {
        let parent = relative.parent()?;
        let src_dir = parent.join("src");
        return Some(src_dir.to_string_lossy().to_string());
    }
    let parent = relative.parent()?;
    if parent.ends_with("src") {
        return Some(parent.to_string_lossy().to_string());
    }
    None
}

fn preferred_source_prefixes(proposal: &DraftProposal) -> Vec<String> {
    let mut prefixes = Vec::new();
    for candidate in proposal
        .import_paths
        .iter()
        .chain(proposal.entry_points.iter())
        .chain(proposal.allowed_scope.iter())
    {
        if let Some(src_dir) = infer_source_dir_from_candidate(candidate) {
            push_unique(&mut prefixes, src_dir);
        }
    }
    prefixes
}

fn find_source_dirs(repo_root: &Path) -> Vec<String> {
    let mut matches = Vec::new();
    collect_source_dirs(repo_root, repo_root, &mut matches).ok();
    stable_dedupe(&mut matches);
    matches
}

fn collect_source_dirs(repo_root: &Path, current: &Path, matches: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_name(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("path escaped repo root"))?;
        if ignored_relative_path(relative) {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        if name == "src" {
            push_unique(matches, relative.to_string_lossy().to_string());
        }
        collect_source_dirs(repo_root, &path, matches)?;
    }
    Ok(())
}

fn find_source_files_by_basename(repo_root: &Path, basename: &str) -> Vec<String> {
    let mut matches = Vec::new();
    collect_source_file_matches(repo_root, repo_root, basename, &mut matches).ok();
    stable_dedupe(&mut matches);
    matches
}

fn collect_source_file_matches(
    repo_root: &Path,
    current: &Path,
    basename: &str,
    matches: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_name(&name) {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_err(|_| anyhow!("path escaped repo root"))?;
        if ignored_relative_path(relative) {
            continue;
        }
        if path.is_dir() {
            collect_source_file_matches(repo_root, &path, basename, matches)?;
            continue;
        }
        if name != basename {
            continue;
        }
        let relative = relative.to_string_lossy().to_string();
        if is_fallback_source_path(repo_root, &relative) {
            push_unique(matches, relative);
        }
    }
    Ok(())
}

fn add_supporting_source_paths(repo_root: &Path, paths: &mut Vec<String>, include_storage: bool) {
    let seed_paths = paths.clone();
    for path in seed_paths {
        let Some(src_dir) = source_dir_for_path(&path) else {
            continue;
        };
        let lib_rs = format!("{src_dir}/lib.rs");
        if repo_root.join(&lib_rs).exists() {
            push_unique(paths, lib_rs);
        }
        if include_storage {
            let storage_rs = format!("{src_dir}/storage.rs");
            if repo_root.join(&storage_rs).exists() {
                push_unique(paths, storage_rs);
            }
        }
    }
}

fn prompt_needs_storage_support(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    [
        "persist", "storage", "artifact", ".punk/", "proposal", "review",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn source_dir_for_path(path: &str) -> Option<String> {
    let relative = normalize_relative_path(path).ok()?;
    let parent = relative.parent()?;
    let file_name = relative.file_name()?.to_str()?;
    if !parent.ends_with("src") || matches!(file_name, "lib.rs" | "main.rs") {
        return None;
    }
    Some(parent.to_string_lossy().to_string())
}

fn is_artifact_storage_pattern(path: &str) -> bool {
    let lowered = path.trim().to_ascii_lowercase();
    lowered.contains('<')
        || lowered.contains('>')
        || lowered.starts_with(".punk/")
        || lowered.contains("/.punk/")
}

fn scope_covers_path(allowed_scope: &[String], entry_point: &str) -> bool {
    let Ok(entry) = normalize_relative_path(entry_point) else {
        return false;
    };
    allowed_scope.iter().any(|scope| {
        let Ok(scope_path) = normalize_relative_path(scope) else {
            return false;
        };
        entry == scope_path || entry.starts_with(&scope_path)
    })
}

fn validate_scope_path(repo_root: &Path, value: &str) -> std::result::Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("must be non-empty".to_string());
    }
    if trimmed == "." || trimmed == "/" || trimmed == "*" {
        return Err("must be bounded to a repo-relative path".to_string());
    }
    let relative = normalize_relative_path(trimmed)?;
    let joined = repo_root.join(&relative);
    if joined.exists() {
        return Ok(());
    }
    let mut current = joined.parent();
    while let Some(parent) = current {
        if parent == repo_root {
            break;
        }
        if parent.exists() {
            return Ok(());
        }
        current = parent.parent();
    }
    if joined
        .parent()
        .ok_or_else(|| "must stay under repo root".to_string())?
        .exists()
    {
        return Ok(());
    }
    Err("path does not exist and has no existing parent directory".to_string())
}

fn validate_check_command(repo_root: &Path, value: &str) -> std::result::Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("must be non-empty".to_string());
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("must be a single-line shell command".to_string());
    }
    for token in trimmed.split_whitespace() {
        if token.contains("../") || token.starts_with("..") || token.contains("..\\") {
            return Err("must not reference paths outside repo root".to_string());
        }
        if token.starts_with('/') {
            let absolute = Path::new(token);
            if !absolute.starts_with(repo_root) {
                return Err("must not reference absolute paths outside repo root".to_string());
            }
        }
    }
    Ok(())
}

fn normalize_relative_path(value: &str) -> std::result::Result<PathBuf, String> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("must be repo-relative".to_string());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => return Err("must not contain parent traversal".to_string()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("must stay within repo root".to_string())
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("must be non-empty".to_string());
    }
    Ok(normalized)
}

fn relevance_score(path: &str, tokens: &[String]) -> i32 {
    let lowered = path.to_ascii_lowercase();
    let mut score = 0;
    for token in tokens {
        if lowered.contains(token) {
            score += 4;
            if lowered.ends_with(token) || lowered.contains(&format!("/{token}")) {
                score += 2;
            }
        }
    }
    if lowered.starts_with("src/") {
        score += 1;
    }
    if lowered.starts_with("tests/") || lowered.contains("/tests/") {
        score += 1;
    }
    score
}

fn explicit_scope_paths(prompt: &str, repo_root: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    for candidate in code_spans(prompt)
        .into_iter()
        .chain(path_like_tokens(prompt).into_iter())
    {
        if !looks_like_repo_path(&candidate) {
            continue;
        }
        if !candidate.contains('/')
            && !is_repo_root_file(&candidate)
            && !repo_root.join(&candidate).exists()
        {
            continue;
        }
        if validate_scope_path(repo_root, &candidate).is_ok() {
            push_unique(&mut paths, candidate);
        }
    }
    paths
}

fn explicit_checks_from_prompt(repo_root: &Path, prompt: &str, markers: &[&str]) -> Vec<String> {
    let lower = prompt.to_ascii_lowercase();
    let mut commands = Vec::new();
    for marker in markers {
        let marker_lower = marker.to_ascii_lowercase();
        let mut search_from = 0;
        while let Some(relative_idx) = lower[search_from..].find(&marker_lower) {
            let start = search_from + relative_idx + marker_lower.len();
            let tail = &prompt[start..];
            let section = section_until_break(tail);
            for clause in split_command_clauses(section) {
                if let Some(command) = command_from_clause(&clause) {
                    if validate_check_command(repo_root, &command).is_ok() {
                        push_unique(&mut commands, command);
                    }
                }
            }
            search_from = start;
        }
    }
    commands
}

fn code_spans(prompt: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut in_span = false;
    let mut start = 0;
    for (idx, ch) in prompt.char_indices() {
        if ch != '`' {
            continue;
        }
        if in_span {
            let candidate = prompt[start..idx].trim();
            if !candidate.is_empty() {
                spans.push(candidate.to_string());
            }
            in_span = false;
        } else {
            in_span = true;
            start = idx + ch.len_utf8();
        }
    }
    spans
}

fn path_like_tokens(prompt: &str) -> Vec<String> {
    prompt
        .split_whitespace()
        .map(trim_token_punctuation)
        .filter(|token| !token.is_empty())
        .filter(|token| looks_like_repo_path(token))
        .map(ToOwned::to_owned)
        .collect()
}

fn looks_like_repo_path(value: &str) -> bool {
    value.contains('/')
        || matches!(
            value,
            "Cargo.toml" | "Cargo.lock" | "README.md" | "rust-toolchain.toml"
        )
        || [".rs", ".toml", ".json", ".md", ".yaml", ".yml"]
            .iter()
            .any(|suffix| value.ends_with(suffix))
}

fn trim_token_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '`' | '"' | '\'' | ',' | '.' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    })
}

fn section_until_break(input: &str) -> &str {
    let mut end = input.len();
    for marker in ["\n\n", "\nTarget checks", "\nIntegrity checks", ". "] {
        if let Some(idx) = input.find(marker) {
            end = end.min(idx);
        }
    }
    input[..end].trim()
}

fn split_command_clauses(section: &str) -> Vec<String> {
    section
        .replace(" and ", "\n")
        .split([',', '\n', ';'])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn command_from_clause(clause: &str) -> Option<String> {
    let prefixes = ["cargo ", "npm ", "pnpm ", "yarn ", "bun ", "make "];
    let lowered = clause.to_ascii_lowercase();
    let start = prefixes
        .iter()
        .filter_map(|prefix| lowered.find(prefix))
        .min()?;
    Some(
        clause[start..]
            .trim()
            .trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '.'))
            .to_string(),
    )
}

fn is_file_like_scope(path: &str) -> bool {
    Path::new(path).extension().is_some()
        || matches!(
            path,
            "Cargo.toml" | "Cargo.lock" | "README.md" | "rust-toolchain.toml"
        )
}

fn stable_dedupe(items: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

fn dedupe(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_repo_scan_infers_cargo_test() {
        let root = std::env::temp_dir().join(format!("punk-core-rust-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        let summary = scan_repo(&root, "update demo lib").unwrap();
        assert!(summary
            .candidate_integrity_checks
            .contains(&"cargo test".to_string()));
        assert!(summary
            .candidate_target_checks
            .contains(&"cargo test --lib".to_string()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_json_scan_uses_existing_test_script() {
        let root = std::env::temp_dir().join(format!("punk-core-node-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"scripts":{"test":"vitest","lint":"eslint ."} }"#,
        )
        .unwrap();
        let summary = scan_repo(&root, "fix lint").unwrap();
        assert!(summary
            .candidate_integrity_checks
            .contains(&"npm run test".to_string()));
        assert!(summary
            .candidate_target_checks
            .contains(&"npm run lint".to_string()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scope_candidates_prefer_prompt_matches() {
        let root = std::env::temp_dir().join(format!("punk-core-scope-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/auth")).unwrap();
        fs::write(root.join("src/auth/mod.rs"), "pub fn auth() {}\n").unwrap();
        fs::write(root.join("src/lib.rs"), "pub mod auth;\n").unwrap();
        let summary = scan_repo(&root, "tighten auth behavior").unwrap();
        assert!(summary
            .candidate_scope_paths
            .first()
            .is_some_and(|path| path.contains("auth")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_outside_scope() {
        let root = std::env::temp_dir().join(format!("punk-core-validate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        let proposal = DraftProposal {
            title: "x".into(),
            summary: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["../escape".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        assert!(!errors.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scope_candidates_ignore_reference_repos() {
        let root = std::env::temp_dir().join(format!("punk-core-ignore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/auth")).unwrap();
        fs::create_dir_all(root.join("docs/reference-repos/demo")).unwrap();
        fs::write(root.join("src/auth/mod.rs"), "pub fn auth() {}\n").unwrap();
        fs::write(
            root.join("docs/reference-repos/demo/auth_helpers.rs"),
            "pub fn noisy() {}\n",
        )
        .unwrap();
        let summary = scan_repo(&root, "tighten auth helpers").unwrap();
        assert!(summary
            .candidate_scope_paths
            .iter()
            .all(|path| !path.starts_with("docs/reference-repos")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scope_candidates_ignore_delve_runs() {
        let root =
            std::env::temp_dir().join(format!("punk-core-delve-ignore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/auth")).unwrap();
        fs::create_dir_all(root.join("docs/research/_delve_runs/demo/output")).unwrap();
        fs::write(root.join("src/auth/mod.rs"), "pub fn auth() {}\n").unwrap();
        fs::write(
            root.join("docs/research/_delve_runs/demo/output/synthesis.md"),
            "auth helper research\n",
        )
        .unwrap();
        let summary = scan_repo(&root, "tighten auth helpers").unwrap();
        assert!(summary
            .candidate_scope_paths
            .iter()
            .all(|path| !path.starts_with("docs/research/_delve_runs")));
        assert!(summary
            .candidate_entry_points
            .iter()
            .all(|path| !path.starts_with("docs/research/_delve_runs")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rust_target_checks_prefer_matching_package_names() {
        let root = std::env::temp_dir().join(format!("punk-core-pkgs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-gate/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/Cargo.toml"),
            "[package]\nname = \"punk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-gate/Cargo.toml"),
            "[package]\nname = \"punk-gate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let summary = scan_repo(&root, "extract helpers in punk-core and punk-gate").unwrap();
        assert!(summary
            .candidate_target_checks
            .contains(&"cargo test -p punk-core".to_string()));
        assert!(summary
            .candidate_target_checks
            .contains(&"cargo test -p punk-gate".to_string()));
        assert!(summary
            .candidate_target_checks
            .iter()
            .all(|check| !check.starts_with("cargo test extract")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rust_workspace_scan_prefers_workspace_integrity_check() {
        let root =
            std::env::temp_dir().join(format!("punk-core-workspace-int-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/demo/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/demo/Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(root.join("crates/demo/src/lib.rs"), "pub fn demo() {}\n").unwrap();

        let summary = scan_repo(&root, "tighten demo").unwrap();
        assert_eq!(
            summary.candidate_integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_accepts_scaffold_paths_with_existing_ancestor() {
        let root = std::env::temp_dir().join(format!("punk-core-scaffold-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates")).unwrap();

        let proposal = DraftProposal {
            title: "new crate".into(),
            summary: "new crate".into(),
            entry_points: vec![
                "crates/punk-council/Cargo.toml".into(),
                "crates/punk-council/src/lib.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["crate scaffold".into()],
            behavior_requirements: vec!["create crate".into()],
            allowed_scope: vec![
                "crates/punk-council/Cargo.toml".into(),
                "crates/punk-council/src/lib.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
        };

        let errors = validate_draft_proposal(&root, &proposal);
        assert!(errors.is_empty(), "{errors:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn canonicalize_preserves_explicit_paths_and_checks() {
        let root =
            std::env::temp_dir().join(format!("punk-core-canonicalize-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates")).unwrap();

        let prompt = "Scaffold crate with `crates/punk-council/Cargo.toml` and `crates/punk-council/src/lib.rs`. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let mut proposal = DraftProposal {
            title: "wrong".into(),
            summary: "wrong".into(),
            entry_points: vec!["crates/punk-core/src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["crates/punk-core".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };

        canonicalize_draft_proposal(&root, prompt, &mut proposal);

        assert_eq!(
            proposal.allowed_scope,
            vec![
                "crates/punk-council/Cargo.toml".to_string(),
                "crates/punk-council/src/lib.rs".to_string()
            ]
        );
        assert_eq!(proposal.entry_points, proposal.allowed_scope);
        assert_eq!(
            proposal.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-council".to_string()
            ]
        );
        assert_eq!(
            proposal.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn validation_rejects_artifact_storage_paths_and_entrypoint_mismatch() {
        let root =
            std::env::temp_dir().join(format!("punk-core-invalid-scope-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/demo/src")).unwrap();
        fs::write(root.join("crates/demo/src/lib.rs"), "pub fn demo() {}\n").unwrap();

        let proposal = DraftProposal {
            title: "proposal phase".into(),
            summary: "proposal phase".into(),
            entry_points: vec!["crates/demo/src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["proposal orchestration".into()],
            behavior_requirements: vec!["persist proposal artifacts".into()],
            allowed_scope: vec!["punk/council/<id>/proposals/".into()],
            target_checks: vec!["cargo test -p demo".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
        };

        let errors = validate_draft_proposal(&root, &proposal);
        assert!(errors
            .iter()
            .any(|error| error.message.contains("artifact storage or placeholder")));
        assert!(errors
            .iter()
            .any(|error| error.message.contains("covered by allowed_scope")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_recovers_real_source_paths_and_checks() {
        let root = std::env::temp_dir().join(format!("punk-core-fallback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod proposal;\npub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/proposal.rs"),
            "pub fn run() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();

        let prompt = "Implement the proposal phase in punk-council. Add bounded proposal orchestration that persists proposal artifacts under .punk/council/<id>/proposals/. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "proposal phase".into(),
            summary: "proposal phase".into(),
            entry_points: vec![
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["proposal result".into()],
            behavior_requirements: vec!["persist proposal artifacts".into()],
            allowed_scope: vec!["punk/council/<id>/proposals/".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "crates/punk-council/src/proposal.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );
        assert_eq!(fallback.entry_points, fallback.allowed_scope);
        assert_eq!(
            fallback.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-council".to_string(),
            ]
        );
        assert_eq!(
            fallback.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }
    #[test]
    fn bounded_fallback_recovers_review_recipe_from_module_placeholders() {
        let root =
            std::env::temp_dir().join(format!("punk-core-review-fallback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod review;\npub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/review.rs"),
            "pub fn run() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();

        let prompt = "Implement the review phase in punk-council. Add bounded review orchestration that persists typed review payloads under .punk/council/<id>/reviews/. Target checks should include cargo build -p punk-cli, cargo test -p punk-core, and cargo test -p punk-orch. Integrity checks should include cargo test --workspace.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "review phase".into(),
            summary: "review phase".into(),
            entry_points: vec!["review.rs".into(), "lib.rs".into(), "storage.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["review result".into()],
            behavior_requirements: vec!["review payload orchestration".into()],
            allowed_scope: vec!["punk/council/<id>/reviews/".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "crates/punk-council/src/review.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );
        assert_eq!(fallback.entry_points, fallback.allowed_scope);
        assert_eq!(
            fallback.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-core".to_string(),
                "cargo test -p punk-orch".to_string(),
            ]
        );
        assert_eq!(
            fallback.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_recovers_scoring_recipe_without_storage() {
        let root =
            std::env::temp_dir().join(format!("punk-core-scoring-fallback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod scoring;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/scoring.rs"),
            "pub fn score() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();

        let prompt = "Add deterministic council scoring in punk-council. Keep the slice scoring-only. Target checks should include cargo build -p punk-cli and cargo test -p punk-core. Integrity checks should include cargo test --workspace.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "scoring phase".into(),
            summary: "scoring phase".into(),
            entry_points: vec!["scoring.rs".into(), "lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["scoreboard".into()],
            behavior_requirements: vec!["deterministic scoring".into()],
            allowed_scope: vec![".punk/council/<id>/scoreboard.json".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "crates/punk-council/src/scoring.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
            ]
        );
        assert_eq!(fallback.entry_points, fallback.allowed_scope);
        assert!(!fallback
            .allowed_scope
            .iter()
            .any(|path| path.ends_with("storage.rs")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_recovers_synthesis_recipe_with_storage() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-synthesis-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod synthesis;\npub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/synthesis.rs"),
            "pub fn synthesize() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();

        let prompt = "Add council synthesis and final record completion in punk-council. Keep the slice advisory-only. Target checks should include cargo build -p punk-cli and cargo test -p punk-orch. Integrity checks should include cargo test --workspace.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "synthesis phase".into(),
            summary: "synthesis phase".into(),
            entry_points: vec!["synthesis.rs".into(), "lib.rs".into(), "storage.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["synthesis result".into()],
            behavior_requirements: vec!["persist synthesis artifacts".into()],
            allowed_scope: vec![".punk/council/<id>/synthesis.json".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "crates/punk-council/src/synthesis.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );
        assert_eq!(fallback.entry_points, fallback.allowed_scope);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_recovers_new_synthesis_file_from_broad_scope() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-synthesis-new-file-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/packet.rs"),
            "pub fn packet() {}\n",
        )
        .unwrap();
        let prompt = "Add council synthesis and final record completion in punk-council. Take the deterministic scoreboard and produce a typed CouncilSynthesis with Leader, Hybrid, or Escalate, persist synthesis.json, and write a final record.json that points to packet, proposals, reviews, scoreboard, and synthesis artifacts. Keep the slice advisory-only. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "synthesis phase".into(),
            summary: "synthesis phase".into(),
            entry_points: vec![
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["final record".into()],
            behavior_requirements: vec!["write final record.json".into()],
            allowed_scope: vec![
                "crates/punk-council/src".into(),
                "crates/punk-council/src/packet.rs".into(),
                "crates/punk-domain/src/council.rs".into(),
            ],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "crates/punk-council/src/synthesis.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );
        assert_eq!(fallback.entry_points, fallback.allowed_scope);
        assert_eq!(
            fallback.target_checks,
            vec![
                "cargo build -p punk-cli".to_string(),
                "cargo test -p punk-council".to_string(),
            ]
        );
        assert_eq!(
            fallback.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_prefers_import_path_source_prefix_for_new_synthesis_file() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-synthesis-prefix-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-cli/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub mod storage;\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/punk-cli/src/main.rs"), "fn main() {}\n").unwrap();

        let prompt = "Add council synthesis and final record completion in punk-council. Take the deterministic scoreboard and produce a typed CouncilSynthesis with Leader, Hybrid, or Escalate, persist synthesis.json, and write a final record.json that points to packet, proposals, reviews, scoreboard, and synthesis artifacts. Keep the slice advisory-only and inside punk-council. Target checks should include cargo build -p punk-cli and cargo test -p punk-council. Integrity checks should include cargo test --workspace.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "synthesis phase".into(),
            summary: "synthesis phase".into(),
            entry_points: vec!["synthesis.json".into(), "record.json".into()],
            import_paths: vec![
                "crates/punk-council/src".into(),
                "crates/punk-domain/src/council.rs".into(),
                "crates/punk-core/src/artifacts.rs".into(),
            ],
            expected_interfaces: vec!["final record".into()],
            behavior_requirements: vec!["write final record.json".into()],
            allowed_scope: vec!["synthesis.json".into(), "record.json".into()],
            target_checks: vec![
                "cargo test -p punk-council".into(),
                "cargo test -p punk-domain".into(),
                "cargo test -p punk-core".into(),
            ],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "crates/punk-council/src/synthesis.rs".to_string(),
                "crates/punk-council/src/lib.rs".to_string(),
                "crates/punk-council/src/storage.rs".to_string(),
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }
}
