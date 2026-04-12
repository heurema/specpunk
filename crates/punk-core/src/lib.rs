use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use punk_domain::{DraftProposal, DraftValidationError, RepoScanSummary};

pub mod artifacts;

pub use artifacts::{find_object_path, read_json, relative_ref, write_json};

struct ScopeCandidates {
    files: Vec<String>,
    directories: Vec<String>,
}

enum GreenfieldScaffoldKind {
    Rust,
    Go,
    Python,
    Node,
}

struct GreenfieldScaffoldSeed {
    entry_points: Vec<String>,
    file_scope_paths: Vec<String>,
    directory_scope_paths: Vec<String>,
}

fn is_swiftpm_build_path(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::Normal(name) if name == ".build"))
}

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
    } else if repo_root.join("go.mod").exists() {
        manifests.push("go.mod".to_string());
        candidate_integrity_checks.extend(go_integrity_checks());
        candidate_target_checks.extend(go_target_checks());
        "go".to_string()
    } else if let Some(python_manifest) = python_manifest(repo_root) {
        manifests.push(python_manifest);
        python_checks(
            &mut candidate_target_checks,
            &mut candidate_integrity_checks,
        );
        "python".to_string()
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

    let greenfield_scaffold_kind = greenfield_scaffold_kind(repo_root, &tokens);
    if candidate_integrity_checks.is_empty() {
        if let Some(scaffold_kind) = &greenfield_scaffold_kind {
            let bootstrap_check = greenfield_bootstrap_check(scaffold_kind, &tokens);
            candidate_target_checks.push(bootstrap_check.clone());
            candidate_integrity_checks.push(bootstrap_check);
            notes.push(format!(
                "inferred initial {} bootstrap checks from bootstrapped greenfield prompt",
                greenfield_scaffold_kind_label(scaffold_kind)
            ));
        }
    }

    dedupe(&mut candidate_integrity_checks);
    dedupe(&mut candidate_target_checks);

    if candidate_integrity_checks.is_empty() {
        notes.push("no trustworthy integrity checks inferred".to_string());
    }

    let greenfield_scaffold_seed = greenfield_scaffold_kind
        .as_ref()
        .map(|kind| greenfield_scaffold_seed(kind, &tokens));

    let mut scope_candidates = collect_scope_candidates(repo_root, prompt)?;
    if let Some(seed) = &greenfield_scaffold_seed {
        prepend_preferred_candidates(&mut scope_candidates.files, &seed.file_scope_paths, 20);
        prepend_preferred_candidates(
            &mut scope_candidates.directories,
            &seed.directory_scope_paths,
            20,
        );
        if let Some(scaffold_kind) = &greenfield_scaffold_kind {
            notes.push(format!(
                "preferring scaffoldable {} scope candidates from bootstrapped greenfield prompt",
                greenfield_scaffold_kind_label(scaffold_kind)
            ));
        }
    }

    let mut candidate_entry_points = infer_entry_points(repo_root, prompt, &scope_candidates.files);
    if let Some(seed) = &greenfield_scaffold_seed {
        prepend_preferred_candidates(&mut candidate_entry_points, &seed.entry_points, 10);
    }
    let candidate_scope_paths = combined_scope_candidates(&scope_candidates);

    Ok(RepoScanSummary {
        project_kind,
        manifests,
        package_manager,
        available_scripts,
        candidate_entry_points,
        candidate_scope_paths,
        candidate_file_scope_paths: scope_candidates.files,
        candidate_directory_scope_paths: scope_candidates.directories,
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
    if !has_structural_invalidity(repo_root, prompt, proposal, scan, errors) {
        return None;
    }

    let mut allowed_scope = fallback_source_paths(repo_root, prompt, proposal, scan);
    extend_greenfield_scaffold_scope(repo_root, prompt, &mut allowed_scope);
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
    extend_greenfield_scaffold_scope(repo_root, prompt, &mut fallback.allowed_scope);
    stable_dedupe(&mut fallback.allowed_scope);
    Some(fallback)
}

fn extend_greenfield_scaffold_scope(
    repo_root: &Path,
    prompt: &str,
    allowed_scope: &mut Vec<String>,
) {
    let tokens = prompt_tokens(prompt);
    let Some(kind) = greenfield_scaffold_kind(repo_root, &tokens) else {
        return;
    };
    let seed = greenfield_scaffold_seed(&kind, &tokens);
    for path in seed
        .file_scope_paths
        .into_iter()
        .chain(seed.directory_scope_paths.into_iter())
    {
        push_unique(allowed_scope, path);
    }
}

pub fn canonicalize_draft_proposal(repo_root: &Path, prompt: &str, proposal: &mut DraftProposal) {
    if let Some(explicit_scope) = explicit_scope_override(prompt, repo_root) {
        proposal.allowed_scope = explicit_scope.clone();
        if let Some(explicit_entry_points) = explicit_entry_points_override(&explicit_scope) {
            proposal.entry_points = explicit_entry_points;
        }
    } else {
        stable_dedupe(&mut proposal.allowed_scope);
        stable_dedupe(&mut proposal.entry_points);
    }

    if let Some(explicit_target_checks) = explicit_target_checks_override(repo_root, prompt) {
        proposal.target_checks = explicit_target_checks;
    } else {
        stable_dedupe(&mut proposal.target_checks);
    }

    if let Some(explicit_integrity_checks) = explicit_integrity_checks_override(repo_root, prompt) {
        proposal.integrity_checks = explicit_integrity_checks;
    } else {
        stable_dedupe(&mut proposal.integrity_checks);
    }
}

pub fn apply_explicit_prompt_overrides(
    repo_root: &Path,
    prompt: &str,
    proposal: &mut DraftProposal,
) {
    if let Some(explicit_scope) = explicit_scope_override(prompt, repo_root) {
        proposal.allowed_scope = explicit_scope.clone();
        if let Some(explicit_entry_points) = explicit_entry_points_override(&explicit_scope) {
            proposal.entry_points = explicit_entry_points;
        }
    }

    if let Some(explicit_target_checks) = explicit_target_checks_override(repo_root, prompt) {
        proposal.target_checks = explicit_target_checks;
    }

    if let Some(explicit_integrity_checks) = explicit_integrity_checks_override(repo_root, prompt) {
        proposal.integrity_checks = explicit_integrity_checks;
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

fn collect_scope_candidates(repo_root: &Path, prompt: &str) -> Result<ScopeCandidates> {
    let mut file_candidates = BTreeMap::new();
    let mut directory_candidates = BTreeMap::new();
    let tokens = prompt_tokens(prompt);
    let mut top_level_files = Vec::new();
    let mut top_level_directories = Vec::new();
    for entry in fs::read_dir(repo_root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if ignored_name(&name)
            || ignored_relative_path(Path::new(&name))
            || is_swiftpm_build_path(Path::new(&name))
        {
            continue;
        }
        if path.is_dir() {
            top_level_directories.push(name.clone());
            directory_candidates.insert(name, 1);
        } else {
            top_level_files.push(name.clone());
            file_candidates.insert(name, 1);
        }
    }

    walk_repo(
        repo_root,
        repo_root,
        &tokens,
        &mut file_candidates,
        &mut directory_candidates,
    )?;

    if file_candidates.is_empty() {
        for name in top_level_files {
            file_candidates.insert(name, 1);
        }
    }
    if directory_candidates.is_empty() {
        for name in top_level_directories {
            directory_candidates.insert(name, 1);
        }
    }

    let mut files = sort_scored_candidates(file_candidates);
    files.retain(|candidate| !is_swiftpm_build_path(Path::new(candidate)));

    let mut directories = sort_scored_candidates(directory_candidates);
    directories.retain(|candidate| !is_swiftpm_build_path(Path::new(candidate)));

    Ok(ScopeCandidates { files, directories })
}

fn walk_repo(
    repo_root: &Path,
    current: &Path,
    tokens: &[String],
    file_candidates: &mut BTreeMap<String, i32>,
    directory_candidates: &mut BTreeMap<String, i32>,
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
            walk_repo(
                repo_root,
                &path,
                tokens,
                file_candidates,
                directory_candidates,
            )?;
            continue;
        }
        let relative = relative_path.to_string_lossy().to_string();
        let score = relevance_score(&relative, tokens)
            + content_relevance_score(repo_root, &relative, tokens, false)
            + entry_point_hint_score(repo_root, &relative, tokens);
        if score > 0 {
            file_candidates
                .entry(relative.clone())
                .and_modify(|value| *value = (*value).max(score))
                .or_insert(score);
            if let Some(parent) = Path::new(&relative).parent() {
                let parent = parent.to_string_lossy().to_string();
                if !parent.is_empty() && parent != "." {
                    let parent_score = score.saturating_sub(1).max(1);
                    directory_candidates
                        .entry(parent)
                        .and_modify(|value| *value = (*value).max(parent_score))
                        .or_insert(parent_score);
                }
            }
        }
    }
    Ok(())
}

fn sort_scored_candidates(candidates: BTreeMap<String, i32>) -> Vec<String> {
    let mut items: Vec<(String, i32)> = candidates.into_iter().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items.into_iter().map(|(path, _)| path).take(20).collect()
}

fn combined_scope_candidates(candidates: &ScopeCandidates) -> Vec<String> {
    candidates
        .files
        .iter()
        .chain(candidates.directories.iter())
        .filter(|candidate| !is_swiftpm_build_path(Path::new(candidate)))
        .cloned()
        .take(20)
        .collect()
}

fn ignored_name(name: &str) -> bool {
    matches!(name, ".git" | ".punk" | "target" | "node_modules")
}

fn ignored_relative_path(relative: &Path) -> bool {
    if is_swiftpm_build_path(relative) {
        return true;
    }
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

fn repo_has_bootstrap_markers(repo_root: &Path) -> bool {
    let bootstrap_dir = repo_root.join(".punk/bootstrap");
    let has_bootstrap_doc = fs::read_dir(&bootstrap_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .any(|entry| entry.path().is_file());
    repo_root.join(".punk/AGENT_START.md").exists() && has_bootstrap_doc
}

fn python_manifest(repo_root: &Path) -> Option<String> {
    for candidate in ["pyproject.toml", "pytest.ini", "requirements.txt"] {
        if repo_root.join(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn go_target_checks() -> Vec<String> {
    vec!["go test ./...".to_string()]
}

fn go_integrity_checks() -> Vec<String> {
    vec!["go test ./...".to_string()]
}

fn python_checks(target: &mut Vec<String>, integrity: &mut Vec<String>) {
    target.push("pytest".to_string());
    integrity.push("pytest".to_string());
}

fn prompt_explicitly_requests_greenfield_rust_scaffold(tokens: &[String]) -> bool {
    let requests_rust = tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "rust" | "cargo" | "crate" | "crates" | "workspace"
        )
    });
    let requests_scaffold = prompt_explicitly_requests_scaffold(tokens);
    requests_rust && requests_scaffold
}

fn prompt_explicitly_requests_greenfield_go_scaffold(tokens: &[String]) -> bool {
    let requests_go = tokens
        .iter()
        .any(|token| matches!(token.as_str(), "go" | "golang" | "module"));
    let requests_scaffold = prompt_explicitly_requests_scaffold(tokens);
    requests_go && requests_scaffold
}

fn prompt_explicitly_requests_greenfield_python_scaffold(tokens: &[String]) -> bool {
    let requests_python = tokens
        .iter()
        .any(|token| matches!(token.as_str(), "python" | "pytest" | "pyproject" | "poetry"));
    let requests_scaffold = prompt_explicitly_requests_scaffold(tokens);
    requests_python && requests_scaffold
}

fn prompt_explicitly_requests_greenfield_node_scaffold(tokens: &[String]) -> bool {
    let requests_node = tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "typescript"
                | "javascript"
                | "node"
                | "npm"
                | "pnpm"
                | "yarn"
                | "tsconfig"
                | "tsx"
                | "vite"
        )
    });
    let requests_scaffold = prompt_explicitly_requests_scaffold(tokens);
    requests_node && requests_scaffold
}

fn prompt_explicitly_requests_scaffold(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| matches!(token.as_str(), "scaffold" | "bootstrap" | "greenfield"))
}

