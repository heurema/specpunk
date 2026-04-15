use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use punk_domain::{
    now_rfc3339, CapabilityCandidateView, CapabilityScopeSeeds, FrozenCapabilityResolution,
    FrozenCapabilitySpec, ProjectCapabilityIndex, RepoCapabilityResolution,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinCapabilityKind {
    RustCargo,
    NodePackageScripts,
    GoMod,
    PythonPyprojectPytest,
    Swiftpm,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltinCapabilitySpec {
    pub id: &'static str,
    pub version: &'static str,
    pub source_kind: &'static str,
    pub kind: BuiltinCapabilityKind,
    pub detect_markers: &'static [&'static str],
    pub ignore_names: &'static [&'static str],
    pub default_directories: &'static [&'static str],
    pub extra_file_hints: &'static [&'static str],
    pub controller_scaffold_kind: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCapability {
    pub view: CapabilityCandidateView,
    pub ignore_rules: Vec<String>,
    pub scope_seeds: CapabilityScopeSeeds,
    pub target_checks: Vec<String>,
    pub integrity_checks: Vec<String>,
    pub controller_scaffold_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCapabilitySet {
    pub resolution: RepoCapabilityResolution,
    pub active: Vec<ResolvedCapability>,
    pub package_manager: Option<String>,
    pub available_scripts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct BuiltinCapabilityFingerprint<'a> {
    id: &'a str,
    version: &'a str,
    source_kind: &'a str,
    kind: &'a str,
    detect_markers: &'a [&'a str],
    ignore_names: &'a [&'a str],
    default_directories: &'a [&'a str],
    extra_file_hints: &'a [&'a str],
    controller_scaffold_kind: Option<&'a str>,
}

pub(crate) fn builtin_specs() -> Vec<BuiltinCapabilitySpec> {
    vec![
        BuiltinCapabilitySpec {
            id: "rust-cargo",
            version: "1",
            source_kind: "builtin",
            kind: BuiltinCapabilityKind::RustCargo,
            detect_markers: &["Cargo.toml"],
            ignore_names: &["target"],
            default_directories: &["crates", "src", "tests"],
            extra_file_hints: &[],
            controller_scaffold_kind: Some("rust-cargo"),
        },
        BuiltinCapabilitySpec {
            id: "node-package-scripts",
            version: "1",
            source_kind: "builtin",
            kind: BuiltinCapabilityKind::NodePackageScripts,
            detect_markers: &["package.json"],
            ignore_names: &["node_modules", "dist"],
            default_directories: &["packages", "apps", "src", "tests"],
            extra_file_hints: &["tsconfig.json"],
            controller_scaffold_kind: None,
        },
        BuiltinCapabilitySpec {
            id: "go-mod",
            version: "1",
            source_kind: "builtin",
            kind: BuiltinCapabilityKind::GoMod,
            detect_markers: &["go.mod"],
            ignore_names: &[],
            default_directories: &["cmd", "internal", "pkg"],
            extra_file_hints: &[],
            controller_scaffold_kind: Some("go-mod"),
        },
        BuiltinCapabilitySpec {
            id: "python-pyproject-pytest",
            version: "1",
            source_kind: "builtin",
            kind: BuiltinCapabilityKind::PythonPyprojectPytest,
            detect_markers: &["pyproject.toml", "pytest.ini", "requirements.txt"],
            ignore_names: &[".venv", ".pytest_cache", "dist"],
            default_directories: &["src", "tests"],
            extra_file_hints: &[],
            controller_scaffold_kind: Some("python-pyproject-pytest"),
        },
        BuiltinCapabilitySpec {
            id: "swiftpm",
            version: "1",
            source_kind: "builtin",
            kind: BuiltinCapabilityKind::Swiftpm,
            detect_markers: &["Package.swift"],
            ignore_names: &[".build"],
            default_directories: &["Sources", "Tests"],
            extra_file_hints: &[],
            controller_scaffold_kind: None,
        },
    ]
}

pub fn capability_generated_noise_path(path: &str) -> bool {
    let path = path.trim_matches('/');
    if path.is_empty() {
        return false;
    }
    let components = path
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    components
        .iter()
        .any(|component| capability_ignore_name(component))
}

pub(crate) fn capability_ignore_name(name: &str) -> bool {
    builtin_specs()
        .into_iter()
        .flat_map(|spec| spec.ignore_names.iter().copied())
        .any(|candidate| candidate == name)
}

