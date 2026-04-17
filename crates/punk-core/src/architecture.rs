use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use punk_domain::{
    now_rfc3339, ArchitectureOversizedFile, ArchitectureSeverity, ArchitectureSignals,
    ArchitectureThresholds,
};

pub const DEFAULT_ARCHITECTURE_THRESHOLDS: ArchitectureThresholds = ArchitectureThresholds {
    warn_file_loc: 600,
    critical_file_loc: 1200,
    critical_scope_roots: 1,
    warn_expected_interfaces: 2,
    warn_import_paths: 5,
};

const IGNORED_DIR_NAMES: &[&str] = &[
    ".git",
    ".jj",
    ".punk",
    ".playwright-mcp",
    ".build",
    "node_modules",
    "target",
];

pub struct ArchitectureSignalInput<'a> {
    pub contract_id: &'a str,
    pub feature_id: &'a str,
    pub prompt_source: &'a str,
    pub allowed_scope: &'a [String],
    pub entry_points: &'a [String],
    pub import_paths: &'a [String],
    pub expected_interfaces: &'a [String],
    pub behavior_requirements: &'a [String],
}

pub fn compute_architecture_signals(
    repo_root: &Path,
    input: ArchitectureSignalInput<'_>,
) -> Result<ArchitectureSignals> {
    let thresholds = DEFAULT_ARCHITECTURE_THRESHOLDS.clone();
    let scope_paths =
        combined_scope_paths(input.allowed_scope, input.entry_points, input.import_paths);
    let scope_roots = scope_roots(&scope_paths);
    let distinct_scope_roots = scope_roots.len();
    let scoped_files = collect_scope_files(repo_root, &scope_paths)?;

    let mut oversized_files = scoped_files
        .into_iter()
        .filter_map(|path| {
            let loc = line_count_for_path(repo_root, &path).ok()?;
            (loc >= thresholds.warn_file_loc).then_some(ArchitectureOversizedFile { path, loc })
        })
        .collect::<Vec<_>>();
    oversized_files.sort_by(|a, b| a.path.cmp(&b.path));

    let entry_point_count = distinct_count(input.entry_points);
    let expected_interface_count = distinct_count(input.expected_interfaces);
    let import_path_count = distinct_count(input.import_paths);
    let has_cleanup_obligations = has_obligation_keywords(
        input.prompt_source,
        input.behavior_requirements,
        CLEANUP_KEYWORDS,
    );
    let has_docs_obligations = has_docs_obligations(
        input.allowed_scope,
        input.entry_points,
        input.behavior_requirements,
    ) || text_contains_any(input.prompt_source, DOC_KEYWORDS);
    let has_migration_sensitive_surfaces = has_obligation_keywords(
        input.prompt_source,
        input.behavior_requirements,
        MIGRATION_KEYWORDS,
    );

    let mut severity = ArchitectureSeverity::None;
    let mut trigger_reasons = Vec::new();

    for file in &oversized_files {
        if file.loc >= thresholds.critical_file_loc {
            severity = max_severity(severity, ArchitectureSeverity::Critical);
            trigger_reasons.push(format!(
                "oversized file {} has {} LOC (>= critical threshold {})",
                file.path, file.loc, thresholds.critical_file_loc
            ));
        } else {
            severity = max_severity(severity, ArchitectureSeverity::Warn);
            trigger_reasons.push(format!(
                "oversized file {} has {} LOC (>= warn threshold {})",
                file.path, file.loc, thresholds.warn_file_loc
            ));
        }
    }

    if scope_roots.len() > thresholds.critical_scope_roots {
        severity = max_severity(severity, ArchitectureSeverity::Critical);
        trigger_reasons.push(format!(
            "scope spans {} roots ({}) which exceeds the critical threshold {}",
            scope_roots.len(),
            scope_roots.join(", "),
            thresholds.critical_scope_roots
        ));
    }

    if expected_interface_count > thresholds.warn_expected_interfaces {
        severity = max_severity(severity, ArchitectureSeverity::Warn);
        trigger_reasons.push(format!(
            "expected interfaces {} exceed warn threshold {}",
            expected_interface_count, thresholds.warn_expected_interfaces
        ));
    }

    if import_path_count > thresholds.warn_import_paths {
        severity = max_severity(severity, ArchitectureSeverity::Warn);
        trigger_reasons.push(format!(
            "import paths {} exceed warn threshold {}",
            import_path_count, thresholds.warn_import_paths
        ));
    }

    if has_cleanup_obligations {
        trigger_reasons.push("cleanup obligations detected in the current contract context".into());
    }
    if has_docs_obligations {
        trigger_reasons
            .push("documentation obligations detected in the current contract context".into());
    }
    if has_migration_sensitive_surfaces {
        severity = max_severity(severity, ArchitectureSeverity::Warn);
        trigger_reasons
            .push("migration-sensitive surfaces detected in the current contract context".into());
    }

    Ok(ArchitectureSignals {
        contract_id: input.contract_id.to_string(),
        feature_id: input.feature_id.to_string(),
        scope_roots,
        oversized_files,
        distinct_scope_roots,
        entry_point_count,
        expected_interface_count,
        import_path_count,
        has_cleanup_obligations,
        has_docs_obligations,
        has_migration_sensitive_surfaces,
        severity,
        trigger_reasons,
        thresholds,
        computed_at: now_rfc3339(),
    })
}