fn greenfield_scaffold_kind(repo_root: &Path, tokens: &[String]) -> Option<GreenfieldScaffoldKind> {
    if !repo_has_bootstrap_markers(repo_root) {
        return None;
    }
    if !repo_root.join("Cargo.toml").exists()
        && prompt_explicitly_requests_greenfield_rust_scaffold(tokens)
    {
        return Some(GreenfieldScaffoldKind::Rust);
    }
    if !repo_root.join("go.mod").exists()
        && prompt_explicitly_requests_greenfield_go_scaffold(tokens)
    {
        return Some(GreenfieldScaffoldKind::Go);
    }
    if python_manifest(repo_root).is_none()
        && prompt_explicitly_requests_greenfield_python_scaffold(tokens)
    {
        return Some(GreenfieldScaffoldKind::Python);
    }
    if !repo_root.join("package.json").exists()
        && prompt_explicitly_requests_greenfield_node_scaffold(tokens)
    {
        return Some(GreenfieldScaffoldKind::Node);
    }
    None
}

fn greenfield_bootstrap_check(kind: &GreenfieldScaffoldKind, tokens: &[String]) -> String {
    match kind {
        GreenfieldScaffoldKind::Rust => {
            if tokens.iter().any(|token| token == "workspace") {
                "cargo test --workspace".to_string()
            } else {
                "cargo test".to_string()
            }
        }
        GreenfieldScaffoldKind::Go => "go test ./...".to_string(),
        GreenfieldScaffoldKind::Python => "pytest".to_string(),
        GreenfieldScaffoldKind::Node => "npm test".to_string(),
    }
}

fn greenfield_scaffold_kind_label(kind: &GreenfieldScaffoldKind) -> &'static str {
    match kind {
        GreenfieldScaffoldKind::Rust => "Rust",
        GreenfieldScaffoldKind::Go => "Go",
        GreenfieldScaffoldKind::Python => "Python",
        GreenfieldScaffoldKind::Node => "TypeScript/Node",
    }
}

fn greenfield_scaffold_seed(
    kind: &GreenfieldScaffoldKind,
    tokens: &[String],
) -> GreenfieldScaffoldSeed {
    match kind {
        GreenfieldScaffoldKind::Rust => {
            let prefers_workspace_layout = tokens
                .iter()
                .any(|token| matches!(token.as_str(), "workspace" | "crates"));
            let mut directory_scope_paths = if prefers_workspace_layout {
                vec!["crates".to_string()]
            } else {
                vec!["src".to_string()]
            };
            if tokens
                .iter()
                .any(|token| matches!(token.as_str(), "validate" | "validation" | "test" | "tests"))
            {
                directory_scope_paths.push("tests".to_string());
            }
            GreenfieldScaffoldSeed {
                entry_points: vec!["Cargo.toml".to_string()],
                file_scope_paths: vec!["Cargo.toml".to_string()],
                directory_scope_paths,
            }
        }
        GreenfieldScaffoldKind::Go => GreenfieldScaffoldSeed {
            entry_points: vec!["go.mod".to_string()],
            file_scope_paths: vec!["go.mod".to_string()],
            directory_scope_paths: vec![
                "cmd".to_string(),
                "internal".to_string(),
                "pkg".to_string(),
            ],
        },
        GreenfieldScaffoldKind::Python => GreenfieldScaffoldSeed {
            entry_points: vec!["pyproject.toml".to_string()],
            file_scope_paths: vec!["pyproject.toml".to_string()],
            directory_scope_paths: vec!["src".to_string(), "tests".to_string()],
        },
        GreenfieldScaffoldKind::Node => {
            let prefers_workspace_layout = tokens.iter().any(|token| {
                matches!(
                    token.as_str(),
                    "workspace" | "workspaces" | "monorepo" | "packages" | "apps"
                )
            });
            let prefers_typescript = tokens
                .iter()
                .any(|token| matches!(token.as_str(), "typescript" | "tsconfig" | "tsx" | "vite"));
            let mut file_scope_paths = vec!["package.json".to_string()];
            if prefers_typescript {
                file_scope_paths.push("tsconfig.json".to_string());
            }
            let mut directory_scope_paths = if prefers_workspace_layout {
                vec!["packages".to_string(), "apps".to_string()]
            } else {
                vec!["src".to_string()]
            };
            if tokens.iter().any(|token| {
                matches!(
                    token.as_str(),
                    "validate" | "validation" | "test" | "tests" | "check"
                )
            }) {
                directory_scope_paths.push("tests".to_string());
            }
            GreenfieldScaffoldSeed {
                entry_points: vec!["package.json".to_string()],
                file_scope_paths,
                directory_scope_paths,
            }
        }
    }
}