pub(crate) fn capability_ignored_relative_path(relative: &Path) -> bool {
    let components = relative
        .components()
        .map(super::component_to_string)
        .collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| capability_ignore_name(component))
    {
        return true;
    }
    components.starts_with(&["docs".to_string(), "reference-repos".to_string()])
        || components.starts_with(&[
            "docs".to_string(),
            "research".to_string(),
            "_delve_runs".to_string(),
        ])
}

pub fn scope_seeds_for_entry_point(entry_point: &str) -> Option<CapabilityScopeSeeds> {
    builtin_specs()
        .into_iter()
        .find(|spec| spec.detect_markers.contains(&entry_point))
        .map(|spec| default_scope_seeds_for_spec(&spec, &[]))
}

pub fn scope_seeds_for_entry_point_with_prompt(
    entry_point: &str,
    prompt: &str,
) -> Option<CapabilityScopeSeeds> {
    let prompt_tokens = super::prompt_tokens(prompt);
    builtin_specs()
        .into_iter()
        .find(|spec| spec.detect_markers.contains(&entry_point))
        .map(|spec| default_scope_seeds_for_spec(&spec, &prompt_tokens))
}

pub fn build_project_capability_index(
    project_id: &str,
    resolution: Option<&RepoCapabilityResolution>,
) -> ProjectCapabilityIndex {
    let empty = RepoCapabilityResolution {
        resolution_mode: "builtin_only_v1".to_string(),
        detected: Vec::new(),
        active: Vec::new(),
        suppressed: Vec::new(),
        conflicted: Vec::new(),
        advisory: Vec::new(),
        generated_at: now_rfc3339(),
    };
    let resolution = resolution.unwrap_or(&empty);
    ProjectCapabilityIndex {
        schema: "specpunk/project-capability-index/v1".to_string(),
        version: 1,
        project_id: project_id.to_string(),
        source_kind: "builtin".to_string(),
        resolution_mode: resolution.resolution_mode.clone(),
        detected: resolution.detected.clone(),
        active: resolution.active.clone(),
        suppressed: resolution.suppressed.clone(),
        conflicted: resolution.conflicted.clone(),
        advisory: resolution.advisory.clone(),
        generated_at: now_rfc3339(),
    }
}