pub fn scope_roots(paths: &[String]) -> Vec<String> {
    let mut roots = BTreeSet::new();
    for path in paths {
        let Some(root) = scope_root(path) else {
            continue;
        };
        roots.insert(root);
    }
    roots.into_iter().collect()
}

pub fn line_count_for_path(repo_root: &Path, path: &str) -> Result<usize> {
    let bytes = fs::read(repo_root.join(path))?;
    if bytes.is_empty() {
        return Ok(0);
    }
    let contents = String::from_utf8_lossy(&bytes);
    Ok(contents.lines().count())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenPathDependencyViolation {
    pub from_path: String,
    pub to_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForbiddenPathDependencyScan {
    pub matched_files: Vec<String>,
    pub violating_edges: Vec<ForbiddenPathDependencyViolation>,
    pub unparsed_files: Vec<String>,
}

pub fn scan_forbidden_path_dependency(
    repo_root: &Path,
    candidate_files: &[String],
    from_glob: &str,
    to_glob: &str,
) -> Result<ForbiddenPathDependencyScan> {
    let matched_files = candidate_files
        .iter()
        .filter_map(|path| normalize_repo_path(Path::new(path)))
        .filter(|path| glob_matches_path(from_glob, path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let rust_source_roots = if matched_files.iter().any(|path| path.ends_with(".rs")) {
        workspace_rust_source_roots(repo_root)?
    } else {
        BTreeMap::new()
    };

    let mut violating_edges = BTreeSet::new();
    let mut unparsed_files = BTreeSet::new();

    for from_path in &matched_files {
        match dependency_targets_for_file(repo_root, from_path, &rust_source_roots)? {
            DependencyTargetScan::Parsed(targets) => {
                for to_path in targets {
                    if glob_matches_path(to_glob, &to_path) {
                        violating_edges.insert((from_path.clone(), to_path));
                    }
                }
            }
            DependencyTargetScan::Unparsed => {
                unparsed_files.insert(from_path.clone());
            }
        }
    }

    Ok(ForbiddenPathDependencyScan {
        matched_files,
        violating_edges: violating_edges
            .into_iter()
            .map(|(from_path, to_path)| ForbiddenPathDependencyViolation { from_path, to_path })
            .collect(),
        unparsed_files: unparsed_files.into_iter().collect(),
    })
}

fn combined_scope_paths(
    allowed_scope: &[String],
    entry_points: &[String],
    import_paths: &[String],
) -> Vec<String> {
    let mut combined = BTreeSet::new();
    for path in allowed_scope
        .iter()
        .chain(entry_points.iter())
        .chain(import_paths.iter())
    {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        combined.insert(trimmed.to_string());
    }
    combined.into_iter().collect()
}

fn collect_scope_files(repo_root: &Path, scope_paths: &[String]) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    for path in scope_paths {
        let absolute = repo_root.join(path);
        if absolute.is_file() {
            files.insert(path.clone());
            continue;
        }
        if absolute.is_dir() {
            collect_dir_files(repo_root, &absolute, &mut files)?;
        }
    }
    Ok(files.into_iter().collect())
}

fn collect_dir_files(repo_root: &Path, dir: &Path, files: &mut BTreeSet<String>) -> Result<()> {
    let mut entries = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    entries.sort();

    for entry in entries {
        if entry.is_dir() {
            if should_skip_dir(&entry) {
                continue;
            }
            collect_dir_files(repo_root, &entry, files)?;
            continue;
        }
        if let Ok(relative) = entry.strip_prefix(repo_root) {
            files.insert(relative.to_string_lossy().to_string());
        }
    }

    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| IGNORED_DIR_NAMES.iter().any(|ignored| ignored == &name))
        || path
            .components()
            .any(|component| matches!(component, Component::Normal(name) if name == ".git"))
}

enum DependencyTargetScan {
    Parsed(Vec<String>),
    Unparsed,
}

fn dependency_targets_for_file(
    repo_root: &Path,
    from_path: &str,
    rust_source_roots: &BTreeMap<String, Vec<String>>,
) -> Result<DependencyTargetScan> {
    let absolute = repo_root.join(from_path);
    if !absolute.exists() || absolute.is_dir() {
        return Ok(DependencyTargetScan::Parsed(Vec::new()));
    }

    let bytes = match fs::read(&absolute) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(DependencyTargetScan::Unparsed),
    };
    let contents = String::from_utf8_lossy(&bytes);

    let Some(extension) = absolute.extension().and_then(|ext| ext.to_str()) else {
        return Ok(DependencyTargetScan::Unparsed);
    };

    let targets = match extension {
        "rs" => rust_dependency_targets(repo_root, from_path, &contents, rust_source_roots),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            script_dependency_targets(repo_root, from_path, &contents)
        }
        _ => return Ok(DependencyTargetScan::Unparsed),
    };

    Ok(DependencyTargetScan::Parsed(targets))
}

