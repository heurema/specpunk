use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use punk_domain::Contract;
use serde::{Deserialize, Serialize};

const RUST_ENTRY_POINT_STUB_HEADER: &str =
    "// Approved new entry-point file for this bounded contract.";
const ENTRY_POINT_MASK_MANIFEST: &str = ".punk/entry-point-mask.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextFileExcerpt {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub truncated_at_test_boundary: bool,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextRecipeFileSeed {
    pub path: String,
    pub role: String,
    pub edit_targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextRecipeSeed {
    pub title: String,
    pub summary: String,
    pub files: Vec<ContextRecipeFileSeed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextPatchSeedFile {
    pub path: String,
    pub purpose: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContextPatchSeed {
    pub title: String,
    pub summary: String,
    pub files: Vec<ContextPatchSeedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ContextPack {
    pub files: Vec<ContextFileExcerpt>,
    pub missing_paths: Vec<String>,
    pub recipe_seed: Option<ContextRecipeSeed>,
    pub patch_seed: Option<ContextPatchSeed>,
}

pub(crate) fn build_context_pack(repo_root: &Path, contract: &Contract) -> Result<ContextPack> {
    let mut pack = ContextPack::default();
    let mut included_paths = BTreeSet::new();

    for path in contract
        .entry_points
        .iter()
        .chain(contract.allowed_scope.iter())
    {
        if !included_paths.insert(path.clone()) {
            continue;
        }

        if !is_path_in_allowed_scope(path, &contract.allowed_scope) {
            continue;
        }

        if !path.ends_with(".rs") {
            continue;
        }

        let file_path = repo_root.join(path);
        if !file_path.exists() {
            if contract.entry_points.iter().any(|entry| entry == path) {
                pack.missing_paths.push(path.clone());
            }
            continue;
        }

        let source = fs::read_to_string(&file_path)
            .with_context(|| format!("read context-pack file {path}"))?;
        pack.files.push(build_rust_excerpt(path, &source));
    }

    pack.recipe_seed = build_recipe_seed(contract);
    pack.patch_seed = build_patch_seed(contract);

    Ok(pack)
}

pub(crate) fn ensure_retry_patch_seed(
    repo_root: &Path,
    contract: &Contract,
    pack: &mut ContextPack,
) {
    if pack.patch_seed.is_some() {
        return;
    }
    if !is_existing_file_retry_candidate(repo_root, contract) {
        return;
    }

    if is_punk_latest_run_triage_recipe(contract) {
        pack.patch_seed = Some(punk_latest_run_triage_patch_seed());
        return;
    }

    if let Some(seed) = generic_existing_file_patch_seed(contract, &pack.files) {
        pack.patch_seed = Some(seed);
    }
}

pub(crate) fn materialize_missing_entry_points(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Vec<String>> {
    let mut created = Vec::new();

    for entry_point in &contract.entry_points {
        if !is_path_in_allowed_scope(entry_point, &contract.allowed_scope) {
            continue;
        }
        if !entry_point.ends_with(".rs") {
            continue;
        }

        let file_path = repo_root.join(entry_point);
        if file_path.exists() {
            continue;
        }

        let Some(parent) = file_path.parent() else {
            continue;
        };
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent dirs for missing entry point {entry_point}"))?;
        fs::write(
            &file_path,
            render_rust_entry_point_stub(contract, entry_point),
        )
        .with_context(|| format!("materialize missing entry point {entry_point}"))?;
        created.push(entry_point.clone());
    }

    Ok(created)
}

pub(crate) fn restore_missing_materialized_entry_points(
    repo_root: &Path,
    contract: &Contract,
    paths: &[String],
) -> Result<Vec<String>> {
    let mut restored = Vec::new();

    for path in paths {
        if !path.ends_with(".rs") {
            continue;
        }

        let file_path = repo_root.join(path);
        if file_path.exists() {
            continue;
        }

        let Some(parent) = file_path.parent() else {
            continue;
        };
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent dirs for restored entry point {path}"))?;
        fs::write(&file_path, render_rust_entry_point_stub(contract, path))
            .with_context(|| format!("restore missing materialized entry point {path}"))?;
        restored.push(path.clone());
    }

    Ok(restored)
}

pub(crate) fn scaffold_only_entry_points(
    repo_root: &Path,
    contract: &Contract,
) -> Result<Vec<String>> {
    let mut matches = Vec::new();

    for entry_point in &contract.entry_points {
        if !is_path_in_allowed_scope(entry_point, &contract.allowed_scope) {
            continue;
        }
        if !entry_point.ends_with(".rs") {
            continue;
        }

        let file_path = repo_root.join(entry_point);
        if !file_path.exists() {
            continue;
        }

        let current = fs::read_to_string(&file_path)
            .with_context(|| format!("read entry point scaffold probe {entry_point}"))?;
        if current == render_rust_entry_point_stub(contract, entry_point) {
            matches.push(entry_point.clone());
        }
    }

    Ok(matches)
}

pub(crate) fn restore_stale_entry_point_masks(repo_root: &Path) -> Result<()> {
    let manifest_path = mask_manifest_path(repo_root);
    if !manifest_path.exists() {
        return Ok(());
    }

    let manifest = read_mask_manifest(&manifest_path)?;
    restore_masked_files(repo_root, &manifest.files)?;
    fs::remove_file(&manifest_path)
        .with_context(|| format!("remove mask manifest {}", manifest_path.display()))?;
    Ok(())
}

pub(crate) struct EntryPointExcerptGuard {
    repo_root: std::path::PathBuf,
    masked_files: Vec<MaskedEntryPointFile>,
    active: bool,
}

impl EntryPointExcerptGuard {
    pub(crate) fn apply(repo_root: &Path, pack: &ContextPack) -> Result<Option<Self>> {
        let mut masked_files = Vec::new();

        for file in &pack.files {
            if !file.truncated_at_test_boundary {
                continue;
            }

            let file_path = repo_root.join(&file.path);
            let source = fs::read_to_string(&file_path)
                .with_context(|| format!("read masked entry point {}", file.path))?;
            let Some((head, tail)) = split_rust_source_at_test_boundary(&source) else {
                continue;
            };

            fs::write(&file_path, head)
                .with_context(|| format!("mask test tail for {}", file.path))?;
            masked_files.push(MaskedEntryPointFile {
                path: file.path.clone(),
                original_tail: tail.to_string(),
            });
        }

        if masked_files.is_empty() {
            return Ok(None);
        }

        write_mask_manifest(repo_root, &masked_files)?;

        Ok(Some(Self {
            repo_root: repo_root.to_path_buf(),
            masked_files,
            active: true,
        }))
    }

    pub(crate) fn restore(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }

        restore_masked_files(&self.repo_root, &self.masked_files)?;
        let manifest_path = mask_manifest_path(&self.repo_root);
        if manifest_path.exists() {
            fs::remove_file(&manifest_path)
                .with_context(|| format!("remove mask manifest {}", manifest_path.display()))?;
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for EntryPointExcerptGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

pub(crate) fn format_context_pack(pack: &ContextPack) -> String {
    let mut sections = Vec::new();

    if !pack.files.is_empty() {
        sections.push("Authoritative bounded context pack:".to_string());
        for file in &pack.files {
            sections.push(format!(
                "- {} (lines {}-{}, truncated_at_test_boundary: {})",
                file.path,
                file.start_line,
                file.end_line,
                if file.truncated_at_test_boundary {
                    "true"
                } else {
                    "false"
                }
            ));
            sections.push("```rust".to_string());
            sections.push(file.content.clone());
            sections.push("```".to_string());
        }
    }

    if !pack.missing_paths.is_empty() {
        sections.push(format!(
            "Missing entry-point files at baseline: {}",
            pack.missing_paths.join(", ")
        ));
    }

    if let Some(seed) = &pack.recipe_seed {
        sections.push("Controller-owned recipe seed:".to_string());
        sections.push(format!("- title: {}", seed.title));
        sections.push(format!("- summary: {}", seed.summary));
        for file in &seed.files {
            sections.push(format!("- file: {} ({})", file.path, file.role));
            for target in &file.edit_targets {
                sections.push(format!("  - {target}"));
            }
        }
    }

    if let Some(seed) = &pack.patch_seed {
        sections.push("Controller-owned patch seed:".to_string());
        sections.push(format!("- title: {}", seed.title));
        sections.push(format!("- summary: {}", seed.summary));
        for file in &seed.files {
            sections.push(format!("- file: {} ({})", file.path, file.purpose));
            sections.push("```rust".to_string());
            sections.push(file.snippet.clone());
            sections.push("```".to_string());
        }
    }

    sections.join("\n")
}

pub(crate) fn format_patch_context_pack(pack: &ContextPack) -> String {
    let mut sections = Vec::new();

    if !pack.files.is_empty() {
        sections.push("Patch lane bounded context:".to_string());
        for file in &pack.files {
            sections.push(format!(
                "- {} (lines {}-{}, truncated_at_test_boundary: {})",
                file.path,
                file.start_line,
                file.end_line,
                if file.truncated_at_test_boundary {
                    "true"
                } else {
                    "false"
                }
            ));
            sections.push("```rust".to_string());
            sections.push(compact_patch_excerpt(&file.content, 80, 4000));
            sections.push("```".to_string());
        }
    }

    if !pack.missing_paths.is_empty() {
        sections.push(format!(
            "Missing entry-point files at baseline: {}",
            pack.missing_paths.join(", ")
        ));
    }

    if let Some(seed) = &pack.patch_seed {
        sections.push("Controller-owned patch seed:".to_string());
        sections.push(format!("- title: {}", seed.title));
        sections.push(format!("- summary: {}", seed.summary));
        for file in &seed.files {
            sections.push(format!("- file: {} ({})", file.path, file.purpose));
            sections.push("```rust".to_string());
            sections.push(compact_patch_excerpt(&file.snippet, 60, 2500));
            sections.push("```".to_string());
        }
    } else if let Some(seed) = &pack.recipe_seed {
        sections.push("Controller-owned recipe seed:".to_string());
        sections.push(format!("- title: {}", seed.title));
        sections.push(format!("- summary: {}", seed.summary));
        for file in &seed.files {
            sections.push(format!(
                "- file: {} ({}) targets: {}",
                file.path,
                file.role,
                file.edit_targets.join("; ")
            ));
        }
    }

    sections.join("\n")
}

fn compact_patch_excerpt(content: &str, max_lines: usize, max_chars: usize) -> String {
    let lines: Vec<&str> = content.lines().take(max_lines).collect();
    let mut compact = lines.join("\n");
    if content.lines().count() > max_lines {
        compact.push_str("\n// ... excerpt truncated for patch lane");
    }
    if compact.len() > max_chars {
        compact.truncate(max_chars.saturating_sub(33));
        compact.push_str("\n// ... excerpt truncated for patch lane");
    }
    compact
}

fn build_recipe_seed(contract: &Contract) -> Option<ContextRecipeSeed> {
    if is_council_synthesis_recipe(contract) {
        return Some(council_synthesis_recipe_seed());
    }
    None
}

fn build_patch_seed(contract: &Contract) -> Option<ContextPatchSeed> {
    if is_council_synthesis_recipe(contract) {
        return Some(council_synthesis_patch_seed());
    }
    None
}

fn is_existing_file_retry_candidate(repo_root: &Path, contract: &Contract) -> bool {
    !contract.entry_points.is_empty()
        && contract
            .entry_points
            .iter()
            .all(|path| repo_root.join(path).exists())
}

fn build_rust_excerpt(path: &str, source: &str) -> ContextFileExcerpt {
    let boundary_line = first_rust_test_boundary_line(source);
    let lines: Vec<&str> = source.lines().collect();
    let end_line = boundary_line
        .map(|line| line.saturating_sub(1))
        .unwrap_or(lines.len());
    let excerpt_lines = if end_line > 0 {
        &lines[..end_line]
    } else {
        &[][..]
    };

    ContextFileExcerpt {
        path: path.to_string(),
        start_line: 1,
        end_line,
        truncated_at_test_boundary: boundary_line.is_some(),
        content: excerpt_lines.join("\n"),
    }
}

fn first_rust_test_boundary_line(source: &str) -> Option<usize> {
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]")
            || trimmed == "mod tests {"
            || trimmed.starts_with("mod tests ")
        {
            return Some(idx + 1);
        }
    }
    None
}

fn split_rust_source_at_test_boundary(source: &str) -> Option<(&str, &str)> {
    rust_test_boundary_offset(source).map(|offset| source.split_at(offset))
}

fn rust_test_boundary_offset(source: &str) -> Option<usize> {
    let mut offset = 0usize;
    for segment in source.split_inclusive('\n') {
        let trimmed = segment.trim_start();
        if trimmed.starts_with("#[cfg(test)]")
            || trimmed == "mod tests {"
            || trimmed.starts_with("mod tests ")
        {
            return Some(offset);
        }
        offset += segment.len();
    }

    if !source.ends_with('\n') {
        let last_line = source[offset..].trim_start();
        if last_line.starts_with("#[cfg(test)]")
            || last_line == "mod tests {"
            || last_line.starts_with("mod tests ")
        {
            return Some(offset);
        }
    }

    None
}

fn is_path_in_allowed_scope(path: &str, allowed_scope: &[String]) -> bool {
    allowed_scope.iter().any(|scope| {
        path == scope
            || path
                .strip_prefix(scope)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn render_rust_entry_point_stub(contract: &Contract, entry_point: &str) -> String {
    if is_council_synthesis_entry_point(entry_point) {
        return council_synthesis_stub(entry_point);
    }

    let mut lines = vec![
        RUST_ENTRY_POINT_STUB_HEADER.to_string(),
        format!("// Path: {entry_point}"),
        "// Replace this scaffold in place. Do not delete or rename this file.".to_string(),
    ];

    if !contract.expected_interfaces.is_empty() {
        lines.push("// Expected interfaces:".to_string());
        lines.extend(
            contract
                .expected_interfaces
                .iter()
                .map(|interface| format!("// - {interface}")),
        );
    }

    if !contract.behavior_requirements.is_empty() {
        lines.push("// Behavior requirements:".to_string());
        lines.extend(
            contract
                .behavior_requirements
                .iter()
                .map(|requirement| format!("// - {requirement}")),
        );
    }

    lines.push(String::new());
    lines.join("\n")
}

fn is_council_synthesis_recipe(contract: &Contract) -> bool {
    let has_required_scope = [
        "crates/punk-council/src/synthesis.rs",
        "crates/punk-council/src/lib.rs",
        "crates/punk-council/src/storage.rs",
    ]
    .iter()
    .all(|path| contract.allowed_scope.iter().any(|allowed| allowed == path));
    if !has_required_scope {
        return false;
    }

    let prompt = contract.prompt_source.to_ascii_lowercase();
    let behaviors = contract
        .behavior_requirements
        .join("\n")
        .to_ascii_lowercase();
    let combined = format!("{prompt}\n{behaviors}");

    [
        "scoreboard",
        "final record",
        "synthesis.json",
        "record.json",
        "leader",
        "hybrid",
        "escalate",
    ]
    .iter()
    .any(|signal| combined.contains(signal))
}

fn is_punk_latest_run_triage_recipe(contract: &Contract) -> bool {
    let expected_scope = [
        "punk/punk-orch/src/run.rs",
        "punk/punk-orch/src/context.rs",
        "punk/punk-run/src/main.rs",
    ];
    let has_scope = expected_scope
        .iter()
        .all(|path| contract.allowed_scope.iter().any(|allowed| allowed == path));
    if !has_scope {
        return false;
    }

    let combined = format!(
        "{}\n{}\n{}",
        contract.prompt_source,
        contract.expected_interfaces.join("\n"),
        contract.behavior_requirements.join("\n")
    )
    .to_ascii_lowercase();

    combined.contains("latest_run_triage")
        && combined.contains("stillalive")
        && combined.contains("runtriage")
}

fn punk_latest_run_triage_patch_seed() -> ContextPatchSeed {
    ContextPatchSeed {
        title: "Punk latest-run triage retry patch seed".to_string(),
        summary: "Apply these snippets directly on the second pass. Start the first meaningful edit in each listed file instead of rereading context.".to_string(),
        files: vec![
            ContextPatchSeedFile {
                path: "punk/punk-orch/src/run.rs".to_string(),
                purpose: "triage types + loader".to_string(),
                snippet: r#"#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriageVerdict {
    NoActiveRun,
    Completed,
    StillAlive,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunTriage {
    pub run_id: String,
    pub status: Option<RunStatus>,
    pub age_s: Option<u64>,
    pub heartbeat_age_s: Option<u64>,
    pub has_receipt: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub verdict: TriageVerdict,
}

pub fn latest_run_triage(bus: &Path, project: &str) -> RunTriage {
    // Add deterministic active/completed/stale triage here using:
    // - bus::read_state(bus, ...)
    // - run json under bus/runs/<task>/
    // - .heartbeats/<task>.hb age
    // - receipt presence under done/failed task dirs
    // - bounded stdout/stderr tails
}"#
                .to_string(),
            },
            ContextPatchSeedFile {
                path: "punk/punk-orch/src/context.rs".to_string(),
                purpose: "context integration".to_string(),
                snippet: r#"// In ContextPack::build(...), after project stats:
if should_include_run_triage(category) {
    let triage = crate::run::latest_run_triage(bus, project);
    if triage.verdict != crate::run::TriageVerdict::NoActiveRun {
        sections.push(format_run_triage(&triage));
    }
}

fn should_include_run_triage(category: &str) -> bool {
    matches!(category.trim().to_ascii_lowercase().as_str(), "fix" | "audit")
        || category.contains("goal")
        || category.contains("plan")
}

fn format_run_triage(triage: &crate::run::RunTriage) -> String {
    // Render a short bounded section with verdict, run id, status,
    // heartbeat age, receipt flag, and bounded tails.
}"#
                .to_string(),
            },
            ContextPatchSeedFile {
                path: "punk/punk-run/src/main.rs".to_string(),
                purpose: "goal/planning guard".to_string(),
                snippet: r#"fn latest_run_guard_message(bus: &Path, project: &str, flow: &str) -> Option<String> {
    let triage = punk_orch::run::latest_run_triage(bus, project);
    if triage.verdict != punk_orch::run::TriageVerdict::StillAlive {
        return None;
    }
    Some(format!(
        "Blocked {flow}: latest run for project '{}' is still alive (run={}). Recheck the active run before starting another follow-up.",
        project, triage.run_id
    ))
}

// Call this guard near the start of cmd_goal(...) and cmd_approve(...),
// before planner/approval continues.
"#
                .to_string(),
            },
        ],
    }
}

fn generic_existing_file_patch_seed(
    contract: &Contract,
    files: &[ContextFileExcerpt],
) -> Option<ContextPatchSeed> {
    if files.is_empty() {
        return None;
    }

    let patch_files: Vec<_> = files
        .iter()
        .take(3)
        .enumerate()
        .map(|(idx, file)| ContextPatchSeedFile {
            path: file.path.clone(),
            purpose: "edit target".to_string(),
            snippet: generic_patch_seed_snippet(contract, file, idx + 1, files.len()),
        })
        .collect();

    Some(ContextPatchSeed {
        title: "Existing-file bounded retry patch seed".to_string(),
        summary: "Start editing these files directly on the second pass. Follow the ordered file targets, begin at the highest-confidence symbol anchors below, and make the first in-place diff before any further orientation.".to_string(),
        files: patch_files,
    })
}

fn generic_patch_seed_snippet(
    contract: &Contract,
    file: &ContextFileExcerpt,
    position: usize,
    total_files: usize,
) -> String {
    let contract_targets = contract_target_lines(contract);
    let symbol_targets = highest_confidence_symbols(contract, file);
    let anchors_text = if symbol_targets.is_empty() {
        "No high-confidence symbol anchors detected; edit near the first top-level declarations already shown in the excerpt."
            .to_string()
    } else {
        format!(
            "Highest-confidence symbols already present in this file:\n{}",
            symbol_targets
                .iter()
                .take(3)
                .map(|anchor| format!(
                    "// - line {}: {} (score={})",
                    anchor.line_number, anchor.signature, anchor.score
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let anchor_excerpt_text = if symbol_targets.is_empty() {
        String::new()
    } else {
        format!(
            "\nAnchor excerpts to edit in place:\n{}",
            symbol_targets
                .iter()
                .take(2)
                .map(|anchor| render_anchor_excerpt(file, anchor))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let requirements = contract
        .behavior_requirements
        .iter()
        .take(3)
        .map(|item| format!("// - {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    let interfaces = contract
        .expected_interfaces
        .iter()
        .take(3)
        .map(|item| format!("// - {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    let targets = contract_targets
        .iter()
        .map(|item| format!("// - {item}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "// Retry patch seed for {path}\n// Ordered file target: {position} of {total_files}\n// First action: open this file in place and edit the first matching anchor below before any more orientation.\n// Contract-derived symbol or behavior targets:\n{targets}\n// Required interfaces for this slice:\n{interfaces}\n// Behavior requirements to satisfy here first:\n{requirements}\n{anchors_text}{anchor_excerpt_text}\n// Make the first minimal in-place edit in this file now. Do not reread the same bounded context before editing.",
        path = file.path,
        position = position,
        total_files = total_files,
        targets = targets,
        interfaces = interfaces,
        requirements = requirements,
        anchors_text = anchors_text,
        anchor_excerpt_text = anchor_excerpt_text,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolAnchor {
    line_number: usize,
    signature: String,
    score: usize,
}

fn contract_target_lines(contract: &Contract) -> Vec<String> {
    let mut targets = Vec::new();
    for item in contract
        .expected_interfaces
        .iter()
        .chain(contract.behavior_requirements.iter())
    {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !targets.iter().any(|existing| existing == trimmed) {
            targets.push(trimmed.to_string());
        }
    }
    if targets.is_empty() {
        let prompt = contract.prompt_source.trim();
        if !prompt.is_empty() {
            targets.push(prompt.to_string());
        }
    }
    targets.into_iter().take(4).collect()
}

fn highest_confidence_symbols(contract: &Contract, file: &ContextFileExcerpt) -> Vec<SymbolAnchor> {
    let contract_terms = contract_terms(contract);
    let mut anchors: Vec<_> = file
        .content
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let signature = line.trim();
            if !looks_like_symbol_anchor(signature) {
                return None;
            }
            let score = anchor_score(signature, &contract_terms, &file.path);
            Some(SymbolAnchor {
                line_number: idx + file.start_line,
                signature: signature.to_string(),
                score,
            })
        })
        .collect();

    anchors.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.line_number.cmp(&right.line_number))
    });
    anchors
}

fn contract_terms(contract: &Contract) -> Vec<String> {
    let combined = format!(
        "{}\n{}\n{}\n{}",
        contract.prompt_source,
        contract.expected_interfaces.join("\n"),
        contract.behavior_requirements.join("\n"),
        contract.entry_points.join("\n")
    );
    combined
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            if token.len() >= 3 {
                Some(token)
            } else {
                None
            }
        })
        .collect()
}

fn looks_like_symbol_anchor(line: &str) -> bool {
    line.starts_with("pub struct ")
        || line.starts_with("struct ")
        || line.starts_with("pub enum ")
        || line.starts_with("enum ")
        || line.starts_with("pub fn ")
        || line.starts_with("fn ")
        || line.starts_with("impl ")
        || line.starts_with("pub mod ")
        || line.starts_with("mod ")
        || line.starts_with("pub use ")
}

fn anchor_score(signature: &str, contract_terms: &[String], path: &str) -> usize {
    let signature_lc = signature.to_ascii_lowercase();
    let mut score = 0usize;
    for term in contract_terms {
        if signature_lc.contains(term) {
            score += 2;
        }
        if path.to_ascii_lowercase().contains(term) {
            score += 1;
        }
    }
    if signature.starts_with("pub fn ") || signature.starts_with("fn ") {
        score += 1;
    }
    if signature.starts_with("impl ") {
        score += 1;
    }
    score
}

fn render_anchor_excerpt(file: &ContextFileExcerpt, anchor: &SymbolAnchor) -> String {
    let lines: Vec<&str> = file.content.lines().collect();
    let local_index = anchor.line_number.saturating_sub(file.start_line);
    let start = local_index.saturating_sub(1);
    let end = std::cmp::min(lines.len(), local_index + 2);
    let excerpt = lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| format!("// {:>4} | {}", start + offset + file.start_line, line))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{excerpt}")
}

fn council_synthesis_recipe_seed() -> ContextRecipeSeed {
    ContextRecipeSeed {
        title: "Council synthesis + final record multi-file seed".to_string(),
        summary: "Edit all three allowed files together. Implement synthesis logic in synthesis.rs, add exact wiring/export and service completion method in lib.rs, and add persistence helpers for synthesis.json plus final record.json in storage.rs. Do not stop at the synthesis scaffold alone.".to_string(),
        files: vec![
            ContextRecipeFileSeed {
                path: "crates/punk-council/src/synthesis.rs".to_string(),
                role: "implementation".to_string(),
                edit_targets: vec![
                    "Keep `synthesize_from_scoreboard(council_id, scoreboard) -> Result<CouncilSynthesis>` and replace scaffold TODO strings with scoreboard-derived rationale and unresolved-risk text.".to_string(),
                    "Use `CouncilOutcome::{Leader, Hybrid, Escalate}` only, derive `selected_labels` from top_label/second_label, and keep confidence in [0.0, 1.0].".to_string(),
                    "Keep `must_keep` and `must_fix` typed as `Vec<String>`; conservative empty vectors are acceptable in this slice if no richer source exists.".to_string(),
                ],
            },
            ContextRecipeFileSeed {
                path: "crates/punk-council/src/lib.rs".to_string(),
                role: "wiring".to_string(),
                edit_targets: vec![
                    "Add `pub mod synthesis;` near the other module declarations.".to_string(),
                    "Re-export the synthesis entry point with `pub use synthesis::synthesize_from_scoreboard;` near the scoring export.".to_string(),
                    "Extend `use storage::{...}` with the new synthesis/final-record persistence helpers.".to_string(),
                    "Add a `CouncilService` method that completes the advisory run from `packet + proposal_refs + review_refs + scoreboard`, persists synthesis and record artifacts, and returns `CouncilRecord`.".to_string(),
                ],
            },
            ContextRecipeFileSeed {
                path: "crates/punk-council/src/storage.rs".to_string(),
                role: "persistence".to_string(),
                edit_targets: vec![
                    "Add `persist_synthesis(repo_root, paths, synthesis) -> Result<String>` that writes `synthesis.json` and returns a repo-relative ref.".to_string(),
                    "Replace the placeholder-only final record path with a helper that accepts `proposal_refs`, `review_refs`, and `synthesis_ref` and writes a fully populated `CouncilRecord`.".to_string(),
                    "Keep `packet_ref` and `scoreboard_ref` repo-relative and preserve the existing `.punk/council/<id>/...` artifact layout.".to_string(),
                ],
            },
        ],
    }
}

fn council_synthesis_patch_seed() -> ContextPatchSeed {
    ContextPatchSeed {
        title: "Council synthesis bounded patch seed".to_string(),
        summary: "Apply these snippets in place across the allowed files, then run the required checks. Prefer adapting this seed over inventing new wiring.".to_string(),
        files: vec![
            ContextPatchSeedFile {
                path: "crates/punk-council/src/synthesis.rs".to_string(),
                purpose: "implementation".to_string(),
                snippet: r#"use anyhow::Result;
use punk_domain::council::{CouncilOutcome, CouncilScoreboard, CouncilSynthesis};

pub fn synthesize_from_scoreboard(
    council_id: &str,
    scoreboard: &CouncilScoreboard,
) -> Result<CouncilSynthesis> {
    let outcome = choose_outcome(scoreboard);
    let selected_labels = choose_selected_labels(scoreboard, &outcome);
    let rationale = format!(
        "Top proposal {:?} selected with gap {:?} and disagreement={}",
        scoreboard.top_label,
        scoreboard.top_gap,
        scoreboard.high_disagreement
    );
    let unresolved_risks = if scoreboard.high_disagreement {
        vec!["high review disagreement requires human follow-up".to_string()]
    } else {
        Vec::new()
    };

    Ok(CouncilSynthesis {
        council_id: council_id.to_string(),
        outcome: outcome.clone(),
        selected_labels,
        rationale,
        must_keep: Vec::new(),
        must_fix: Vec::new(),
        unresolved_risks,
        confidence: choose_confidence(scoreboard, &outcome),
    })
}"#
                    .to_string(),
            },
            ContextPatchSeedFile {
                path: "crates/punk-council/src/lib.rs".to_string(),
                purpose: "wiring".to_string(),
                snippet: r#"pub mod synthesis;

use punk_domain::council::{CouncilKind, CouncilPacket, CouncilProposal, CouncilRecord, CouncilScoreboard};
use storage::{persist_packet, persist_record, persist_synthesis, CouncilPaths};

pub use scoring::score_reviews;
pub use synthesis::synthesize_from_scoreboard;

impl CouncilService {
    pub fn complete_synthesis(
        &self,
        packet: &CouncilPacket,
        proposal_refs: &[String],
        review_refs: &[String],
        scoreboard: &CouncilScoreboard,
    ) -> Result<CouncilRecord> {
        let paths = CouncilPaths::new(&self.repo_root, &packet.id);
        let synthesis = synthesize_from_scoreboard(&packet.id, scoreboard)?;
        let synthesis_ref = persist_synthesis(&self.repo_root, &paths, &synthesis)?;
        let record = persist_record(
            &self.repo_root,
            &paths,
            packet,
            proposal_refs,
            review_refs,
            synthesis_ref,
        )?;
        events::emit_completed(&self.events, &self.repo_root, packet, &record, &paths.record_path)?;
        Ok(record)
    }
}"#
                    .to_string(),
            },
            ContextPatchSeedFile {
                path: "crates/punk-council/src/storage.rs".to_string(),
                purpose: "persistence".to_string(),
                snippet: r#"use punk_domain::council::{CouncilPacket, CouncilProposal, CouncilRecord, CouncilSynthesis};

pub fn persist_synthesis(
    repo_root: &Path,
    paths: &CouncilPaths,
    synthesis: &CouncilSynthesis,
) -> Result<String> {
    write_json(&paths.synthesis_path, synthesis)?;
    relative_ref(repo_root, &paths.synthesis_path)
}

pub fn persist_record(
    repo_root: &Path,
    paths: &CouncilPaths,
    packet: &CouncilPacket,
    proposal_refs: &[String],
    review_refs: &[String],
    synthesis_ref: String,
) -> Result<CouncilRecord> {
    let record = CouncilRecord {
        id: packet.id.clone(),
        packet_ref: relative_ref(repo_root, &paths.packet_path)?,
        proposal_refs: proposal_refs.to_vec(),
        review_refs: review_refs.to_vec(),
        synthesis_ref,
        scoreboard_ref: relative_ref(repo_root, &paths.scoreboard_path)?,
        completed_at: punk_domain::now_rfc3339(),
    };
    write_json(&paths.record_path, &record)?;
    Ok(record)
}"#
                    .to_string(),
            },
        ],
    }
}

fn is_council_synthesis_entry_point(entry_point: &str) -> bool {
    entry_point == "crates/punk-council/src/synthesis.rs"
}

fn council_synthesis_stub(entry_point: &str) -> String {
    format!(
        r#"use anyhow::Result;
use punk_domain::council::{{CouncilOutcome, CouncilScoreboard, CouncilSynthesis}};

{header}
// Path: {entry_point}
// Replace this scaffold in place. Do not delete or rename this file.

pub fn synthesize_from_scoreboard(
    council_id: &str,
    scoreboard: &CouncilScoreboard,
) -> Result<CouncilSynthesis> {{
    let outcome = choose_outcome(scoreboard);
    let selected_labels = choose_selected_labels(scoreboard, &outcome);
    let confidence = choose_confidence(scoreboard, &outcome);

    Ok(CouncilSynthesis {{
        council_id: council_id.to_string(),
        outcome,
        selected_labels,
        rationale: "TODO: replace synthesis scaffold rationale with scoreboard-derived explanation".into(),
        must_keep: Vec::new(),
        must_fix: Vec::new(),
        unresolved_risks: vec![
            "TODO: replace synthesis scaffold unresolved risks from review/proposal artifacts".into(),
        ],
        confidence,
    }})
}}

fn choose_outcome(scoreboard: &CouncilScoreboard) -> CouncilOutcome {{
    if scoreboard.top_label.is_none() {{
        CouncilOutcome::Escalate
    }} else if scoreboard.high_disagreement {{
        CouncilOutcome::Escalate
    }} else if scoreboard.top_gap.unwrap_or_default() >= 1.0 {{
        CouncilOutcome::Leader
    }} else if scoreboard.second_label.is_some() {{
        CouncilOutcome::Hybrid
    }} else {{
        CouncilOutcome::Leader
    }}
}}

fn choose_selected_labels(
    scoreboard: &CouncilScoreboard,
    outcome: &CouncilOutcome,
) -> Vec<String> {{
    match outcome {{
        CouncilOutcome::Leader => scoreboard.top_label.iter().cloned().collect(),
        CouncilOutcome::Hybrid => scoreboard
            .top_label
            .iter()
            .chain(scoreboard.second_label.iter())
            .cloned()
            .collect(),
        CouncilOutcome::Escalate => scoreboard.top_label.iter().cloned().collect(),
    }}
}}

fn choose_confidence(scoreboard: &CouncilScoreboard, outcome: &CouncilOutcome) -> f32 {{
    let base = match outcome {{
        CouncilOutcome::Leader => 0.75,
        CouncilOutcome::Hybrid => 0.6,
        CouncilOutcome::Escalate => 0.35,
    }};
    let gap_bonus = scoreboard.top_gap.unwrap_or_default().clamp(0.0, 1.0) * 0.15;
    let disagreement_penalty = if scoreboard.high_disagreement {{ 0.2 }} else {{ 0.0 }};
    (base + gap_bonus - disagreement_penalty).clamp(0.0, 1.0)
}}
"#,
        header = RUST_ENTRY_POINT_STUB_HEADER,
        entry_point = entry_point,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MaskedEntryPointFile {
    path: String,
    original_tail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EntryPointMaskManifest {
    files: Vec<MaskedEntryPointFile>,
}

fn mask_manifest_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(ENTRY_POINT_MASK_MANIFEST)
}

fn read_mask_manifest(path: &Path) -> Result<EntryPointMaskManifest> {
    let payload = fs::read_to_string(path)
        .with_context(|| format!("read mask manifest {}", path.display()))?;
    serde_json::from_str(&payload)
        .with_context(|| format!("parse mask manifest {}", path.display()))
}

fn write_mask_manifest(repo_root: &Path, files: &[MaskedEntryPointFile]) -> Result<()> {
    let manifest_path = mask_manifest_path(repo_root);
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create mask manifest dir {}", parent.display()))?;
    }
    let payload = serde_json::to_string_pretty(&EntryPointMaskManifest {
        files: files.to_vec(),
    })?;
    fs::write(&manifest_path, payload)
        .with_context(|| format!("write mask manifest {}", manifest_path.display()))
}

fn restore_masked_files(repo_root: &Path, files: &[MaskedEntryPointFile]) -> Result<()> {
    for file in files {
        let file_path = repo_root.join(&file.path);
        let current_head = fs::read_to_string(&file_path)
            .with_context(|| format!("read masked entry point {}", file.path))?;
        if current_head.ends_with(&file.original_tail) {
            continue;
        }
        fs::write(&file_path, format!("{current_head}{}", file.original_tail))
            .with_context(|| format!("restore masked entry point {}", file.path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_excerpt_truncates_at_cfg_test_boundary() {
        let source = r#"pub fn score() {}

#[cfg(test)]
mod tests {
    #[test]
    fn works() {}
}"#;
        let excerpt = build_rust_excerpt("src/scoring.rs", source);
        assert_eq!(excerpt.start_line, 1);
        assert_eq!(excerpt.end_line, 2);
        assert!(excerpt.truncated_at_test_boundary);
        assert!(excerpt.content.contains("pub fn score() {}"));
        assert!(!excerpt.content.contains("mod tests"));
    }

    #[test]
    fn rust_excerpt_keeps_full_file_without_test_boundary() {
        let source = "pub fn score() {}\npub fn normalize() {}";
        let excerpt = build_rust_excerpt("src/scoring.rs", source);
        assert_eq!(excerpt.end_line, 2);
        assert!(!excerpt.truncated_at_test_boundary);
        assert!(excerpt.content.contains("normalize"));
    }

    #[test]
    fn formatter_includes_missing_entry_points() {
        let pack = ContextPack {
            files: vec![],
            missing_paths: vec!["src/new_file.rs".into()],
            recipe_seed: None,
            patch_seed: None,
        };
        let rendered = format_context_pack(&pack);
        assert!(rendered.contains("Missing entry-point files at baseline: src/new_file.rs"));
    }

    #[test]
    fn patch_formatter_compacts_large_excerpts() {
        let large = (0..120)
            .map(|idx| format!("pub fn line_{idx}() {{}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pack = ContextPack {
            files: vec![ContextFileExcerpt {
                path: "src/lib.rs".into(),
                start_line: 1,
                end_line: 120,
                truncated_at_test_boundary: false,
                content: large,
            }],
            missing_paths: vec![],
            recipe_seed: None,
            patch_seed: None,
        };
        let rendered = format_patch_context_pack(&pack);
        assert!(rendered.contains("Patch lane bounded context:"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("excerpt truncated for patch lane"));
    }

    #[test]
    fn build_context_pack_adds_council_synthesis_recipe_seed() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-recipe-seed-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub fn keep() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_seed".into(),
            feature_id: "feat_seed".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["CouncilSynthesis".into()],
            behavior_requirements: vec!["persist synthesis.json and final record.json".into()],
            allowed_scope: vec![
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let pack = build_context_pack(&root, &contract).unwrap();
        let seed = pack.recipe_seed.expect("expected synthesis recipe seed");
        assert!(seed
            .summary
            .contains("Edit all three allowed files together"));
        assert_eq!(seed.files.len(), 3);
        assert_eq!(seed.files[0].path, "crates/punk-council/src/synthesis.rs");
        assert!(seed.files[1]
            .edit_targets
            .iter()
            .any(|target| target.contains("pub mod synthesis")));
        assert!(seed.files[2]
            .edit_targets
            .iter()
            .any(|target| target.contains("persist_synthesis")));

        let rendered = format_context_pack(&ContextPack {
            files: pack.files,
            missing_paths: pack.missing_paths,
            recipe_seed: Some(seed),
            patch_seed: None,
        });
        assert!(rendered.contains("Controller-owned recipe seed:"));
        assert!(rendered.contains("crates/punk-council/src/lib.rs (wiring)"));
        assert!(rendered.contains("crates/punk-council/src/storage.rs (persistence)"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_context_pack_adds_council_synthesis_patch_seed() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-patch-seed-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub fn keep() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/storage.rs"),
            "pub fn persist() {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_patch".into(),
            feature_id: "feat_patch".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec![
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["CouncilSynthesis".into()],
            behavior_requirements: vec!["persist synthesis.json and final record.json".into()],
            allowed_scope: vec![
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
                "crates/punk-council/src/storage.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let pack = build_context_pack(&root, &contract).unwrap();
        let seed = pack.patch_seed.expect("expected synthesis patch seed");
        assert_eq!(seed.files.len(), 3);
        assert!(seed.files[0]
            .snippet
            .contains("pub fn synthesize_from_scoreboard"));
        assert!(seed.files[1].snippet.contains("pub mod synthesis;"));
        assert!(seed.files[2].snippet.contains("pub fn persist_synthesis"));

        let rendered = format_context_pack(&ContextPack {
            files: pack.files,
            missing_paths: pack.missing_paths,
            recipe_seed: pack.recipe_seed,
            patch_seed: Some(seed),
        });
        assert!(rendered.contains("Controller-owned patch seed:"));
        assert!(rendered.contains("Apply these snippets in place"));
        assert!(rendered.contains("pub fn persist_synthesis"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn retry_patch_seed_adds_latest_run_triage_seed_for_punk_slice() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-run-triage-seed-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("punk/punk-orch/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-run/src")).unwrap();
        fs::write(
            root.join("punk/punk-orch/src/run.rs"),
            "pub struct Run {}\npub enum RunStatus { Running }\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-orch/src/context.rs"),
            "pub struct ContextPack {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-run/src/main.rs"),
            "fn cmd_goal() {}\nfn cmd_approve() {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_stage3".into(),
            feature_id: "feat_stage3".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Add latest_run_triage and StillAlive guard for Stage 3 goal planning"
                .into(),
            entry_points: vec![
                "punk/punk-orch/src/run.rs".into(),
                "punk/punk-orch/src/context.rs".into(),
                "punk/punk-run/src/main.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec![
                "latest_run_triage API in run.rs".into(),
                "RunTriage and TriageVerdict".into(),
            ],
            behavior_requirements: vec![
                "warn or guard on StillAlive".into(),
                "inject latest-run triage section".into(),
            ],
            allowed_scope: vec![
                "punk/punk-orch/src/run.rs".into(),
                "punk/punk-orch/src/context.rs".into(),
                "punk/punk-run/src/main.rs".into(),
            ],
            target_checks: vec!["cd punk && cargo test -p punk-orch".into()],
            integrity_checks: vec!["cd punk && cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let mut pack = build_context_pack(&root, &contract).unwrap();
        assert!(pack.patch_seed.is_none());
        ensure_retry_patch_seed(&root, &contract, &mut pack);

        let seed = pack.patch_seed.expect("expected retry patch seed");
        assert_eq!(seed.files.len(), 3);
        assert!(seed.files[0].snippet.contains("latest_run_triage"));
        assert!(seed.files[1].snippet.contains("should_include_run_triage"));
        assert!(seed.files[2].snippet.contains("cmd_goal"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn retry_patch_seed_falls_back_to_generic_existing_file_seed() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-generic-retry-seed-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "use std::path::Path;\npub fn existing() {}\nimpl Thing {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_generic".into(),
            feature_id: "feat_generic".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "Add bounded retry patch seed".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["latest run helper".into()],
            behavior_requirements: vec!["start the first meaningful edit".into()],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let mut pack = build_context_pack(&root, &contract).unwrap();
        assert!(pack.patch_seed.is_none());
        ensure_retry_patch_seed(&root, &contract, &mut pack);

        let seed = pack.patch_seed.expect("expected generic retry patch seed");
        assert_eq!(seed.files.len(), 1);
        assert!(seed.files[0]
            .snippet
            .contains("Retry patch seed for src/lib.rs"));
        assert!(seed.files[0]
            .snippet
            .contains("Ordered file target: 1 of 1"));
        assert!(seed.files[0].snippet.contains("Highest-confidence symbols"));
        assert!(seed.files[0]
            .snippet
            .contains("Anchor excerpts to edit in place"));
        assert!(seed.files[0].snippet.contains("line 2: pub fn existing()"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn phase_event_slice_does_not_pick_up_synthesis_seeds() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-phase-events-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("crates/punk-council/src/events.rs"),
            "pub fn emit() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/proposal.rs"),
            "pub fn proposal() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/review.rs"),
            "pub fn review() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/synthesis.rs"),
            "pub fn synthesis() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub fn keep() {}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_events".into(),
            feature_id: "feat_events".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "add repo-local advisory phase events".into(),
            entry_points: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["phase events".into()],
            behavior_requirements: vec![
                "Emit council.proposal_written when a proposal artifact is persisted.".into(),
                "Emit council.review_written when a review artifact is persisted.".into(),
                "Emit council.synthesis_written when synthesis.json is written.".into(),
                "Keep council.completed unchanged.".into(),
            ],
            allowed_scope: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let pack = build_context_pack(&root, &contract).unwrap();
        assert!(pack.recipe_seed.is_none());
        assert!(pack.patch_seed.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn build_context_pack_includes_allowed_scope_support_file_excerpts() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-support-file-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();
        fs::write(
            root.join("crates/punk-council/src/events.rs"),
            "pub fn emit() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/proposal.rs"),
            "pub fn proposal() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/review.rs"),
            "pub fn review() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/synthesis.rs"),
            "pub fn synthesis() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/punk-council/src/lib.rs"),
            "pub fn keep() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn works() {}\n}\n",
        )
        .unwrap();

        let contract = Contract {
            id: "ct_events".into(),
            feature_id: "feat_events".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "add repo-local advisory phase events".into(),
            entry_points: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
            ],
            import_paths: vec![],
            expected_interfaces: vec!["phase events".into()],
            behavior_requirements: vec![
                "Emit council.proposal_written when a proposal artifact is persisted.".into(),
            ],
            allowed_scope: vec![
                "crates/punk-council/src/events.rs".into(),
                "crates/punk-council/src/proposal.rs".into(),
                "crates/punk-council/src/review.rs".into(),
                "crates/punk-council/src/synthesis.rs".into(),
                "crates/punk-council/src/lib.rs".into(),
            ],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "low".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let pack = build_context_pack(&root, &contract).unwrap();
        let lib_excerpt = pack
            .files
            .iter()
            .find(|file| file.path == "crates/punk-council/src/lib.rs")
            .expect("expected lib.rs support excerpt");
        assert!(lib_excerpt.truncated_at_test_boundary);
        assert!(lib_excerpt.content.contains("pub fn keep() {}"));
        assert!(!lib_excerpt.content.contains("mod tests"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_missing_rust_entry_points_writes_stub_and_context() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-materialize-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/new_file.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["typed scaffold".into()],
            behavior_requirements: vec!["persist bounded artifact".into()],
            allowed_scope: vec!["src/new_file.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let created = materialize_missing_entry_points(&root, &contract).unwrap();
        assert_eq!(created, vec!["src/new_file.rs".to_string()]);
        assert!(root.join("src/new_file.rs").is_file());
        assert_eq!(
            fs::read_to_string(root.join("src/new_file.rs")).unwrap(),
            render_rust_entry_point_stub(&contract, "src/new_file.rs")
        );

        let pack = build_context_pack(&root, &contract).unwrap();
        assert!(pack.missing_paths.is_empty());
        assert_eq!(pack.files.len(), 1);
        assert_eq!(pack.files[0].path, "src/new_file.rs");
        assert!(pack.files[0]
            .content
            .contains("Approved new entry-point file for this bounded contract."));
        assert!(pack.files[0].content.contains("Expected interfaces:"));
        assert!(pack.files[0].content.contains("Behavior requirements:"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn materialize_missing_council_synthesis_uses_compilable_recipe_scaffold() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-synthesis-scaffold-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();

        let contract = Contract {
            id: "ct_synth".into(),
            feature_id: "feat_synth".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["crates/punk-council/src/synthesis.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["CouncilSynthesis helper".into()],
            behavior_requirements: vec!["persist synthesis".into()],
            allowed_scope: vec!["crates/punk-council/src/synthesis.rs".into()],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let created = materialize_missing_entry_points(&root, &contract).unwrap();
        assert_eq!(
            created,
            vec!["crates/punk-council/src/synthesis.rs".to_string()]
        );

        let scaffold =
            fs::read_to_string(root.join("crates/punk-council/src/synthesis.rs")).unwrap();
        assert!(scaffold.contains("use anyhow::Result;"));
        assert!(scaffold.contains(
            "use punk_domain::council::{CouncilOutcome, CouncilScoreboard, CouncilSynthesis};"
        ));
        assert!(scaffold.contains("pub fn synthesize_from_scoreboard("));
        assert!(scaffold.contains("fn choose_outcome("));
        assert!(scaffold.contains("fn choose_selected_labels("));
        assert!(scaffold.contains("fn choose_confidence("));
        assert!(scaffold.contains("TODO: replace synthesis scaffold rationale"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scaffold_only_entry_points_detects_unmodified_recipe_scaffold() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-scaffold-detect-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/punk-council/src")).unwrap();

        let contract = Contract {
            id: "ct_synth".into(),
            feature_id: "feat_synth".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["crates/punk-council/src/synthesis.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["CouncilSynthesis helper".into()],
            behavior_requirements: vec!["persist synthesis".into()],
            allowed_scope: vec!["crates/punk-council/src/synthesis.rs".into()],
            target_checks: vec!["cargo test -p punk-council".into()],
            integrity_checks: vec!["cargo test --workspace".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        fs::write(
            root.join("crates/punk-council/src/synthesis.rs"),
            render_rust_entry_point_stub(&contract, "crates/punk-council/src/synthesis.rs"),
        )
        .unwrap();

        assert_eq!(
            scaffold_only_entry_points(&root, &contract).unwrap(),
            vec!["crates/punk-council/src/synthesis.rs".to_string()]
        );

        fs::write(
            root.join("crates/punk-council/src/synthesis.rs"),
            "pub fn real_impl() {}\n",
        )
        .unwrap();
        assert!(scaffold_only_entry_points(&root, &contract)
            .unwrap()
            .is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn entry_point_excerpt_guard_restores_test_tail_after_head_edit() {
        let root =
            std::env::temp_dir().join(format!("punk-context-pack-mask-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        let file_path = root.join("src/lib.rs");
        let source =
            "pub fn score() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn works() {}\n}\n";
        fs::write(&file_path, source).unwrap();

        let contract = Contract {
            id: "ct_1".into(),
            feature_id: "feat_1".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/lib.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec![],
            behavior_requirements: vec![],
            allowed_scope: vec!["src/lib.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let pack = build_context_pack(&root, &contract).unwrap();
        let mut guard = EntryPointExcerptGuard::apply(&root, &pack)
            .unwrap()
            .expect("expected truncated excerpt guard");

        assert_eq!(
            fs::read_to_string(&file_path).unwrap(),
            "pub fn score() {}\n\n"
        );

        fs::write(&file_path, "pub fn score() -> u32 { 1 }\n\n").unwrap();
        guard.restore().unwrap();

        assert_eq!(
            fs::read_to_string(&file_path).unwrap(),
            "pub fn score() -> u32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn works() {}\n}\n"
        );
        assert!(!mask_manifest_path(&root).exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_stale_entry_point_masks_rehydrates_tail_from_manifest() {
        let root =
            std::env::temp_dir().join(format!("punk-context-pack-restore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        let file_path = root.join("src/lib.rs");
        fs::write(&file_path, "pub fn score() {}\n").unwrap();

        write_mask_manifest(
            &root,
            &[MaskedEntryPointFile {
                path: "src/lib.rs".into(),
                original_tail: "\n#[cfg(test)]\nmod tests {}\n".into(),
            }],
        )
        .unwrap();

        restore_stale_entry_point_masks(&root).unwrap();

        assert_eq!(
            fs::read_to_string(&file_path).unwrap(),
            "pub fn score() {}\n\n#[cfg(test)]\nmod tests {}\n"
        );
        assert!(!mask_manifest_path(&root).exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_missing_materialized_entry_points_recreates_deleted_stub() {
        let root = std::env::temp_dir().join(format!(
            "punk-context-pack-restore-materialized-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);

        let contract = Contract {
            id: "ct_restore".into(),
            feature_id: "feat_restore".into(),
            version: 1,
            status: punk_domain::ContractStatus::Approved,
            prompt_source: "x".into(),
            entry_points: vec!["src/synthesis.rs".into()],
            import_paths: vec![],
            expected_interfaces: vec!["typed scaffold".into()],
            behavior_requirements: vec!["persist bounded artifact".into()],
            allowed_scope: vec!["src/synthesis.rs".into()],
            target_checks: vec!["cargo test".into()],
            integrity_checks: vec!["cargo test".into()],
            risk_level: "medium".into(),
            created_at: "now".into(),
            approved_at: Some("now".into()),
        };

        let restored = restore_missing_materialized_entry_points(
            &root,
            &contract,
            &[String::from("src/synthesis.rs")],
        )
        .unwrap();

        assert_eq!(restored, vec!["src/synthesis.rs".to_string()]);
        assert_eq!(
            fs::read_to_string(root.join("src/synthesis.rs")).unwrap(),
            render_rust_entry_point_stub(&contract, "src/synthesis.rs")
        );

        let _ = fs::remove_dir_all(&root);
    }
}