pub fn freeze_contract_capability_resolution(
    repo_root: &Path,
    contract: &punk_domain::Contract,
    project_index: &ProjectCapabilityIndex,
    project_capability_index_ref: &str,
    project_capability_index_sha256: &str,
) -> Result<FrozenCapabilityResolution> {
    let resolved = resolve_repo_capabilities(repo_root, &contract.prompt_source)?;
    let mut selected = Vec::new();
    let mut union_ids = project_index
        .active
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<BTreeSet<_>>();
    union_ids.extend(
        resolved
            .active
            .iter()
            .map(|candidate| candidate.view.id.clone()),
    );

    for capability_id in union_ids {
        let Some(spec) = spec_by_id(&capability_id) else {
            continue;
        };
        let resolved_candidate = resolved
            .active
            .iter()
            .find(|candidate| candidate.view.id == capability_id);
        let project_candidate = project_index
            .active
            .iter()
            .find(|candidate| candidate.id == capability_id);
        let view = resolved_candidate
            .map(|candidate| candidate.view.clone())
            .or_else(|| project_candidate.cloned())
            .unwrap_or_else(|| CapabilityCandidateView {
                id: capability_id.clone(),
                version: spec.version.to_string(),
                source_kind: spec.source_kind.to_string(),
                semantic_hash: semantic_hash_for_spec(&spec),
                matched_markers: Vec::new(),
                path_scopes: Vec::new(),
            });
        let target_checks = resolved_candidate
            .map(|candidate| candidate.target_checks.clone())
            .unwrap_or_else(|| {
                target_checks_for_spec(
                    repo_root,
                    &spec,
                    &contract.prompt_source,
                    &view.matched_markers,
                )
            });
        let integrity_checks = resolved_candidate
            .map(|candidate| candidate.integrity_checks.clone())
            .unwrap_or_else(|| {
                integrity_checks_for_spec(
                    repo_root,
                    &spec,
                    &contract.prompt_source,
                    &view.matched_markers,
                )
            });
        let scope_seeds = resolved_candidate
            .map(|candidate| candidate.scope_seeds.clone())
            .unwrap_or_else(|| {
                default_scope_seeds_for_spec(&spec, &super::prompt_tokens(&contract.prompt_source))
            });
        let ignore_rules = resolved_candidate
            .map(|candidate| candidate.ignore_rules.clone())
            .unwrap_or_else(|| {
                spec.ignore_names
                    .iter()
                    .map(|item| item.to_string())
                    .collect()
            });
        let controller_scaffold_kind = resolved_candidate
            .and_then(|candidate| candidate.controller_scaffold_kind.clone())
            .or_else(|| {
                contract_implies_controller_scaffold(contract, &spec)
                    .then(|| spec.controller_scaffold_kind.map(str::to_string))
                    .flatten()
            });

        let intersects_scope = contract
            .entry_points
            .iter()
            .chain(contract.allowed_scope.iter())
            .any(|path| capability_matches_contract_path(&view, path));
        let intersects_checks = target_checks
            .iter()
            .chain(integrity_checks.iter())
            .any(|check| {
                contract.target_checks.contains(check) || contract.integrity_checks.contains(check)
            });
        let scaffold_matches = controller_scaffold_kind
            .as_ref()
            .is_some_and(|kind| contract_matches_scaffold_kind(contract, &spec, kind));
        let include = intersects_scope || intersects_checks || scaffold_matches;
        if include {
            selected.push(FrozenCapabilitySpec {
                id: view.id.clone(),
                version: view.version.clone(),
                source_kind: view.source_kind.clone(),
                semantic_hash: view.semantic_hash.clone(),
                matched_markers: view.matched_markers.clone(),
                path_scopes: view.path_scopes.clone(),
                ignore_rules,
                scope_seeds,
                target_checks,
                integrity_checks,
                controller_scaffold_kind,
            });
        }
    }

    if selected.is_empty() && project_index.active.len() == 1 {
        if let Some(spec) = spec_by_id(&project_index.active[0].id) {
            let candidate = &project_index.active[0];
            let controller_scaffold_kind = contract_implies_controller_scaffold(contract, &spec)
                .then(|| spec.controller_scaffold_kind.map(str::to_string))
                .flatten();
            selected.push(FrozenCapabilitySpec {
                id: candidate.id.clone(),
                version: candidate.version.clone(),
                source_kind: candidate.source_kind.clone(),
                semantic_hash: candidate.semantic_hash.clone(),
                matched_markers: candidate.matched_markers.clone(),
                path_scopes: candidate.path_scopes.clone(),
                ignore_rules: spec
                    .ignore_names
                    .iter()
                    .map(|item| item.to_string())
                    .collect(),
                scope_seeds: default_scope_seeds_for_spec(
                    &spec,
                    &super::prompt_tokens(&contract.prompt_source),
                ),
                target_checks: target_checks_for_spec(
                    repo_root,
                    &spec,
                    &contract.prompt_source,
                    &candidate.matched_markers,
                ),
                integrity_checks: integrity_checks_for_spec(
                    repo_root,
                    &spec,
                    &contract.prompt_source,
                    &candidate.matched_markers,
                ),
                controller_scaffold_kind,
            });
        }
    }

    let mut ignore_rules = BTreeSet::new();
    let mut scope_seeds = CapabilityScopeSeeds::default();
    let mut target_checks = BTreeSet::new();
    let mut integrity_checks = BTreeSet::new();
    let mut controller_scaffold_kind = None;
    for capability in &selected {
        ignore_rules.extend(capability.ignore_rules.iter().cloned());
        scope_seeds
            .entry_points
            .extend(capability.scope_seeds.entry_points.iter().cloned());
        scope_seeds
            .file_paths
            .extend(capability.scope_seeds.file_paths.iter().cloned());
        scope_seeds
            .directory_paths
            .extend(capability.scope_seeds.directory_paths.iter().cloned());
        target_checks.extend(capability.target_checks.iter().cloned());
        integrity_checks.extend(capability.integrity_checks.iter().cloned());
        if controller_scaffold_kind.is_none() {
            controller_scaffold_kind = capability.controller_scaffold_kind.clone();
        }
    }
    super::dedupe(&mut scope_seeds.entry_points);
    super::dedupe(&mut scope_seeds.file_paths);
    super::dedupe(&mut scope_seeds.directory_paths);

    Ok(FrozenCapabilityResolution {
        schema: "specpunk/contract-capability-resolution/v1".to_string(),
        version: 1,
        contract_id: contract.id.clone(),
        project_capability_index_ref: project_capability_index_ref.to_string(),
        project_capability_index_sha256: project_capability_index_sha256.to_string(),
        selected_capabilities: selected,
        ignore_rules: ignore_rules.into_iter().collect(),
        scope_seeds,
        target_checks: target_checks.into_iter().collect(),
        integrity_checks: integrity_checks.into_iter().collect(),
        controller_scaffold_kind,
        generated_at: now_rfc3339(),
    })
}