fn rust_dependency_targets(
    repo_root: &Path,
    from_path: &str,
    contents: &str,
    rust_source_roots: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let mut targets = BTreeSet::new();
    let mut current = String::new();

    for line in contents.lines() {
        let trimmed = line.split("//").next().unwrap_or("").trim();
        if trimmed.is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(trimmed);
        if !trimmed.ends_with(';') {
            continue;
        }

        if let Some(use_clause) = rust_use_clause(&current) {
            for path in expand_rust_use_paths(use_clause) {
                for target in
                    resolve_rust_module_reference(repo_root, from_path, &path, rust_source_roots)
                {
                    targets.insert(target);
                }
            }
        } else if let Some(module_name) = rust_mod_name(&current) {
            if let Some(target) =
                resolve_relative_module(repo_root, from_path, &module_name, &["rs"])
            {
                targets.insert(target);
            }
        }

        current.clear();
    }

    targets.into_iter().collect()
}

fn script_dependency_targets(repo_root: &Path, from_path: &str, contents: &str) -> Vec<String> {
    const SCRIPT_EXTENSIONS: &[&str] = &["js", "jsx", "ts", "tsx", "mjs", "cjs"];

    let mut targets = BTreeSet::new();
    for specifier in extract_script_specifiers(contents) {
        if !specifier.starts_with('.') {
            continue;
        }
        if let Some(target) =
            resolve_relative_module(repo_root, from_path, &specifier, SCRIPT_EXTENSIONS)
        {
            targets.insert(target);
        }
    }
    targets.into_iter().collect()
}