fn prepend_preferred_candidates(existing: &mut Vec<String>, preferred: &[String], limit: usize) {
    let mut merged = preferred.to_vec();
    for candidate in existing.drain(..) {
        if !merged.iter().any(|existing| existing == &candidate) {
            merged.push(candidate);
        }
    }
    *existing = merged.into_iter().take(limit).collect();
}

fn infer_entry_points(repo_root: &Path, prompt: &str, candidates: &[String]) -> Vec<String> {
    let tokens = prompt_tokens(prompt);
    let mut scored = candidates
        .iter()
        .filter_map(|candidate| {
            let path = repo_root.join(candidate);
            if !path.is_file() {
                return None;
            }
            let score = entry_point_score(repo_root, candidate, &tokens);
            (score > 0).then_some((candidate.clone(), score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut results = scored
        .into_iter()
        .map(|(path, _)| path)
        .take(5)
        .collect::<Vec<_>>();

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
    if let Some(workspace_member_packages) = workspace_member_package_names(repo_root)? {
        return Ok(workspace_member_packages);
    }
    let mut package_names = Vec::new();
    collect_rust_package_names(repo_root, repo_root, &mut package_names)?;
    dedupe(&mut package_names);
    Ok(package_names)
}

fn workspace_member_package_names(repo_root: &Path) -> Result<Option<Vec<String>>> {
    let cargo_toml = repo_root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("read {}", cargo_toml.display()))?;
    if !contents.lines().any(|line| line.trim() == "[workspace]") {
        return Ok(None);
    }
    let member_patterns = workspace_member_patterns(&contents);
    if member_patterns.is_empty() {
        return Ok(None);
    }
    let mut package_names = Vec::new();
    for pattern in member_patterns {
        for member_path in expand_workspace_member_pattern(repo_root, &pattern)? {
            let cargo_toml = if member_path.is_file() {
                member_path.clone()
            } else {
                member_path.join("Cargo.toml")
            };
            if !cargo_toml.exists() {
                continue;
            }
            if let Some(package_name) = cargo_package_name(&cargo_toml)? {
                package_names.push(package_name);
            }
        }
    }
    dedupe(&mut package_names);
    Ok(Some(package_names))
}

fn workspace_member_patterns(contents: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_workspace = false;
    let mut collecting_members = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_workspace = trimmed == "[workspace]";
            collecting_members = false;
            continue;
        }
        if !in_workspace {
            continue;
        }
        if collecting_members {
            for pattern in quoted_values(trimmed) {
                patterns.push(pattern);
            }
            if trimmed.contains(']') {
                collecting_members = false;
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("members") {
            let Some((_, value)) = rest.split_once('=') else {
                continue;
            };
            let value = value.trim();
            for pattern in quoted_values(value) {
                patterns.push(pattern);
            }
            if value.starts_with('[') && !value.contains(']') {
                collecting_members = true;
            }
        }
    }

    patterns
}

fn quoted_values(input: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut in_quote = false;
    let mut start = 0;
    for (idx, ch) in input.char_indices() {
        if ch != '"' {
            continue;
        }
        if in_quote {
            values.push(input[start..idx].to_string());
            in_quote = false;
        } else {
            in_quote = true;
            start = idx + ch.len_utf8();
        }
    }
    values
}

fn expand_workspace_member_pattern(repo_root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut paths = vec![repo_root.to_path_buf()];
    for segment in pattern.split('/').filter(|segment| !segment.is_empty()) {
        let mut next = Vec::new();
        for base in paths {
            if segment.contains('*') {
                for entry in fs::read_dir(&base)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if ignored_name(&name) {
                        continue;
                    }
                    if wildcard_match_segment(segment, &name) {
                        next.push(path);
                    }
                }
            } else {
                let candidate = base.join(segment);
                if candidate.exists() {
                    next.push(candidate);
                }
            }
        }
        paths = next;
        if paths.is_empty() {
            break;
        }
    }
    Ok(paths)
}

fn wildcard_match_segment(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    let mut remainder = value;
    let mut first = true;
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if first && !pattern.starts_with('*') {
            let Some(rest) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = rest;
            first = false;
            continue;
        }
        if idx == parts.len() - 1 && !pattern.ends_with('*') {
            return remainder.ends_with(part);
        }
        let Some(found) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[found + part.len()..];
        first = false;
    }
    true
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

fn entry_point_score(repo_root: &Path, candidate: &str, tokens: &[String]) -> i32 {
    let mut score = relevance_score(candidate, tokens);
    let stem = file_stem(candidate);
    if tokens.iter().any(|token| stem.contains(token)) {
        score += 6;
    }
    let content_score = content_relevance_score(repo_root, candidate, tokens, true);
    if candidate.ends_with("/main.rs") && content_score > 0 {
        score += 20;
    }
    score + content_score
}

fn entry_point_hint_score(repo_root: &Path, relative: &str, tokens: &[String]) -> i32 {
    if !relative.ends_with(".rs") {
        return 0;
    }
    let content_score = content_relevance_score(repo_root, relative, tokens, true);
    if content_score <= 0 {
        return 0;
    }
    let capped = content_score.min(120);
    if relative.ends_with("/main.rs") {
        capped + 20
    } else {
        capped / 2
    }
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
    scan: &RepoScanSummary,
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

    let explicit_target_checks =
        explicit_target_checks_override(repo_root, prompt).unwrap_or_default();
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

    let explicit_integrity_checks =
        explicit_integrity_checks_override(repo_root, prompt).unwrap_or_default();
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

    if explicit_paths.is_empty() {
        if proposal_misses_primary_candidate_entry_point(proposal, scan) {
            return true;
        }

        if broad_directory_scope_needs_file_fallback(repo_root, prompt, proposal, scan) {
            return true;
        }
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

fn broad_directory_scope_needs_file_fallback(
    repo_root: &Path,
    prompt: &str,
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
) -> bool {
    if scan.candidate_file_scope_paths.is_empty()
        || prompt_explicitly_requests_directory_scope(prompt)
    {
        return false;
    }
    if !proposal_prefers_file_scope(proposal, scan) {
        return false;
    }
    proposal
        .allowed_scope
        .iter()
        .any(|path| is_directory_scope_path(repo_root, path))
}

fn proposal_prefers_file_scope(proposal: &DraftProposal, scan: &RepoScanSummary) -> bool {
    proposal
        .entry_points
        .iter()
        .any(|path| is_file_like_scope(path))
        || proposal
            .allowed_scope
            .iter()
            .any(|path| is_file_like_scope(path))
        || !scan.candidate_entry_points.is_empty()
        || !scan.candidate_file_scope_paths.is_empty()
}

fn prompt_explicitly_requests_directory_scope(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    [
        "directory scope",
        "module scope",
        "package scope",
        "workspace scope",
        "whole module",
        "entire module",
        "entire package",
        "whole package",
        "directory-wide",
        "workspace-wide",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn is_directory_scope_path(repo_root: &Path, path: &str) -> bool {
    !is_file_like_scope(path) && repo_root.join(path).is_dir()
}

fn proposal_misses_primary_candidate_entry_point(
    proposal: &DraftProposal,
    scan: &RepoScanSummary,
) -> bool {
    let Some(primary) = scan
        .candidate_entry_points
        .iter()
        .find(|path| is_file_like_scope(path))
    else {
        return false;
    };
    !proposal.entry_points.iter().any(|path| path == primary)
        && !proposal.allowed_scope.iter().any(|path| path == primary)
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
        for candidate in scan
            .candidate_entry_points
            .iter()
            .filter(|path| is_fallback_source_path(repo_root, path))
            .take(2)
        {
            if validate_scope_path(repo_root, candidate).is_ok() {
                push_unique(&mut paths, candidate.clone());
            }
        }
    }

    if paths.is_empty() {
        for candidate in scan
            .candidate_file_scope_paths
            .iter()
            .filter(|path| is_fallback_source_path(repo_root, path))
            .filter(|path| !is_repo_root_file(path))
            .take(4)
        {
            if validate_scope_path(repo_root, candidate).is_ok() {
                push_unique(&mut paths, candidate.clone());
            }
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
    for candidate in scan_candidate_paths(scan) {
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
    for candidate in scan_candidate_paths(scan) {
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

fn scan_candidate_paths(scan: &RepoScanSummary) -> Vec<&String> {
    let mut candidates = Vec::new();
    for candidate in scan
        .candidate_entry_points
        .iter()
        .chain(scan.candidate_file_scope_paths.iter())
        .chain(scan.candidate_directory_scope_paths.iter())
        .chain(scan.candidate_scope_paths.iter())
    {
        if !candidates.iter().any(|existing| *existing == candidate) {
            candidates.push(candidate);
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
        || is_swiftpm_build_path(Path::new(path.trim()))
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
    if is_swiftpm_build_path(&relative) {
        return Err("must not point to generated build artifacts".to_string());
    }
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
    if allow_missing_greenfield_scaffold_path(&relative) {
        return Ok(());
    }
    Err("path does not exist and has no existing parent directory".to_string())
}

fn allow_missing_greenfield_scaffold_path(relative: &Path) -> bool {
    let mut components = relative.components();
    let Some(Component::Normal(first)) = components.next() else {
        return false;
    };
    if components.next().is_none() {
        return false;
    }
    matches!(
        first.to_str(),
        Some("crates" | "src" | "tests" | "cmd" | "internal" | "pkg" | "packages" | "apps")
    )
}

pub fn validate_check_command(repo_root: &Path, value: &str) -> std::result::Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("must be non-empty".to_string());
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("must be a single-line shell command".to_string());
    }

    let forbidden = [
        ';', '&', '|', '>', '<', '$', '(', ')', '`', '!', '*', '?', '[', ']', '{', '}',
    ];
    if trimmed.chars().any(|c| forbidden.contains(&c)) {
        return Err(
            "must not contain shell metacharacters, redirection, or glob patterns".to_string(),
        );
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if let Some(program) = tokens.first() {
        let allowed_programs = [
            "cargo", "npm", "pnpm", "yarn", "bun", "make", "pytest", "go", "swift", "python",
            "python3", "node", "deno", "rake", "gradle", "mvn", "ant", "true", "false",
        ];
        if !allowed_programs.contains(program) {
            return Err(format!(
                "program '{program}' is not in the allowlist of trusted check runners"
            ));
        }
    }
    for token in tokens {
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

fn content_relevance_score(
    repo_root: &Path,
    relative: &str,
    tokens: &[String],
    entry_point_mode: bool,
) -> i32 {
    let path = repo_root.join(relative);
    if !path.is_file() || tokens.is_empty() {
        return 0;
    }
    let Ok(contents) = fs::read_to_string(&path) else {
        return 0;
    };
    let mut score = 0;
    let lines = if entry_point_mode {
        contents.lines().collect::<Vec<_>>()
    } else {
        contents.lines().take(200).collect::<Vec<_>>()
    };
    for line in lines {
        score += content_line_score(line, tokens, entry_point_mode);
    }
    score
}

fn content_line_score(line: &str, tokens: &[String], entry_point_mode: bool) -> i32 {
    let lowered = line.to_ascii_lowercase();
    let mut score = 0;
    if is_symbol_like_line(line) {
        let match_count = symbol_token_match_count(line, tokens);
        if match_count > 0 {
            score += match_count * 5;
            if match_count >= 2 {
                score += 6;
            }
        }
    }
    if entry_point_mode && lowered.contains("fn cmd_") {
        let match_count = symbol_token_match_count(line, tokens);
        if match_count > 0 {
            score += 32;
        } else {
            score += 4;
        }
    }
    if !entry_point_mode && lowered.contains("fn cmd_") {
        let match_count = symbol_token_match_count(line, tokens);
        if match_count > 0 {
            score += 12;
        }
    }
    if lowered.contains("summarize_") {
        let match_count = symbol_token_match_count(line, tokens);
        if match_count > 0 {
            score += 10;
        }
    }
    score
}

fn is_symbol_like_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    [
        "fn ",
        "pub fn ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "impl ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn symbol_token_match_count(line: &str, tokens: &[String]) -> i32 {
    let lowered = line.to_ascii_lowercase();
    tokens
        .iter()
        .filter(|token| semantic_token_match(&lowered, token))
        .count() as i32
}

fn semantic_token_match(haystack: &str, token: &str) -> bool {
    if haystack.contains(token) {
        return true;
    }
    match token {
        "summary" => haystack.contains("summarize"),
        "summaries" => haystack.contains("summarize"),
        "eval" => haystack.contains("evals"),
        "status" => haystack.contains("cmd_status") || haystack.contains("status"),
        _ => false,
    }
}

fn explicit_scope_paths(prompt: &str, repo_root: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    for candidate in code_spans(prompt)
        .into_iter()
        .chain(path_like_tokens(prompt).into_iter())
        .chain(explicit_scaffold_dir_tokens(prompt).into_iter())
    {
        if !looks_like_repo_path(&candidate) {
            continue;
        }
        let explicitly_reviewed_generated_path = is_generated_runtime_artifact_path(&candidate)
            && prompt_explicitly_targets_generated_path(prompt, &candidate);
        if is_artifact_storage_pattern(&candidate) && !explicitly_reviewed_generated_path {
            continue;
        }
        if is_generated_runtime_artifact_path(&candidate) && !explicitly_reviewed_generated_path {
            continue;
        }
        if !candidate.contains('/')
            && !is_repo_root_file(&candidate)
            && !is_top_level_scaffold_dir(&candidate)
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

fn explicit_scaffold_dir_tokens(prompt: &str) -> Vec<String> {
    if !prompt_declares_explicit_touch_set(prompt) {
        return Vec::new();
    }
    prompt
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '`' | '"' | '\'' | ',' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
                )
        })
        .map(|token| token.trim_end_matches('.'))
        .filter(|token| {
            matches!(
                *token,
                "crates" | "src" | "tests" | "cmd" | "internal" | "pkg" | "packages" | "apps"
            )
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn prompt_declares_explicit_touch_set(prompt: &str) -> bool {
    let lowered = prompt.to_ascii_lowercase();
    [
        "touching exactly",
        "touch exactly",
        "exact touch set",
        "requested touch set",
        "scope bounded to",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

fn is_generated_runtime_artifact_path(path: &str) -> bool {
    let normalized = path.trim().trim_matches('`').replace('\\', "/");
    normalized == ".punk/project/harness.json"
        || normalized.starts_with(".punk/project/")
        || normalized.contains("/.punk/project/")
}

fn is_top_level_scaffold_dir(path: &str) -> bool {
    matches!(
        path,
        "crates" | "src" | "tests" | "cmd" | "internal" | "pkg" | "packages" | "apps"
    )
}

fn prompt_explicitly_targets_generated_path(prompt: &str, path: &str) -> bool {
    let prompt = prompt.to_ascii_lowercase();
    let path = path
        .trim()
        .trim_matches('`')
        .replace('\\', "/")
        .to_ascii_lowercase();
    [
        format!("review `{path}`"),
        format!("review {path}"),
        format!("code review target `{path}`"),
        format!("code review target {path}"),
        format!("code-review target `{path}`"),
        format!("code-review target {path}"),
        format!("explicit review target `{path}`"),
        format!("explicit review target {path}"),
        format!("explicitly review `{path}`"),
        format!("explicitly review {path}"),
    ]
    .into_iter()
    .any(|needle| prompt.contains(&needle))
}

#[cfg(test)]
mod generated_runtime_artifact_scope_tests {
    use super::explicit_scope_paths;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn explicit_scope_paths_excludes_generated_runtime_artifacts_from_prompt_scope() {
        let repo_root = create_temp_repo_root("exclude-generated-runtime-artifacts");
        seed_scope_fixture(&repo_root, "crates/punk-orch/src/lib.rs");
        seed_scope_fixture(&repo_root, "crates/punk-cli/src/main.rs");
        seed_scope_fixture(&repo_root, ".punk/project/harness.json");

        let prompt = "Review crates/punk-orch/src/lib.rs and crates/punk-cli/src/main.rs before writing .punk/project/harness.json as the generated runtime packet destination.";
        let paths = explicit_scope_paths(prompt, &repo_root);

        assert_eq!(
            paths,
            vec![
                "crates/punk-orch/src/lib.rs".to_string(),
                "crates/punk-cli/src/main.rs".to_string()
            ]
        );
        assert!(!paths
            .iter()
            .any(|path| path == ".punk/project/harness.json"));
    }

    #[test]
    fn explicit_scope_paths_keeps_generated_runtime_artifacts_when_explicitly_reviewed() {
        let repo_root = create_temp_repo_root("preserve-explicit-generated-runtime-review");
        seed_scope_fixture(&repo_root, ".punk/project/harness.json");

        let prompt =
            "Code-review target .punk/project/harness.json and confirm the generated runtime packet is valid.";
        let paths = explicit_scope_paths(prompt, &repo_root);

        assert_eq!(paths, vec![".punk/project/harness.json".to_string()]);
    }

    fn create_temp_repo_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("punk-core-{label}-{nanos}"));
        fs::create_dir_all(&root).expect("create repo root");
        root
    }

    fn seed_scope_fixture(repo_root: &Path, relative_path: &str) {
        let path = repo_root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, "fixture\n").expect("write fixture file");
    }
}

fn authoritative_exact_scope_paths(prompt: &str, repo_root: &Path) -> Vec<String> {
    let lowered = prompt.to_ascii_lowercase();
    let anchors = [
        "allowed_scope exactly",
        "allowed scope exactly",
        "final allowed_scope must contain exactly",
        "final allowed scope must contain exactly",
        "restrict allowed_scope exactly",
        "restrict allowed scope exactly",
    ];
    let stop_markers = [
        "\ndo not ",
        " do not ",
        "\ndon't ",
        " don't ",
        "\ntarget checks",
        " target checks",
        "\nintegrity checks",
        " integrity checks",
        "\nthe regression",
        " the regression",
        "\nadd a focused regression",
        " add a focused regression",
        "\nupdate only ",
        " update only ",
        "\nkeep this ",
        " keep this ",
    ];

    for anchor in anchors {
        let Some(anchor_idx) = lowered.find(anchor) else {
            continue;
        };
        let tail = &prompt[anchor_idx..];
        let Some(colon_rel) = tail.find(':') else {
            continue;
        };
        let start = anchor_idx + colon_rel + 1;
        let end = stop_markers
            .iter()
            .filter_map(|marker| lowered[start..].find(marker).map(|idx| start + idx))
            .min()
            .unwrap_or(prompt.len());
        let explicit = explicit_scope_paths(prompt[start..end].trim(), repo_root);
        if !explicit.is_empty() {
            return explicit;
        }
    }

    Vec::new()
}

fn explicit_scope_override(prompt: &str, repo_root: &Path) -> Option<Vec<String>> {
    let exact_scope = authoritative_exact_scope_paths(prompt, repo_root);
    if !exact_scope.is_empty() {
        return Some(exact_scope);
    }
    let explicit_scope = explicit_scope_paths(prompt, repo_root);
    (!explicit_scope.is_empty()).then_some(explicit_scope)
}

fn explicit_entry_points_override(explicit_scope: &[String]) -> Option<Vec<String>> {
    let explicit_entry_points = explicit_scope
        .iter()
        .filter(|path| is_file_like_scope(path))
        .cloned()
        .collect::<Vec<_>>();
    (!explicit_entry_points.is_empty()).then_some(explicit_entry_points)
}

fn explicit_target_checks_override(repo_root: &Path, prompt: &str) -> Option<Vec<String>> {
    let explicit_target_checks =
        explicit_checks_from_prompt(repo_root, prompt, target_check_markers());
    (!explicit_target_checks.is_empty()).then_some(explicit_target_checks)
}

fn explicit_integrity_checks_override(repo_root: &Path, prompt: &str) -> Option<Vec<String>> {
    let explicit_integrity_checks =
        explicit_checks_from_prompt(repo_root, prompt, integrity_check_markers());
    (!explicit_integrity_checks.is_empty()).then_some(explicit_integrity_checks)
}

fn target_check_markers() -> &'static [&'static str] {
    &[
        "target_checks must contain exactly one command:",
        "target checks must contain exactly one command:",
        "replace target_checks with exactly one command:",
        "replace target checks with exactly one command:",
        "target_checks must contain exactly:",
        "target checks must contain exactly:",
        "replace target_checks with exactly:",
        "replace target checks with exactly:",
        "target_checks should include",
        "target checks should include",
        "target checks to satisfy",
        "target checks:",
        "target_checks:",
    ]
}

fn integrity_check_markers() -> &'static [&'static str] {
    &[
        "integrity_checks must contain exactly one command:",
        "integrity checks must contain exactly one command:",
        "replace integrity_checks with exactly one command:",
        "replace integrity checks with exactly one command:",
        "integrity_checks must contain exactly:",
        "integrity checks must contain exactly:",
        "replace integrity_checks with exactly:",
        "replace integrity checks with exactly:",
        "integrity_checks should include",
        "integrity checks should include",
        "integrity checks to keep passing",
        "integrity checks:",
        "integrity_checks:",
    ]
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
            "Cargo.toml"
                | "Cargo.lock"
                | "README.md"
                | "rust-toolchain.toml"
                | "crates"
                | "src"
                | "tests"
                | "cmd"
                | "internal"
                | "pkg"
                | "packages"
                | "apps"
        )
        || [".rs", ".toml", ".json", ".md", ".yaml", ".yml"]
            .iter()
            .any(|suffix| value.ends_with(suffix))
}

fn trim_token_punctuation(token: &str) -> &str {
    token
        .trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | ',' | ':' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        })
        .trim_end_matches('.')
        .trim_end_matches(|c: char| {
            matches!(c, '`' | '"' | '\'' | ',' | ':' | ';' | ')' | ']' | '}')
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
            .candidate_file_scope_paths
            .first()
            .is_some_and(|path| path.contains("auth")));
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "src/auth"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_repo_separates_file_and_directory_candidates() {
        let root =
            std::env::temp_dir().join(format!("punk-core-scope-split-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-orch/src")).unwrap();
        fs::write(
            root.join("punk/punk-orch/src/eval.rs"),
            "pub fn summarize_skill_evals() {}\n",
        )
        .unwrap();
        fs::write(root.join("punk/punk-orch/src/lib.rs"), "pub mod eval;\n").unwrap();

        let summary = scan_repo(&root, "reuse existing eval summary helpers").unwrap();
        assert!(summary
            .candidate_file_scope_paths
            .iter()
            .any(|path| path == "punk/punk-orch/src/eval.rs"));
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "punk/punk-orch/src"));
        assert!(summary
            .candidate_scope_paths
            .first()
            .is_some_and(|path| path.ends_with(".rs")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn infer_entry_points_prefers_cli_status_handler_and_eval_helper() {
        let root =
            std::env::temp_dir().join(format!("punk-core-entry-points-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("punk/punk-orch/src/eval.rs"),
            "pub fn summarize_skill_evals() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-orch/src/skill.rs"),
            "pub fn promote_skill() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/src/main.rs"),
            "fn cmd_status() {}\nfn cmd_eval_skill_summary() {}\n",
        )
        .unwrap();

        let prompt =
            "Add a skill eval summary line to nested punk status and reuse existing eval summary helpers.";
        let summary = scan_repo(&root, prompt).unwrap();
        assert!(summary
            .candidate_entry_points
            .iter()
            .any(|path| path == "punk/punk-run/src/main.rs"));
        assert!(summary
            .candidate_entry_points
            .iter()
            .any(|path| path == "punk/punk-orch/src/eval.rs"));
        assert!(summary
            .candidate_entry_points
            .first()
            .is_some_and(|path| path == "punk/punk-run/src/main.rs"));

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
        assert!(summary
            .candidate_file_scope_paths
            .iter()
            .all(|path| !path.starts_with("docs/reference-repos")));
        assert!(summary
            .candidate_directory_scope_paths
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
            .candidate_file_scope_paths
            .iter()
            .all(|path| !path.starts_with("docs/research/_delve_runs")));
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .all(|path| !path.starts_with("docs/research/_delve_runs")));
        assert!(summary
            .candidate_entry_points
            .iter()
            .all(|path| !path.starts_with("docs/research/_delve_runs")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scope_candidates_ignore_swiftpm_build_artifacts() {
        let root =
            std::env::temp_dir().join(format!("punk-core-swiftpm-build-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/Sources/InterviewCoachDevUI"))
            .unwrap();
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests"))
            .unwrap();
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/.build/debug")).unwrap();
        fs::write(
            root.join("Packages/InterviewCoachKit/Package.swift"),
            "// swift package manifest\n",
        )
        .unwrap();
        fs::write(
            root.join(
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift",
            ),
            "struct MainWindowView {}\n",
        )
        .unwrap();
        fs::write(
            root.join("Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift"),
            "func testDevUI() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("Packages/InterviewCoachKit/.build/debug/Generated.swift"),
            "struct GeneratedArtifact {}\n",
        )
        .unwrap();

        let summary = scan_repo(
            &root,
            "Add trace panel to InterviewCoachDevUI MainWindowView",
        )
        .unwrap();
        assert!(summary
            .candidate_scope_paths
            .iter()
            .all(|path| !path.contains(".build/")));
        assert!(summary
            .candidate_file_scope_paths
            .iter()
            .all(|path| !path.contains(".build/")));
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .all(|path| !path.contains(".build/")));
        assert!(summary
            .candidate_file_scope_paths
            .iter()
            .any(|path| path.ends_with("MainWindowView.swift")));

        let proposal = DraftProposal {
            title: "Add trace panel".into(),
            summary: "Keep scope in real sources only".into(),
            entry_points: vec![
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift"
                    .into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["Trace panel remains mock-only".into()],
            behavior_requirements: vec!["Show latest trace events".into()],
            allowed_scope: vec!["Packages/InterviewCoachKit/.build/debug/Generated.swift".into()],
            target_checks: vec!["swift test".into()],
            integrity_checks: vec!["swift test".into()],
            risk_level: "low".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        assert!(errors.iter().any(|error| {
            error.field.starts_with("allowed_scope")
                && error.message.contains("generated build artifacts")
        }));

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
    fn rust_target_checks_ignore_nested_non_member_workspace_packages() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-workspace-members-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/Cargo.toml"),
            "[package]\nname = \"punk-orch\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-core/Cargo.toml"),
            "[package]\nname = \"punk-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/Cargo.toml"),
            "[package]\nname = \"punk-run\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let summary = scan_repo(&root, "tighten run reporting in punk-orch").unwrap();
        assert!(summary
            .candidate_target_checks
            .contains(&"cargo test -p punk-orch".to_string()));
        assert!(!summary
            .candidate_target_checks
            .contains(&"cargo test -p punk-run".to_string()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_greenfield_rust_prompt_infers_initial_workspace_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-rust-bootstrap-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();

        let summary = scan_repo(
            &root,
            "scaffold Rust workspace and implement pubpunk init + validate",
        )
        .unwrap();
        assert_eq!(
            summary.candidate_target_checks,
            vec!["cargo test --workspace".to_string()]
        );
        assert_eq!(
            summary.candidate_integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );
        assert!(summary
            .notes
            .iter()
            .any(|note| note.contains("inferred initial Rust bootstrap checks")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_greenfield_rust_prompt_prefers_scaffoldable_scope_candidates_over_docs() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-rust-scope-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("archive")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/PUBPUNK_DEVELOPMENT_HANDOFF.md"),
            "scaffold Rust workspace and implement pubpunk init + validate\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/IMPLEMENTATION_PLAN.md"),
            "workspace scaffold validate init plan\n",
        )
        .unwrap();
        fs::write(root.join("archive/pubpunk-docs.zip"), "zip placeholder\n").unwrap();

        let summary = scan_repo(
            &root,
            "scaffold Rust workspace and implement pubpunk init + validate",
        )
        .unwrap();
        assert_eq!(
            summary.candidate_entry_points.first().map(String::as_str),
            Some("Cargo.toml")
        );
        assert_eq!(
            summary
                .candidate_file_scope_paths
                .first()
                .map(String::as_str),
            Some("Cargo.toml")
        );
        assert_eq!(
            summary
                .candidate_directory_scope_paths
                .first()
                .map(String::as_str),
            Some("crates")
        );
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "tests"));
        assert!(summary
            .notes
            .iter()
            .any(|note| { note.contains("preferring scaffoldable Rust scope candidates") }));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_greenfield_non_rust_prompt_does_not_infer_checks() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-non-rust-bootstrap-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();

        let summary = scan_repo(&root, "write launch copy for landing page").unwrap();
        assert!(summary.candidate_target_checks.is_empty());
        assert!(summary.candidate_integrity_checks.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_generic_init_verbs_do_not_trigger_greenfield_scaffold() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-generic-init-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();

        let rust_summary = scan_repo(&root, "initialize config loading in Rust").unwrap();
        assert!(rust_summary.candidate_target_checks.is_empty());
        assert!(rust_summary.candidate_integrity_checks.is_empty());
        assert!(!rust_summary
            .candidate_entry_points
            .iter()
            .any(|path| path == "Cargo.toml"));

        let go_summary = scan_repo(&root, "implement a Go init helper").unwrap();
        assert!(go_summary.candidate_target_checks.is_empty());
        assert!(go_summary.candidate_integrity_checks.is_empty());
        assert!(!go_summary
            .candidate_entry_points
            .iter()
            .any(|path| path == "go.mod"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_greenfield_go_prompt_infers_initial_checks_and_scope() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-go-bootstrap-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/IMPLEMENTATION_PLAN.md"),
            "scaffold go module and validate pubpunk init\n",
        )
        .unwrap();

        let summary = scan_repo(
            &root,
            "scaffold Go module and implement pubpunk init + validate",
        )
        .unwrap();
        assert_eq!(
            summary.candidate_target_checks,
            vec!["go test ./...".to_string()]
        );
        assert_eq!(
            summary.candidate_integrity_checks,
            vec!["go test ./...".to_string()]
        );
        assert_eq!(
            summary.candidate_entry_points.first().map(String::as_str),
            Some("go.mod")
        );
        assert_eq!(
            summary
                .candidate_file_scope_paths
                .first()
                .map(String::as_str),
            Some("go.mod")
        );
        assert_eq!(
            summary
                .candidate_directory_scope_paths
                .first()
                .map(String::as_str),
            Some("cmd")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_greenfield_python_prompt_infers_initial_checks_and_scope() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-python-bootstrap-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/SPEC.md"),
            "scaffold python package and validate pubpunk init\n",
        )
        .unwrap();

        let summary = scan_repo(
            &root,
            "scaffold Python package and implement pubpunk init + validate",
        )
        .unwrap();
        assert_eq!(summary.candidate_target_checks, vec!["pytest".to_string()]);
        assert_eq!(
            summary.candidate_integrity_checks,
            vec!["pytest".to_string()]
        );
        assert_eq!(
            summary.candidate_entry_points.first().map(String::as_str),
            Some("pyproject.toml")
        );
        assert_eq!(
            summary
                .candidate_file_scope_paths
                .first()
                .map(String::as_str),
            Some("pyproject.toml")
        );
        assert_eq!(
            summary
                .candidate_directory_scope_paths
                .first()
                .map(String::as_str),
            Some("src")
        );
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "tests"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrapped_greenfield_node_prompt_infers_initial_checks_and_scope() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-node-bootstrap-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "# Agent start\n").unwrap();
        fs::write(
            root.join(".punk/bootstrap/pubpunk-core.md"),
            "bootstrap guidance\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/SPEC.md"),
            "scaffold TypeScript package and validate pubpunk init\n",
        )
        .unwrap();

        let summary = scan_repo(
            &root,
            "scaffold TypeScript package and implement pubpunk init + validate",
        )
        .unwrap();
        assert_eq!(
            summary.candidate_target_checks,
            vec!["npm test".to_string()]
        );
        assert_eq!(
            summary.candidate_integrity_checks,
            vec!["npm test".to_string()]
        );
        assert_eq!(
            summary.candidate_entry_points.first().map(String::as_str),
            Some("package.json")
        );
        assert_eq!(
            summary
                .candidate_file_scope_paths
                .first()
                .map(String::as_str),
            Some("package.json")
        );
        assert!(summary
            .candidate_file_scope_paths
            .iter()
            .any(|path| path == "tsconfig.json"));
        assert_eq!(
            summary
                .candidate_directory_scope_paths
                .first()
                .map(String::as_str),
            Some("src")
        );
        assert!(summary
            .candidate_directory_scope_paths
            .iter()
            .any(|path| path == "tests"));

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
    fn validation_accepts_scaffold_paths_without_existing_scaffold_root() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-scaffold-missing-root-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let proposal = DraftProposal {
            title: "bootstrap workspace".into(),
            summary: "bootstrap workspace".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec!["crates/pubpunk-cli".into(), "crates/pubpunk-core".into()],
            expected_interfaces: vec!["workspace bootstrap".into()],
            behavior_requirements: vec!["create workspace members".into()],
            allowed_scope: vec![
                "Cargo.toml".into(),
                "crates/pubpunk-cli".into(),
                "crates/pubpunk-core".into(),
                "tests".into(),
            ],
            target_checks: vec!["cargo test --workspace".into()],
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
    fn canonicalize_preserves_explicit_bootstrap_touch_set_without_existing_crates_dir() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-bootstrap-explicit-scope-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let prompt = "bootstrap initial Rust workspace for pubpunk touching exactly Cargo.toml, crates/pubpunk-cli, crates/pubpunk-core, and tests; create workspace members and make cargo test --workspace pass";
        let mut proposal = DraftProposal {
            title: "wrong".into(),
            summary: "wrong".into(),
            entry_points: vec!["Cargo.toml".into()],
            import_paths: vec!["crates/pubpunk-cli".into(), "crates/pubpunk-core".into()],
            expected_interfaces: vec!["initial Rust scaffold".into()],
            behavior_requirements: vec!["bootstrap workspace".into()],
            allowed_scope: vec!["Cargo.toml".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
        };

        canonicalize_draft_proposal(&root, prompt, &mut proposal);

        assert_eq!(
            proposal.allowed_scope,
            vec![
                "Cargo.toml".to_string(),
                "crates/pubpunk-cli".to_string(),
                "crates/pubpunk-core".to_string(),
                "tests".to_string(),
            ]
        );
        assert_eq!(proposal.entry_points, vec!["Cargo.toml".to_string()]);

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
    fn explicit_prompt_overrides_replace_scope_and_checks() {
        let root =
            std::env::temp_dir().join(format!("punk-core-refine-overrides-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/Sources/InterviewCoachDevUI"))
            .unwrap();
        fs::create_dir_all(root.join("Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests"))
            .unwrap();
        fs::write(
            root.join(
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/DevAppViewModel.swift",
            ),
            "struct DevAppViewModel {}\n",
        )
        .unwrap();
        fs::write(
            root.join(
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift",
            ),
            "struct MainWindowView {}\n",
        )
        .unwrap();
        fs::write(
            root.join("Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift"),
            "func testExample() {}\n",
        )
        .unwrap();
        fs::write(root.join("Makefile"), "test:\n\t@echo ok\n").unwrap();

        let guidance = "Expand allowed_scope exactly to the files needed for the copy/export trace slice: `Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/DevAppViewModel.swift`; `Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift`; `Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift`. Target checks should include make test. Integrity checks should include make test.";
        let mut proposal = DraftProposal {
            title: "trace copy/export".into(),
            summary: "trace copy/export".into(),
            entry_points: vec![
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/LegacyView.swift".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec!["Packages/InterviewCoachKit/Sources/InterviewCoachDevUI".into()],
            target_checks: vec!["swift test".into()],
            integrity_checks: vec!["swift test".into()],
            risk_level: "low".into(),
        };

        apply_explicit_prompt_overrides(&root, guidance, &mut proposal);

        assert_eq!(
            proposal.allowed_scope,
            vec![
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/DevAppViewModel.swift"
                    .to_string(),
                "Packages/InterviewCoachKit/Sources/InterviewCoachDevUI/MainWindowView.swift"
                    .to_string(),
                "Packages/InterviewCoachKit/Tests/InterviewCoachDevUITests/DevAppViewModelTests.swift"
                    .to_string(),
            ]
        );
        assert_eq!(proposal.entry_points, proposal.allowed_scope);
        assert_eq!(proposal.target_checks, vec!["make test".to_string()]);
        assert_eq!(proposal.integrity_checks, vec!["make test".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_scope_override_keeps_exact_list_without_readding_excluded_path_mentions() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-exact-scope-negative-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("crates/punk-core/src/lib.rs"),
            "pub fn core() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();
        fs::write(root.join("punk/punk-run/src/main.rs"), "fn main() {}\n").unwrap();

        let guidance = "Restrict allowed_scope exactly to these two files and nothing else: crates/punk-core/src/lib.rs; crates/punk-orch/src/lib.rs. Do not include punk/punk-run in allowed_scope.";
        let explicit = explicit_scope_override(guidance, &root).unwrap();

        assert_eq!(
            explicit,
            vec![
                "crates/punk-core/src/lib.rs".to_string(),
                "crates/punk-orch/src/lib.rs".to_string(),
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_scope_override_ignores_generated_runtime_artifact_paths() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-exact-scope-generated-artifact-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-cli/src")).unwrap();
        fs::create_dir_all(root.join(".punk/project")).unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();
        fs::write(root.join("crates/punk-cli/src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join(".punk/project/harness.json"), "{}\n").unwrap();

        let guidance = "Restrict allowed_scope exactly to these two files and nothing else: crates/punk-orch/src/lib.rs; crates/punk-cli/src/main.rs. Mention the generated packet `.punk/project/harness.json` in docs, but do not include it in allowed_scope or entry_points because it is a runtime artifact destination.";
        let explicit = explicit_scope_override(guidance, &root).unwrap();

        assert_eq!(
            explicit,
            vec![
                "crates/punk-orch/src/lib.rs".to_string(),
                "crates/punk-cli/src/main.rs".to_string(),
            ]
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_target_checks_override_preserves_exact_combined_command() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-exact-target-checks-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/punk-orch/src")).unwrap();
        fs::write(
            root.join("crates/punk-core/src/lib.rs"),
            "pub fn core() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-orch/src/lib.rs"),
            "pub fn orch() {}\n",
        )
        .unwrap();

        let guidance = "Keep allowed_scope exactly as-is. target_checks must contain exactly one command: cargo test -p punk-core -p punk-orch. integrity_checks must contain exactly one command: cargo test --workspace. Remove every other target check.";
        let mut proposal = DraftProposal {
            title: "tighten exact target checks".into(),
            summary: "tighten exact target checks".into(),
            entry_points: vec!["crates/punk-orch/src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["x".into()],
            behavior_requirements: vec!["x".into()],
            allowed_scope: vec![
                "crates/punk-core/src/lib.rs".into(),
                "crates/punk-orch/src/lib.rs".into(),
            ],
            target_checks: vec![
                "cargo test -p punk-core".into(),
                "cargo test -p punk-orch".into(),
                "cargo test -p punk-domain".into(),
            ],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "low".into(),
        };

        apply_explicit_prompt_overrides(&root, guidance, &mut proposal);

        assert_eq!(
            proposal.target_checks,
            vec!["cargo test -p punk-core -p punk-orch".to_string()]
        );
        assert_eq!(
            proposal.integrity_checks,
            vec!["cargo test --workspace".to_string()]
        );

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
    fn bounded_fallback_preserves_greenfield_rust_scaffold_scope_for_plain_init_goal() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-greenfield-rust-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".punk/bootstrap")).unwrap();
        fs::write(root.join(".punk/bootstrap/pubpunk-core.md"), "bootstrap\n").unwrap();
        fs::write(root.join(".punk/AGENT_START.md"), "agent start\n").unwrap();

        let prompt = "scaffold Rust workspace and implement pubpunk init command with --json output and tests";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "pubpunk init".into(),
            summary: "broken plain-goal fallback".into(),
            entry_points: vec!["Cargo.toml".into(), "crates/pubpunk-cli/src/main.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec![
                "A Rust workspace with a `pubpunk` CLI crate.".into(),
                "A `pubpunk init` command exposed through the CLI.".into(),
            ],
            behavior_requirements: vec![
                "Scaffold a minimal Rust workspace rooted at Cargo.toml.".into(),
                "Implement a conservative `pubpunk init` command with `--json` output.".into(),
                "Add tests covering the `init` command and its JSON output.".into(),
            ],
            allowed_scope: vec!["tests".into()],
            target_checks: vec!["cargo test --workspace".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert!(fallback.allowed_scope.contains(&"Cargo.toml".to_string()));
        assert!(fallback.allowed_scope.contains(&"crates".to_string()));
        assert!(fallback.allowed_scope.contains(&"tests".to_string()));
        assert!(fallback.entry_points.contains(&"Cargo.toml".to_string()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_replaces_directory_scope_with_concrete_file_candidates() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-directory-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("punk/punk-orch/src/eval.rs"),
            "pub fn summarize_skill_evals() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/src/main.rs"),
            "fn cmd_status() {}\n",
        )
        .unwrap();
        fs::write(root.join("punk/punk-orch/src/lib.rs"), "pub mod eval;\n").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"punk/*\"]\n",
        )
        .unwrap();

        let prompt = "Add a skill eval summary line to nested punk status and reuse existing eval summary helpers. Keep the slice bounded to files.";
        let scan = scan_repo(&root, prompt).unwrap();
        let proposal = DraftProposal {
            title: "status eval summary".into(),
            summary: "status eval summary".into(),
            entry_points: vec!["punk/punk-run/src/main.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["status output".into()],
            behavior_requirements: vec!["print skill eval summary".into()],
            allowed_scope: vec!["punk/punk-run/src".into(), "punk/punk-orch/src".into()],
            target_checks: vec!["cargo test -p punk-run".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope.first().map(String::as_str),
            Some("punk/punk-run/src/main.rs")
        );
        assert_eq!(
            fallback.allowed_scope.get(1).map(String::as_str),
            Some("punk/punk-orch/src/eval.rs")
        );
        assert!(fallback
            .allowed_scope
            .iter()
            .all(|path| path.ends_with(".rs")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn bounded_fallback_recovers_primary_candidate_entry_point_when_missing() {
        let root = std::env::temp_dir().join(format!(
            "punk-core-primary-entry-fallback-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("punk/punk-orch/src/eval.rs"),
            "pub fn summarize_skill_evals() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-orch/src/skill.rs"),
            "pub fn promote_skill() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/src/main.rs"),
            "fn cmd_status() {}\nfn cmd_eval_skill_summary() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"punk/*\"]\n",
        )
        .unwrap();

        let prompt =
            "Add a skill eval summary line to nested punk status and reuse existing eval summary helpers. Keep the slice bounded and additive.";
        let scan = scan_repo(&root, prompt).unwrap();
        assert_eq!(
            scan.candidate_entry_points.first().map(String::as_str),
            Some("punk/punk-run/src/main.rs")
        );
        let proposal = DraftProposal {
            title: "status eval summary".into(),
            summary: "status eval summary".into(),
            entry_points: vec![
                "punk/punk-orch/src/skill.rs".into(),
                "punk/punk-orch/src/eval.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["status output".into()],
            behavior_requirements: vec!["print skill eval summary".into()],
            allowed_scope: vec![
                "punk/punk-orch/src/skill.rs".into(),
                "punk/punk-orch/src/eval.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-orch".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
        };
        let errors = validate_draft_proposal(&root, &proposal);
        let fallback =
            build_bounded_fallback_proposal(&root, prompt, &proposal, &scan, &errors).unwrap();

        assert_eq!(
            fallback.allowed_scope,
            vec![
                "punk/punk-run/src/main.rs".to_string(),
                "punk/punk-orch/src/eval.rs".to_string(),
            ]
        );
        assert_eq!(fallback.entry_points, fallback.allowed_scope);

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

    #[test]
    fn validate_check_command_rejects_shell_fragments_and_untrusted_runners() {
        let root =
            std::env::temp_dir().join(format!("punk-core-check-guard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        assert!(validate_check_command(&root, "cargo test -p punk-core").is_ok());
        assert!(validate_check_command(&root, "true").is_ok());

        let shell_fragment = validate_check_command(&root, "cargo test; touch hacked")
            .expect_err("shell metacharacters should be rejected");
        assert!(shell_fragment.contains("must not contain shell metacharacters"));

        let untrusted_runner = validate_check_command(&root, "sh -c true")
            .expect_err("untrusted shell runner should be rejected");
        assert!(untrusted_runner.contains("allowlist of trusted check runners"));

        let traversal = validate_check_command(&root, "cargo test ../outside")
            .expect_err("parent traversal should be rejected");
        assert!(traversal.contains("outside repo root"));

        let _ = fs::remove_dir_all(&root);
    }
}