pub(crate) fn resolve_repo_capabilities(
    repo_root: &Path,
    prompt: &str,
) -> Result<ResolvedCapabilitySet> {
    let prompt_tokens = super::prompt_tokens(prompt);
    let manifest_hits = collect_manifest_hits(repo_root)?;
    let mut active = Vec::new();
    let package_json_path = repo_root.join("package.json");
    let (package_manager, available_scripts) = if package_json_path.exists() {
        let package_manager = super::detect_package_manager(repo_root);
        let available_scripts = read_package_scripts(&package_json_path)?;
        (package_manager, available_scripts)
    } else {
        (None, BTreeMap::new())
    };

    for spec in builtin_specs() {
        let matched_markers = spec
            .detect_markers
            .iter()
            .flat_map(|marker| manifest_hits.get(*marker).cloned().unwrap_or_default())
            .collect::<Vec<_>>();
        let is_greenfield = matched_markers.is_empty()
            && super::repo_has_bootstrap_markers(repo_root)
            && prompt_requests_greenfield_scaffold(spec.kind, &prompt_tokens);
        if matched_markers.is_empty() && !is_greenfield {
            continue;
        }

        let path_scopes = if matched_markers.is_empty() {
            let seeds = default_scope_seeds_for_spec(&spec, &prompt_tokens);
            seeds
                .entry_points
                .iter()
                .chain(seeds.file_paths.iter())
                .chain(seeds.directory_paths.iter())
                .cloned()
                .collect::<Vec<_>>()
        } else {
            marker_path_scopes(&matched_markers)
        };
        let view = CapabilityCandidateView {
            id: spec.id.to_string(),
            version: spec.version.to_string(),
            source_kind: spec.source_kind.to_string(),
            semantic_hash: semantic_hash_for_spec(&spec),
            matched_markers: if matched_markers.is_empty() {
                vec![format!("prompt:greenfield:{}", spec.id)]
            } else {
                matched_markers.clone()
            },
            path_scopes,
        };
        let scope_seeds = if is_greenfield {
            default_scope_seeds_for_spec(&spec, &prompt_tokens)
        } else {
            scope_seeds_from_markers(repo_root, &spec, &matched_markers)
        };
        let controller_scaffold_kind = if is_greenfield {
            spec.controller_scaffold_kind.map(str::to_string)
        } else {
            None
        };
        active.push(ResolvedCapability {
            target_checks: target_checks_for_spec_with_scripts(
                repo_root,
                &spec,
                &prompt_tokens,
                &view.matched_markers,
                package_manager.as_deref(),
                &available_scripts,
            ),
            integrity_checks: integrity_checks_for_spec_with_scripts(
                repo_root,
                &spec,
                &prompt_tokens,
                &view.matched_markers,
                package_manager.as_deref(),
                &available_scripts,
            ),
            ignore_rules: spec
                .ignore_names
                .iter()
                .map(|item| item.to_string())
                .collect(),
            scope_seeds,
            controller_scaffold_kind,
            view,
        });
    }

    active.sort_by(|left, right| left.view.id.cmp(&right.view.id));
    let active_views = active
        .iter()
        .map(|candidate| candidate.view.clone())
        .collect::<Vec<_>>();
    Ok(ResolvedCapabilitySet {
        resolution: RepoCapabilityResolution {
            resolution_mode: "builtin_only_v1".to_string(),
            detected: active_views.clone(),
            active: active_views,
            suppressed: Vec::new(),
            conflicted: Vec::new(),
            advisory: Vec::new(),
            generated_at: now_rfc3339(),
        },
        active,
        package_manager,
        available_scripts,
    })
}

fn collect_manifest_hits(repo_root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut hits = BTreeMap::new();
    collect_manifest_hits_inner(repo_root, repo_root, &mut hits)?;
    Ok(hits)
}