fn rust_use_clause(statement: &str) -> Option<&str> {
    let trimmed = statement.trim();
    if let Some(rest) = trimmed.strip_prefix("use ") {
        return Some(rest.trim_end_matches(';').trim());
    }
    let index = trimmed.find(" use ")?;
    trimmed
        .starts_with("pub")
        .then_some(trimmed[index + 5..].trim_end_matches(';').trim())
}

fn rust_mod_name(statement: &str) -> Option<String> {
    let trimmed = statement.trim().trim_end_matches(';').trim();
    let rest = if let Some(rest) = trimmed.strip_prefix("mod ") {
        rest
    } else {
        let index = trimmed.find(" mod ")?;
        trimmed
            .starts_with("pub")
            .then_some(&trimmed[index + 5..])?
    };

    let name = rest.split_whitespace().next()?.trim();
    (!name.is_empty() && !name.contains("::") && name != "{").then_some(name.to_string())
}

fn expand_rust_use_paths(use_clause: &str) -> Vec<String> {
    expand_rust_use_paths_inner(use_clause.trim())
        .into_iter()
        .filter_map(|path| {
            let path = strip_rust_use_alias(&path);
            let path = path.trim().trim_end_matches("::*").trim_end_matches("::");
            (!path.is_empty()).then_some(path.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn expand_rust_use_paths_inner(clause: &str) -> Vec<String> {
    let clause = clause.trim().trim_end_matches(';').trim();
    if clause.is_empty() {
        return Vec::new();
    }

    let Some((open, close)) = top_level_brace_pair(clause) else {
        return vec![clause.to_string()];
    };

    let prefix = clause[..open].trim().trim_end_matches("::").trim();
    let inner = &clause[open + 1..close];
    let mut expanded = Vec::new();

    for item in split_top_level(inner, ',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if item == "self" {
            if !prefix.is_empty() {
                expanded.push(prefix.to_string());
            }
            continue;
        }
        let combined = if prefix.is_empty() {
            item.to_string()
        } else {
            format!("{prefix}::{item}")
        };
        expanded.extend(expand_rust_use_paths_inner(&combined));
    }

    expanded
}

fn strip_rust_use_alias(path: &str) -> &str {
    path.split(" as ").next().unwrap_or(path).trim()
}

fn top_level_brace_pair(input: &str) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut open_index = None;

    for (index, ch) in input.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    open_index = Some(index);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                if depth == 0 {
                    return open_index.map(|open| (open, index));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level(input: &str, delimiter: char) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, ch) in input.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if ch == delimiter && depth == 0 => {
                items.push(input[start..index].to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    items.push(input[start..].to_string());
    items
}

fn resolve_rust_module_reference(
    repo_root: &Path,
    from_path: &str,
    reference: &str,
    rust_source_roots: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let reference = reference.trim().trim_start_matches("::").trim();
    if reference.is_empty() {
        return Vec::new();
    }

    let segments = reference
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Vec::new();
    }

    let (bases, start_index) = match segments[0] {
        "crate" => match crate_source_root(from_path) {
            Some(base) => (vec![base], 1usize),
            None => return Vec::new(),
        },
        "self" => match current_rust_module_dir(from_path) {
            Some(base) => (vec![base], 1usize),
            None => return Vec::new(),
        },
        "super" => {
            let Some(mut base) = current_rust_module_dir(from_path) else {
                return Vec::new();
            };
            let mut index = 0usize;
            while index < segments.len() && segments[index] == "super" {
                let Some(parent) = base.parent() else {
                    return Vec::new();
                };
                base = parent.to_path_buf();
                index += 1;
            }
            (vec![base], index)
        }
        segment => {
            if let Some(roots) = rust_source_roots.get(segment) {
                (roots.iter().map(PathBuf::from).collect::<Vec<_>>(), 1usize)
            } else {
                match crate_source_root(from_path) {
                    Some(base) => (vec![base], 0usize),
                    None => return Vec::new(),
                }
            }
        }
    };

    let mut resolved = BTreeSet::new();
    for base in bases {
        if let Some(target) = resolve_module_reference(repo_root, &base, &segments[start_index..]) {
            resolved.insert(target);
        }
    }
    resolved.into_iter().collect()
}

fn resolve_relative_module(
    repo_root: &Path,
    from_path: &str,
    module: &str,
    extensions: &[&str],
) -> Option<String> {
    let from_dir = repo_root.join(from_path).parent()?.to_path_buf();
    let candidate = from_dir.join(module);
    resolve_module_candidate(repo_root, &candidate, extensions)
}

fn resolve_module_reference(repo_root: &Path, base: &Path, segments: &[&str]) -> Option<String> {
    if segments.is_empty() {
        return relative_repo_path(repo_root, &repo_root.join(base));
    }

    for length in (1..=segments.len()).rev() {
        let mut candidate = repo_root.join(base);
        for segment in &segments[..length] {
            candidate.push(segment);
        }
        if let Some(target) = resolve_module_candidate(repo_root, &candidate, &["rs"]) {
            return Some(target);
        }
    }

    None
}

fn resolve_module_candidate(
    repo_root: &Path,
    candidate: &Path,
    extensions: &[&str],
) -> Option<String> {
    if candidate.is_file() || candidate.is_dir() {
        if let Some(path) = relative_repo_path(repo_root, candidate) {
            return Some(path);
        }
    }

    for extension in extensions {
        let file_candidate = candidate.with_extension(extension);
        if file_candidate.is_file() {
            if let Some(path) = relative_repo_path(repo_root, &file_candidate) {
                return Some(path);
            }
        }
    }

    for extension in extensions {
        let index_candidate = candidate.join(format!("index.{extension}"));
        if index_candidate.is_file() {
            if let Some(path) = relative_repo_path(repo_root, &index_candidate) {
                return Some(path);
            }
        }
    }

    let mod_candidate = candidate.join("mod.rs");
    if mod_candidate.is_file() {
        return relative_repo_path(repo_root, &mod_candidate);
    }

    None
}

fn current_rust_module_dir(from_path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(from_path);
    let parent = path.parent()?.to_path_buf();
    let file_name = path.file_name()?.to_str()?;
    match file_name {
        "lib.rs" | "main.rs" | "mod.rs" => Some(parent),
        _ => Some(parent.join(path.file_stem()?)),
    }
}

fn crate_source_root(from_path: &str) -> Option<PathBuf> {
    let path = Path::new(from_path);
    let components = path.components().collect::<Vec<_>>();
    let source_index = components.iter().position(
        |component| matches!(component, Component::Normal(segment) if *segment == "src"),
    )?;

    let mut root = PathBuf::new();
    for component in &components[..=source_index] {
        if let Component::Normal(segment) = component {
            root.push(segment);
        }
    }
    Some(root)
}

fn workspace_rust_source_roots(repo_root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut manifests = Vec::new();
    collect_cargo_manifests(repo_root, repo_root, &mut manifests)?;

    let mut roots: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for manifest in manifests {
        let Some(package_name) = cargo_package_name(&manifest) else {
            continue;
        };
        let Some(manifest_dir) = manifest.parent() else {
            continue;
        };
        let Some(relative_src) = relative_repo_path(repo_root, &manifest_dir.join("src")) else {
            continue;
        };
        roots
            .entry(package_name.clone())
            .or_default()
            .push(relative_src.clone());
        roots
            .entry(package_name.replace('-', "_"))
            .or_default()
            .push(relative_src);
    }
    for paths in roots.values_mut() {
        paths.sort();
        paths.dedup();
    }
    Ok(roots)
}

fn collect_cargo_manifests(
    repo_root: &Path,
    current: &Path,
    manifests: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut entries = fs::read_dir(current)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    entries.sort();

    for entry in entries {
        if entry.is_dir() {
            if should_skip_dir(&entry) {
                continue;
            }
            collect_cargo_manifests(repo_root, &entry, manifests)?;
            continue;
        }
        if entry.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml")
            && entry.strip_prefix(repo_root).is_ok()
        {
            manifests.push(entry);
        }
    }

    Ok(())
}

fn cargo_package_name(manifest: &Path) -> Option<String> {
    let contents = fs::read_to_string(manifest).ok()?;
    let mut in_package = false;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package || !trimmed.starts_with("name") {
            continue;
        }
        let (_, value) = trimmed.split_once('=')?;
        let value = value.trim().trim_matches('"');
        if value.is_empty() {
            continue;
        }
        return Some(value.to_string());
    }

    None
}

fn extract_script_specifiers(contents: &str) -> Vec<String> {
    let mut specifiers = BTreeSet::new();
    for marker in [
        "from \"",
        "from '",
        "require(\"",
        "require('",
        "import(\"",
        "import('",
        "import \"",
        "import '",
    ] {
        collect_quoted_after_marker(contents, marker, &mut specifiers);
    }
    specifiers.into_iter().collect()
}

fn collect_quoted_after_marker(contents: &str, marker: &str, specifiers: &mut BTreeSet<String>) {
    let Some(quote) = marker.chars().last() else {
        return;
    };
    let mut offset = 0usize;
    while let Some(index) = contents[offset..].find(marker) {
        let start = offset + index + marker.len();
        let Some(end) = contents[start..].find(quote) else {
            break;
        };
        let specifier = contents[start..start + end].trim();
        if !specifier.is_empty() {
            specifiers.insert(specifier.to_string());
        }
        offset = start + end + quote.len_utf8();
    }
}

fn glob_matches_path(pattern: &str, path: &str) -> bool {
    let pattern = normalize_glob(pattern);
    let path = normalize_glob(path);
    let pattern_segments = pattern
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let path_segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    glob_matches_segments(&pattern_segments, &path_segments)
}

fn normalize_glob(value: &str) -> String {
    value.replace('\\', "/")
}

fn glob_matches_segments(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        return glob_matches_segments(&pattern[1..], path)
            || (!path.is_empty() && glob_matches_segments(pattern, &path[1..]));
    }
    !path.is_empty()
        && wildcard_match_segment(pattern[0], path[0])
        && glob_matches_segments(&pattern[1..], &path[1..])
}

fn wildcard_match_segment(pattern: &str, value: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == value;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    let mut remainder = value;
    let mut first = true;

    for (index, part) in parts.iter().enumerate() {
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
        if index == parts.len() - 1 && !pattern.ends_with('*') {
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

fn relative_repo_path(repo_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(repo_root).ok()?;
    normalize_repo_path(relative)
}

fn normalize_repo_path(path: &Path) -> Option<String> {
    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(segment) => normalized.push(segment.to_string_lossy().to_string()),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!normalized.is_empty()).then_some(normalized.join("/"))
}

fn scope_root(path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let mut components = candidate.components();
    match components.next() {
        Some(Component::Normal(component)) => Some(component.to_string_lossy().to_string()),
        _ => None,
    }
}

fn distinct_count(values: &[String]) -> usize {
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .len()
}

fn has_obligation_keywords(
    prompt_source: &str,
    behavior_requirements: &[String],
    keywords: &[&str],
) -> bool {
    text_contains_any(prompt_source, keywords)
        || behavior_requirements
            .iter()
            .any(|item| text_contains_any(item, keywords))
}

fn has_docs_obligations(
    allowed_scope: &[String],
    entry_points: &[String],
    behavior_requirements: &[String],
) -> bool {
    allowed_scope
        .iter()
        .chain(entry_points.iter())
        .any(|path| is_docs_path(path))
        || behavior_requirements
            .iter()
            .any(|item| text_contains_any(item, DOC_KEYWORDS))
}

fn is_docs_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "readme.md"
        || lower.starts_with("docs/")
        || lower.ends_with(".md")
        || lower == "agents.md"
}

fn text_contains_any(text: &str, keywords: &[&str]) -> bool {
    let lowered = text.to_ascii_lowercase();
    keywords.iter().any(|keyword| lowered.contains(keyword))
}

fn max_severity(
    current: ArchitectureSeverity,
    candidate: ArchitectureSeverity,
) -> ArchitectureSeverity {
    if severity_rank(&candidate) > severity_rank(&current) {
        candidate
    } else {
        current
    }
}

fn severity_rank(severity: &ArchitectureSeverity) -> usize {
    match severity {
        ArchitectureSeverity::None => 0,
        ArchitectureSeverity::Warn => 1,
        ArchitectureSeverity::Critical => 2,
    }
}

const CLEANUP_KEYWORDS: &[&str] = &[
    "cleanup",
    "remove",
    "delete",
    "retire",
    "replace",
    "supersede",
    "prune",
];

const DOC_KEYWORDS: &[&str] = &[
    "docs",
    "documentation",
    "readme",
    "architecture.md",
    "cli.md",
];

const MIGRATION_KEYWORDS: &[&str] = &[
    "migration",
    "migrate",
    "schema",
    "rename",
    "backfill",
    "compatibility",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_repo_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "punk-core-architecture-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn scope_roots_collapse_paths_to_top_level_zones() {
        let roots = scope_roots(&[
            "crates/punk-core/src/lib.rs".into(),
            "docs/product/CLI.md".into(),
            "README.md".into(),
        ]);
        assert_eq!(roots, vec!["README.md", "crates", "docs"]);
    }

    #[test]
    fn compute_architecture_signals_marks_large_files_warn_and_critical() {
        let root = temp_repo_root("loc-thresholds");
        fs::create_dir_all(root.join("src")).unwrap();
        let warn_contents = std::iter::repeat_n("pub fn warn() {}\n", 650).collect::<String>();
        let critical_contents =
            std::iter::repeat_n("pub fn critical() {}\n", 1300).collect::<String>();
        fs::write(root.join("src/warn.rs"), warn_contents).unwrap();
        fs::write(root.join("src/critical.rs"), critical_contents).unwrap();

        let warn = compute_architecture_signals(
            &root,
            ArchitectureSignalInput {
                contract_id: "ct_warn",
                feature_id: "feat_warn",
                prompt_source: "tighten architecture steering",
                allowed_scope: &["src/warn.rs".into()],
                entry_points: &["src/warn.rs".into()],
                import_paths: &[],
                expected_interfaces: &["warn path".into()],
                behavior_requirements: &["keep bounded".into()],
            },
        )
        .unwrap();
        assert_eq!(warn.severity, ArchitectureSeverity::Warn);
        assert_eq!(warn.oversized_files[0].path, "src/warn.rs");

        let critical = compute_architecture_signals(
            &root,
            ArchitectureSignalInput {
                contract_id: "ct_critical",
                feature_id: "feat_critical",
                prompt_source: "tighten architecture steering",
                allowed_scope: &["src/critical.rs".into()],
                entry_points: &["src/critical.rs".into()],
                import_paths: &[],
                expected_interfaces: &["critical path".into()],
                behavior_requirements: &["keep bounded".into()],
            },
        )
        .unwrap();
        assert_eq!(critical.severity, ArchitectureSeverity::Critical);
        assert_eq!(critical.oversized_files[0].path, "src/critical.rs");
    }

    #[test]
    fn compute_architecture_signals_marks_multi_root_scope_critical() {
        let root = temp_repo_root("multi-root");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
        fs::write(root.join("docs/CLI.md"), "# CLI\n").unwrap();

        let signals = compute_architecture_signals(
            &root,
            ArchitectureSignalInput {
                contract_id: "ct_multi_root",
                feature_id: "feat_multi_root",
                prompt_source: "update runtime and docs",
                allowed_scope: &["src/lib.rs".into(), "docs/CLI.md".into()],
                entry_points: &["src/lib.rs".into(), "docs/CLI.md".into()],
                import_paths: &[],
                expected_interfaces: &["runtime summary".into()],
                behavior_requirements: &["update docs".into()],
            },
        )
        .unwrap();

        assert_eq!(signals.severity, ArchitectureSeverity::Critical);
        assert_eq!(signals.scope_roots, vec!["docs", "src"]);
        assert!(signals
            .trigger_reasons
            .iter()
            .any(|reason| reason.contains("scope spans 2 roots")));
    }

    #[test]
    fn scan_forbidden_path_dependency_detects_rust_cross_crate_use() {
        let root = temp_repo_root("forbidden-rust-dependency");
        fs::create_dir_all(root.join("crates/app-core/src")).unwrap();
        fs::create_dir_all(root.join("crates/forbidden/src")).unwrap();
        fs::write(
            root.join("crates/app-core/Cargo.toml"),
            "[package]\nname = \"app-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/forbidden/Cargo.toml"),
            "[package]\nname = \"forbidden\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/app-core/src/lib.rs"),
            "use forbidden::api::Client;\npub fn build() -> Client { todo!() }\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/forbidden/src/api.rs"),
            "pub struct Client;\n",
        )
        .unwrap();

        let scan = scan_forbidden_path_dependency(
            &root,
            &["crates/app-core/src/lib.rs".into()],
            "crates/app-core/**",
            "crates/forbidden/**",
        )
        .unwrap();

        assert_eq!(scan.matched_files, vec!["crates/app-core/src/lib.rs"]);
        assert!(scan.unparsed_files.is_empty());
        assert_eq!(
            scan.violating_edges,
            vec![ForbiddenPathDependencyViolation {
                from_path: "crates/app-core/src/lib.rs".into(),
                to_path: "crates/forbidden/src/api.rs".into(),
            }]
        );
    }

    #[test]
    fn scan_forbidden_path_dependency_detects_script_relative_import() {
        let root = temp_repo_root("forbidden-script-dependency");
        fs::create_dir_all(root.join("src/ui")).unwrap();
        fs::create_dir_all(root.join("src/data")).unwrap();
        fs::write(
            root.join("src/ui/view.ts"),
            "import { query } from \"../data/query\";\nexport const view = query;\n",
        )
        .unwrap();
        fs::write(root.join("src/data/query.ts"), "export const query = 1;\n").unwrap();

        let scan = scan_forbidden_path_dependency(
            &root,
            &["src/ui/view.ts".into()],
            "src/ui/**",
            "src/data/**",
        )
        .unwrap();

        assert_eq!(
            scan.violating_edges,
            vec![ForbiddenPathDependencyViolation {
                from_path: "src/ui/view.ts".into(),
                to_path: "src/data/query.ts".into(),
            }]
        );
    }

    #[test]
    fn scan_forbidden_path_dependency_uses_all_matching_package_roots() {
        let root = temp_repo_root("duplicate-package-roots");
        fs::create_dir_all(root.join("crates/app/src")).unwrap();
        fs::create_dir_all(root.join("crates/specpunk/src")).unwrap();
        fs::create_dir_all(root.join("punk/punk-cli/src")).unwrap();
        fs::write(
            root.join("crates/app/Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/specpunk/Cargo.toml"),
            "[package]\nname = \"punk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-cli/Cargo.toml"),
            "[package]\nname = \"punk-cli\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/app/src/lib.rs"),
            "use punk_cli::commands::run;\npub fn call() { run(); }\n",
        )
        .unwrap();
        fs::write(
            root.join("crates/specpunk/src/commands.rs"),
            "pub fn run() {}\n",
        )
        .unwrap();
        fs::write(
            root.join("punk/punk-cli/src/lib.rs"),
            "pub fn shadow() {}\n",
        )
        .unwrap();

        let scan = scan_forbidden_path_dependency(
            &root,
            &["crates/app/src/lib.rs".into()],
            "crates/app/**",
            "crates/specpunk/**",
        )
        .unwrap();

        assert_eq!(
            scan.violating_edges,
            vec![ForbiddenPathDependencyViolation {
                from_path: "crates/app/src/lib.rs".into(),
                to_path: "crates/specpunk/src/commands.rs".into(),
            }]
        );
    }
}