fn collect_manifest_hits_inner(
    repo_root: &Path,
    current: &Path,
    hits: &mut BTreeMap<String, Vec<String>>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(repo_root)
            .with_context(|| format!("path escaped repo root: {}", path.display()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if super::ignored_name(&name) || capability_ignore_name(&name) {
            continue;
        }
        if capability_ignored_relative_path(relative) || super::ignored_relative_path(relative) {
            continue;
        }
        if path.is_dir() {
            collect_manifest_hits_inner(repo_root, &path, hits)?;
            continue;
        }
        hits.entry(name)
            .or_default()
            .push(relative.to_string_lossy().to_string());
    }
    Ok(())
}

fn read_package_scripts(path: &Path) -> Result<BTreeMap<String, String>> {
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    Ok(value
        .get("scripts")
        .and_then(|scripts| scripts.as_object())
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default())
}

fn semantic_hash_for_spec(spec: &BuiltinCapabilitySpec) -> String {
    let fingerprint = BuiltinCapabilityFingerprint {
        id: spec.id,
        version: spec.version,
        source_kind: spec.source_kind,
        kind: capability_kind_label(spec.kind),
        detect_markers: spec.detect_markers,
        ignore_names: spec.ignore_names,
        default_directories: spec.default_directories,
        extra_file_hints: spec.extra_file_hints,
        controller_scaffold_kind: spec.controller_scaffold_kind,
    };
    let bytes = serde_json::to_vec(&fingerprint).expect("serialize capability fingerprint");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn spec_by_id(id: &str) -> Option<BuiltinCapabilitySpec> {
    builtin_specs().into_iter().find(|spec| spec.id == id)
}

fn capability_kind_label(kind: BuiltinCapabilityKind) -> &'static str {
    match kind {
        BuiltinCapabilityKind::RustCargo => "rust_cargo",
        BuiltinCapabilityKind::NodePackageScripts => "node_package_scripts",
        BuiltinCapabilityKind::GoMod => "go_mod",
        BuiltinCapabilityKind::PythonPyprojectPytest => "python_pyproject_pytest",
        BuiltinCapabilityKind::Swiftpm => "swiftpm",
    }
}

fn prompt_requests_greenfield_scaffold(
    kind: BuiltinCapabilityKind,
    prompt_tokens: &[String],
) -> bool {
    match kind {
        BuiltinCapabilityKind::RustCargo => {
            super::prompt_explicitly_requests_greenfield_rust_scaffold(prompt_tokens)
        }
        BuiltinCapabilityKind::NodePackageScripts => {
            super::prompt_explicitly_requests_greenfield_node_scaffold(prompt_tokens)
        }
        BuiltinCapabilityKind::GoMod => {
            super::prompt_explicitly_requests_greenfield_go_scaffold(prompt_tokens)
        }
        BuiltinCapabilityKind::PythonPyprojectPytest => {
            super::prompt_explicitly_requests_greenfield_python_scaffold(prompt_tokens)
        }
        BuiltinCapabilityKind::Swiftpm => false,
    }
}

fn marker_path_scopes(markers: &[String]) -> Vec<String> {
    let mut scopes = markers
        .iter()
        .map(|marker| {
            Path::new(marker)
                .parent()
                .map(|path| path.to_string_lossy().to_string())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| ".".to_string())
        })
        .collect::<Vec<_>>();
    super::dedupe(&mut scopes);
    scopes
}

fn default_scope_seeds_for_spec(
    spec: &BuiltinCapabilitySpec,
    prompt_tokens: &[String],
) -> CapabilityScopeSeeds {
    match spec.kind {
        BuiltinCapabilityKind::RustCargo => {
            super::greenfield_scaffold_seed(&super::GreenfieldScaffoldKind::Rust, prompt_tokens)
                .into()
        }
        BuiltinCapabilityKind::NodePackageScripts => {
            super::greenfield_scaffold_seed(&super::GreenfieldScaffoldKind::Node, prompt_tokens)
                .into()
        }
        BuiltinCapabilityKind::GoMod => {
            super::greenfield_scaffold_seed(&super::GreenfieldScaffoldKind::Go, prompt_tokens)
                .into()
        }
        BuiltinCapabilityKind::PythonPyprojectPytest => {
            super::greenfield_scaffold_seed(&super::GreenfieldScaffoldKind::Python, prompt_tokens)
                .into()
        }
        BuiltinCapabilityKind::Swiftpm => CapabilityScopeSeeds {
            entry_points: vec!["Package.swift".to_string()],
            file_paths: vec!["Package.swift".to_string()],
            directory_paths: spec
                .default_directories
                .iter()
                .map(|item| item.to_string())
                .collect(),
        },
    }
}

fn scope_seeds_from_markers(
    repo_root: &Path,
    spec: &BuiltinCapabilitySpec,
    markers: &[String],
) -> CapabilityScopeSeeds {
    let mut file_paths = markers.to_vec();
    let mut directory_paths = Vec::new();
    for marker in markers {
        let marker_path = PathBuf::from(marker);
        let base = marker_path.parent().unwrap_or_else(|| Path::new(""));
        for directory in spec.default_directories {
            let candidate = if base.as_os_str().is_empty() {
                PathBuf::from(directory)
            } else {
                base.join(directory)
            };
            if repo_root.join(&candidate).exists() {
                directory_paths.push(candidate.to_string_lossy().to_string());
            }
        }
        for file in spec.extra_file_hints {
            let candidate = if base.as_os_str().is_empty() {
                PathBuf::from(file)
            } else {
                base.join(file)
            };
            if repo_root.join(&candidate).exists() {
                file_paths.push(candidate.to_string_lossy().to_string());
            }
        }
    }
    super::dedupe(&mut file_paths);
    super::dedupe(&mut directory_paths);
    CapabilityScopeSeeds {
        entry_points: markers.to_vec(),
        file_paths,
        directory_paths,
    }
}

fn target_checks_for_spec_with_scripts(
    repo_root: &Path,
    spec: &BuiltinCapabilitySpec,
    prompt_tokens: &[String],
    matched_markers: &[String],
    package_manager: Option<&str>,
    available_scripts: &BTreeMap<String, String>,
) -> Vec<String> {
    if matched_markers
        .iter()
        .any(|marker| marker.starts_with("prompt:greenfield:"))
    {
        return match spec.kind {
            BuiltinCapabilityKind::RustCargo => {
                vec![super::greenfield_bootstrap_check(
                    &super::GreenfieldScaffoldKind::Rust,
                    prompt_tokens,
                )]
            }
            BuiltinCapabilityKind::NodePackageScripts => {
                vec![super::greenfield_bootstrap_check(
                    &super::GreenfieldScaffoldKind::Node,
                    prompt_tokens,
                )]
            }
            BuiltinCapabilityKind::GoMod => {
                vec![super::greenfield_bootstrap_check(
                    &super::GreenfieldScaffoldKind::Go,
                    prompt_tokens,
                )]
            }
            BuiltinCapabilityKind::PythonPyprojectPytest => {
                vec![super::greenfield_bootstrap_check(
                    &super::GreenfieldScaffoldKind::Python,
                    prompt_tokens,
                )]
            }
            BuiltinCapabilityKind::Swiftpm => Vec::new(),
        };
    }

    match spec.kind {
        BuiltinCapabilityKind::RustCargo => {
            if repo_root.join("Cargo.toml").exists() {
                super::rust_target_checks(repo_root, prompt_tokens)
            } else {
                nested_manifest_checks("cargo test --manifest-path", matched_markers)
            }
        }
        BuiltinCapabilityKind::NodePackageScripts => {
            let (target, _) = node_checks_for_markers(
                repo_root,
                matched_markers,
                package_manager,
                available_scripts,
                prompt_tokens,
            );
            target
        }
        BuiltinCapabilityKind::GoMod => {
            if repo_root.join("go.mod").exists() {
                super::go_target_checks()
            } else if matched_markers.is_empty() {
                Vec::new()
            } else {
                matched_markers
                    .iter()
                    .map(|marker| {
                        let dir = Path::new(marker)
                            .parent()
                            .map(|path| path.to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
                        format!("cd {dir} && go test ./...")
                    })
                    .collect()
            }
        }
        BuiltinCapabilityKind::PythonPyprojectPytest => {
            if super::python_manifest(repo_root).is_some() {
                vec!["pytest".to_string()]
            } else {
                matched_markers
                    .iter()
                    .map(|marker| {
                        let dir = Path::new(marker)
                            .parent()
                            .map(|path| path.to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
                        if dir == "." {
                            "pytest".to_string()
                        } else {
                            format!("cd {dir} && pytest")
                        }
                    })
                    .collect()
            }
        }
        BuiltinCapabilityKind::Swiftpm => {
            if repo_root.join("Package.swift").exists() {
                vec!["swift test".to_string()]
            } else {
                matched_markers
                    .iter()
                    .map(|marker| {
                        let dir = Path::new(marker)
                            .parent()
                            .map(|path| path.to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
                        if dir == "." {
                            "swift test".to_string()
                        } else {
                            format!("cd {dir} && swift test")
                        }
                    })
                    .collect()
            }
        }
    }
}

fn integrity_checks_for_spec_with_scripts(
    repo_root: &Path,
    spec: &BuiltinCapabilitySpec,
    prompt_tokens: &[String],
    matched_markers: &[String],
    package_manager: Option<&str>,
    available_scripts: &BTreeMap<String, String>,
) -> Vec<String> {
    if matched_markers
        .iter()
        .any(|marker| marker.starts_with("prompt:greenfield:"))
    {
        return target_checks_for_spec_with_scripts(
            repo_root,
            spec,
            prompt_tokens,
            matched_markers,
            package_manager,
            available_scripts,
        );
    }

    match spec.kind {
        BuiltinCapabilityKind::RustCargo => {
            if repo_root.join("Cargo.toml").exists() {
                super::rust_integrity_checks(repo_root).unwrap_or_default()
            } else {
                nested_manifest_checks("cargo test --manifest-path", matched_markers)
            }
        }
        BuiltinCapabilityKind::NodePackageScripts => {
            let (_, integrity) = node_checks_for_markers(
                repo_root,
                matched_markers,
                package_manager,
                available_scripts,
                prompt_tokens,
            );
            integrity
        }
        BuiltinCapabilityKind::GoMod => {
            if repo_root.join("go.mod").exists() {
                super::go_integrity_checks()
            } else {
                target_checks_for_spec_with_scripts(
                    repo_root,
                    spec,
                    prompt_tokens,
                    matched_markers,
                    package_manager,
                    available_scripts,
                )
            }
        }
        BuiltinCapabilityKind::PythonPyprojectPytest => {
            if super::python_manifest(repo_root).is_some() {
                let mut target = Vec::new();
                let mut integrity = Vec::new();
                super::python_checks(&mut target, &mut integrity);
                integrity
            } else {
                target_checks_for_spec_with_scripts(
                    repo_root,
                    spec,
                    prompt_tokens,
                    matched_markers,
                    package_manager,
                    available_scripts,
                )
            }
        }
        BuiltinCapabilityKind::Swiftpm => target_checks_for_spec_with_scripts(
            repo_root,
            spec,
            prompt_tokens,
            matched_markers,
            package_manager,
            available_scripts,
        ),
    }
}

fn node_checks_for_markers(
    repo_root: &Path,
    matched_markers: &[String],
    package_manager: Option<&str>,
    available_scripts: &BTreeMap<String, String>,
    prompt_tokens: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut markers = matched_markers
        .iter()
        .filter(|marker| marker.ends_with("package.json") && !marker.starts_with("prompt:"))
        .cloned()
        .collect::<Vec<_>>();
    if markers.iter().any(|marker| marker.contains('/')) {
        markers.retain(|marker| marker.contains('/'));
    }
    if markers.is_empty() && repo_root.join("package.json").exists() {
        let mut target = Vec::new();
        let mut integrity = Vec::new();
        super::node_checks(
            package_manager,
            available_scripts,
            prompt_tokens,
            &mut target,
            &mut integrity,
        );
        return (target, integrity);
    }

    let mut target = Vec::new();
    let mut integrity = Vec::new();
    for marker in markers {
        let package_json_path = repo_root.join(&marker);
        let scripts = match read_package_scripts(&package_json_path) {
            Ok(scripts) => scripts,
            Err(_) => continue,
        };
        if !scripts.contains_key("check") {
            continue;
        }
        let package_dir = Path::new(&marker)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let package_manager = detect_package_manager_for_dir(&repo_root.join(&package_dir));
        push_unique_check(
            &mut integrity,
            package_manager_run_for_dir(package_manager.as_deref(), &package_dir, "check"),
        );
        if scripts.contains_key("build:web") {
            push_unique_check(
                &mut target,
                package_manager_run_for_dir(package_manager.as_deref(), &package_dir, "build:web"),
            );
        } else if scripts.contains_key("build") {
            push_unique_check(
                &mut target,
                package_manager_run_for_dir(package_manager.as_deref(), &package_dir, "build"),
            );
        } else {
            push_unique_check(
                &mut target,
                package_manager_run_for_dir(package_manager.as_deref(), &package_dir, "check"),
            );
        }
    }

    if target.is_empty() && integrity.is_empty() {
        let mut fallback_target = Vec::new();
        let mut fallback_integrity = Vec::new();
        super::node_checks(
            package_manager,
            available_scripts,
            prompt_tokens,
            &mut fallback_target,
            &mut fallback_integrity,
        );
        return (fallback_target, fallback_integrity);
    }

    (target, integrity)
}

fn detect_package_manager_for_dir(package_dir: &Path) -> Option<String> {
    for (file, pm) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
        ("package-lock.json", "npm"),
    ] {
        if package_dir.join(file).exists() {
            return Some(pm.to_string());
        }
    }
    Some("npm".to_string())
}

fn package_manager_run_for_dir(
    package_manager: Option<&str>,
    relative_dir: &Path,
    script: &str,
) -> String {
    if relative_dir.as_os_str().is_empty() {
        return match package_manager.unwrap_or("npm") {
            "pnpm" => format!("pnpm {script}"),
            "yarn" => format!("yarn {script}"),
            "bun" => format!("bun run {script}"),
            _ => format!("npm run {script}"),
        };
    }
    let directory = relative_dir.to_string_lossy();
    match package_manager.unwrap_or("npm") {
        "pnpm" => format!("pnpm --dir {directory} {script}"),
        "yarn" => format!("yarn --cwd {directory} {script}"),
        "bun" => format!("bun --cwd {directory} run {script}"),
        _ => format!("npm --prefix {directory} run {script}"),
    }
}

fn push_unique_check(checks: &mut Vec<String>, candidate: String) {
    if !checks.iter().any(|existing| existing == &candidate) {
        checks.push(candidate);
    }
}

fn target_checks_for_spec(
    repo_root: &Path,
    spec: &BuiltinCapabilitySpec,
    prompt: &str,
    matched_markers: &[String],
) -> Vec<String> {
    target_checks_for_spec_with_scripts(
        repo_root,
        spec,
        &super::prompt_tokens(prompt),
        matched_markers,
        super::detect_package_manager(repo_root).as_deref(),
        &if repo_root.join("package.json").exists() {
            read_package_scripts(&repo_root.join("package.json")).unwrap_or_default()
        } else {
            BTreeMap::new()
        },
    )
}

fn integrity_checks_for_spec(
    repo_root: &Path,
    spec: &BuiltinCapabilitySpec,
    prompt: &str,
    matched_markers: &[String],
) -> Vec<String> {
    integrity_checks_for_spec_with_scripts(
        repo_root,
        spec,
        &super::prompt_tokens(prompt),
        matched_markers,
        super::detect_package_manager(repo_root).as_deref(),
        &if repo_root.join("package.json").exists() {
            read_package_scripts(&repo_root.join("package.json")).unwrap_or_default()
        } else {
            BTreeMap::new()
        },
    )
}

fn nested_manifest_checks(prefix: &str, matched_markers: &[String]) -> Vec<String> {
    let mut checks = matched_markers
        .iter()
        .filter(|marker| !marker.starts_with("prompt:"))
        .map(|marker| format!("{prefix} {marker}"))
        .collect::<Vec<_>>();
    super::dedupe(&mut checks);
    checks
}

fn capability_matches_contract_path(view: &CapabilityCandidateView, path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    view.matched_markers.iter().any(|marker| {
        marker == normalized
            || normalized.starts_with(&format!("{marker}/"))
            || marker.starts_with(&format!("{normalized}/"))
    }) || view.path_scopes.iter().any(|scope| {
        scope == "."
            || scope == normalized
            || normalized.starts_with(&format!("{scope}/"))
            || scope.starts_with(&format!("{normalized}/"))
    })
}

fn contract_implies_controller_scaffold(
    contract: &punk_domain::Contract,
    spec: &BuiltinCapabilitySpec,
) -> bool {
    match spec.kind {
        BuiltinCapabilityKind::RustCargo => {
            contract.entry_points == vec!["Cargo.toml".to_string()]
                && contract
                    .target_checks
                    .iter()
                    .chain(contract.integrity_checks.iter())
                    .any(|check| check.trim().starts_with("cargo test"))
        }
        BuiltinCapabilityKind::GoMod => {
            contract.entry_points == vec!["go.mod".to_string()]
                && contract
                    .target_checks
                    .iter()
                    .chain(contract.integrity_checks.iter())
                    .any(|check| check.trim() == "go test ./...")
        }
        BuiltinCapabilityKind::PythonPyprojectPytest => {
            contract.entry_points == vec!["pyproject.toml".to_string()]
                && contract
                    .target_checks
                    .iter()
                    .chain(contract.integrity_checks.iter())
                    .any(|check| check.trim() == "pytest")
        }
        _ => false,
    }
}

fn contract_matches_scaffold_kind(
    contract: &punk_domain::Contract,
    spec: &BuiltinCapabilitySpec,
    scaffold_kind: &str,
) -> bool {
    spec.controller_scaffold_kind == Some(scaffold_kind)
        && contract_implies_controller_scaffold(contract, spec)
}

impl From<super::GreenfieldScaffoldSeed> for CapabilityScopeSeeds {
    fn from(value: super::GreenfieldScaffoldSeed) -> Self {
        Self {
            entry_points: value.entry_points,
            file_paths: value.file_scope_paths,
            directory_paths: value.directory_scope_paths,
        }
    }
}
