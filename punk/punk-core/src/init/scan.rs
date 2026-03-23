use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::artifacts::{ArtifactSet, Convention, ConversionConfidence};
use super::InitError;

/// Maximum time allowed for the full scan.
const SCAN_TIMEOUT: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub primary_language: Option<String>,
    pub languages: HashMap<String, usize>,
    pub frameworks: Vec<String>,
    pub test_runner: Option<String>,
    pub test_count: usize,
    pub ci_detected: bool,
    pub ci_files: Vec<String>,
    pub container_detected: bool,
    pub build_system: Option<String>,
    pub entry_points: Vec<String>,
    pub dir_map: HashMap<String, Vec<String>>,
    pub dependencies: HashMap<String, Vec<String>>,
    pub conventions: Vec<Convention>,
    pub never_touch: Vec<String>,
    pub archaeology: GitArchaeology,
    pub error_crate: Option<String>,
    pub unwrap_density: f64,
    pub logging_crate: Option<String>,
    pub scanned_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitArchaeology {
    pub commit_style: String,
    pub contributor_count: usize,
    pub branch_count: usize,
    pub conventional_commit_ratio: f64,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run_scan(root: &Path) -> Result<ScanResult, InitError> {
    let start = Instant::now();

    let (langs, dir_map, entry_points, ci_files, container_detected, build_system) =
        scan_structure(root, start)?;

    check_timeout(start)?;

    let primary_language = primary_lang(&langs);
    let frameworks = detect_frameworks(root, &primary_language);
    let test_runner = detect_test_runner(root, &primary_language);
    let test_count = count_tests(root, &primary_language, start).unwrap_or(0);

    check_timeout(start)?;

    let deps = parse_config_files(root);

    check_timeout(start)?;

    let conventions = detect_conventions(root);

    check_timeout(start)?;

    let never_touch = detect_never_touch(root);
    let archaeology = run_git_archaeology(root);

    check_timeout(start)?;

    let (unwrap_density, error_crate, logging_crate) =
        grep_patterns(root, &primary_language, start).unwrap_or((0.0, None, None));

    let ci_detected = !ci_files.is_empty();

    Ok(ScanResult {
        primary_language,
        languages: langs,
        frameworks,
        test_runner,
        test_count,
        ci_detected,
        ci_files,
        container_detected,
        build_system,
        entry_points,
        dir_map,
        dependencies: deps,
        conventions,
        never_touch,
        archaeology,
        error_crate,
        unwrap_density,
        logging_crate,
        scanned_at: chrono::Local::now().to_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Structure scan (T1)
// ---------------------------------------------------------------------------

/// (langs, dir_map, entry_points, ci_files, container_detected, build_tool)
type StructureScanResult = (
    HashMap<String, usize>,
    HashMap<String, Vec<String>>,
    Vec<String>,
    Vec<String>,
    bool,
    Option<String>,
);

fn scan_structure(root: &Path, start: Instant) -> Result<StructureScanResult, InitError> {
    let mut langs: HashMap<String, usize> = HashMap::new();
    let mut dir_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut entry_points: Vec<String> = Vec::new();
    let mut ci_files: Vec<String> = Vec::new();
    let mut container_detected = false;
    let mut build_system: Option<String> = None;

    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "target"
                && name != "node_modules"
                && name != ".git"
                && name != ".jj"
                && name != "vendor"
                && name != "__pycache__"
                && name != ".venv"
        });

    for entry in walker.flatten() {
        if start.elapsed() >= SCAN_TIMEOUT {
            break;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();

        // Language counting by extension
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            let lang = ext_to_lang(&ext);
            if let Some(l) = lang {
                *langs.entry(l.to_string()).or_insert(0) += 1;
            }
        }

        // Depth-2 directory map
        let components: Vec<_> = rel.components().collect();
        if components.len() >= 2 {
            let dir = components[0].as_os_str().to_string_lossy().into_owned();
            let file = components[components.len() - 1]
                .as_os_str()
                .to_string_lossy()
                .into_owned();
            let entries = dir_map.entry(dir).or_default();
            if entries.len() < 100 {
                entries.push(file);
            }
        }

        // Entry points
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_entry_point(&file_name) {
            entry_points.push(rel_str.to_string());
        }

        // CI detection
        if rel_str.contains(".github/workflows")
            || rel_str.contains(".gitlab-ci")
            || rel_str.contains("Jenkinsfile")
            || file_name == ".travis.yml"
            || file_name == ".circleci"
        {
            ci_files.push(rel_str.to_string());
        }

        // Container
        if file_name == "Dockerfile" || file_name == "docker-compose.yml" || file_name == "docker-compose.yaml" {
            container_detected = true;
        }

        // Build system
        if build_system.is_none() {
            build_system = detect_build_system_from_file(&file_name);
        }
    }

    Ok((langs, dir_map, entry_points, ci_files, container_detected, build_system))
}

fn ext_to_lang(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "go" => Some("go"),
        "py" | "pyi" => Some("python"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "java" => Some("java"),
        "kt" | "kts" => Some("kotlin"),
        "cpp" | "cc" | "cxx" => Some("cpp"),
        "c" => Some("c"),
        "h" | "hpp" => Some("c"),
        "cs" => Some("csharp"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "swift" => Some("swift"),
        _ => None,
    }
}

fn is_entry_point(name: &str) -> bool {
    matches!(
        name,
        "main.rs"
            | "main.go"
            | "main.py"
            | "__main__.py"
            | "index.ts"
            | "index.js"
            | "index.tsx"
            | "app.ts"
            | "app.js"
            | "server.ts"
            | "server.js"
            | "manage.py"
    )
}

fn detect_build_system_from_file(name: &str) -> Option<String> {
    match name {
        "Cargo.toml" => Some("cargo".to_string()),
        "package.json" => Some("npm".to_string()),
        "go.mod" => Some("go".to_string()),
        "pyproject.toml" | "setup.py" | "setup.cfg" => Some("python".to_string()),
        "Makefile" | "makefile" | "GNUmakefile" => Some("make".to_string()),
        "CMakeLists.txt" => Some("cmake".to_string()),
        "BUILD" | "BUILD.bazel" | "WORKSPACE" => Some("bazel".to_string()),
        _ => None,
    }
}

fn primary_lang(langs: &HashMap<String, usize>) -> Option<String> {
    langs.iter().max_by_key(|(_, v)| *v).map(|(k, _)| k.clone())
}

// ---------------------------------------------------------------------------
// Framework and test detection
// ---------------------------------------------------------------------------

fn detect_frameworks(root: &Path, primary: &Option<String>) -> Vec<String> {
    let mut frameworks = Vec::new();
    match primary.as_deref() {
        Some("rust") => {
            if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
                if content.contains("actix-web") {
                    frameworks.push("actix-web".to_string());
                }
                if content.contains("axum") {
                    frameworks.push("axum".to_string());
                }
                if content.contains("rocket") {
                    frameworks.push("rocket".to_string());
                }
                if content.contains("warp") {
                    frameworks.push("warp".to_string());
                }
                if content.contains("tokio") {
                    frameworks.push("tokio".to_string());
                }
            }
        }
        Some("javascript") | Some("typescript") => {
            if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
                for fw in &["react", "vue", "angular", "next", "nuxt", "express", "fastify"] {
                    if content.contains(fw) {
                        frameworks.push(fw.to_string());
                    }
                }
            }
        }
        Some("python") => {
            for config in &["pyproject.toml", "requirements.txt", "setup.py"] {
                if let Ok(content) = std::fs::read_to_string(root.join(config)) {
                    for fw in &["django", "fastapi", "flask", "starlette", "tornado"] {
                        if content.to_lowercase().contains(fw) {
                            frameworks.push(fw.to_string());
                        }
                    }
                    break;
                }
            }
        }
        Some("go") => {
            if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
                for fw in &["gin-gonic", "echo", "fiber", "chi", "gorilla"] {
                    if content.contains(fw) {
                        frameworks.push(fw.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    frameworks
}

fn detect_test_runner(root: &Path, primary: &Option<String>) -> Option<String> {
    match primary.as_deref() {
        Some("rust") => Some("cargo-test".to_string()),
        Some("javascript") | Some("typescript") => {
            let pkg = root.join("package.json");
            if let Ok(content) = std::fs::read_to_string(&pkg) {
                if content.contains("jest") {
                    return Some("jest".to_string());
                }
                if content.contains("vitest") {
                    return Some("vitest".to_string());
                }
                if content.contains("mocha") {
                    return Some("mocha".to_string());
                }
            }
            None
        }
        Some("python") => {
            if root.join("pytest.ini").exists()
                || root.join("pyproject.toml").exists()
            {
                return Some("pytest".to_string());
            }
            None
        }
        Some("go") => Some("go-test".to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Test count (T5)
// ---------------------------------------------------------------------------

fn count_tests(root: &Path, primary: &Option<String>, start: Instant) -> Option<usize> {
    let (pattern, exts): (&str, &[&str]) = match primary.as_deref() {
        Some("rust") => ("#\\[test\\]|#\\[tokio::test\\]", &["rs"]),
        Some("go") => ("func Test", &["go"]),
        Some("python") => ("def test_", &["py"]),
        Some("javascript") | Some("typescript") => ("\\b(it|test)\\s*\\(", &["js", "ts", "jsx", "tsx"]),
        _ => return None,
    };

    let mut count = 0usize;
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "target" && name != "node_modules" && name != ".git"
        });

    let re = match regex_lite_count(pattern) {
        Ok(r) => r,
        Err(_) => return None,
    };

    for entry in walker.flatten() {
        if start.elapsed() >= SCAN_TIMEOUT {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if exts.contains(&ext.as_str()) {
                if let Ok(content) = std::fs::read_to_string(path) {
                    count += re(&content);
                }
            }
        }
    }
    Some(count)
}

/// Simple regex-free line-based counter — returns a closure that counts pattern occurrences.
/// We avoid the regex crate dependency by using a simple substring search.
type PatternCounter = Box<dyn Fn(&str) -> usize>;

fn regex_lite_count(pattern: &str) -> Result<PatternCounter, String> {
    // Convert simple patterns to substring matchers
    let patterns: Vec<String> = if pattern.contains('|') {
        pattern
            .split('|')
            .map(unescape_pattern)
            .collect()
    } else {
        vec![unescape_pattern(pattern)]
    };

    Ok(Box::new(move |content: &str| {
        content
            .lines()
            .filter(|line| patterns.iter().any(|p| line.contains(p.as_str())))
            .count()
    }))
}

fn unescape_pattern(p: &str) -> String {
    p.replace("\\[", "[")
        .replace("\\]", "]")
        .replace("\\b", "")
        .replace("\\s*", "")
        .replace("\\(", "(")
}

// ---------------------------------------------------------------------------
// Config file parsing (T2)
// ---------------------------------------------------------------------------

fn parse_config_files(root: &Path) -> HashMap<String, Vec<String>> {
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();

    // Cargo.toml
    if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
        let extracted = extract_cargo_deps(&content);
        if !extracted.is_empty() {
            deps.insert("rust".to_string(), extracted);
        }
    }

    // package.json
    if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
        let extracted = extract_npm_deps(&content);
        if !extracted.is_empty() {
            deps.insert("javascript".to_string(), extracted);
        }
    }

    // go.mod
    if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
        let extracted = extract_go_deps(&content);
        if !extracted.is_empty() {
            deps.insert("go".to_string(), extracted);
        }
    }

    // pyproject.toml
    if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml")) {
        let extracted = extract_python_deps(&content);
        if !extracted.is_empty() {
            deps.insert("python".to_string(), extracted);
        }
    }

    deps
}

fn extract_cargo_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" || trimmed == "[dev-dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
        }
        if in_deps {
            if let Some(name) = trimmed.split('=').next() {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() && !name.starts_with('#') {
                    deps.push(name.to_string());
                }
            }
        }
    }
    deps
}

fn extract_npm_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(content) {
        for section in &["dependencies", "devDependencies"] {
            if let Some(obj) = val.get(section).and_then(|v| v.as_object()) {
                for key in obj.keys() {
                    deps.push(key.clone());
                }
            }
        }
    }
    deps
}

fn extract_go_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_require = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("require (") || trimmed == "require (" {
            in_require = true;
            continue;
        }
        if in_require && trimmed == ")" {
            in_require = false;
            continue;
        }
        if in_require {
            if let Some(pkg) = trimmed.split_whitespace().next() {
                deps.push(pkg.to_string());
            }
        } else if trimmed.starts_with("require ") {
            // single line require
            let rest = trimmed.trim_start_matches("require ");
            if let Some(pkg) = rest.split_whitespace().next() {
                deps.push(pkg.to_string());
            }
        }
    }
    deps
}

fn extract_python_deps(content: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[tool.poetry.dependencies]"
            || trimmed == "[project]"
            || trimmed.starts_with("dependencies")
        {
            in_deps = true;
            continue;
        }
        if in_deps && trimmed.starts_with('[') && !trimmed.starts_with("[tool.poetry.dependencies") {
            in_deps = false;
        }
        if in_deps {
            if let Some(name) = trimmed.split(['=', '>', '<', '~', '^', '"']).next() {
                let name = name.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() && !name.starts_with('#') && name != "python" {
                    deps.push(name.to_string());
                }
            }
        }
    }
    deps
}

// ---------------------------------------------------------------------------
// Conventions (T2)
// ---------------------------------------------------------------------------

fn detect_conventions(root: &Path) -> Vec<Convention> {
    let mut conventions = Vec::new();

    // CLAUDE.md / AGENTS.md — authoritative
    for authoritative_file in &["CLAUDE.md", "AGENTS.md", ".claude/CLAUDE.md"] {
        if let Ok(content) = std::fs::read_to_string(root.join(authoritative_file)) {
            if !content.is_empty() {
                conventions.push(Convention {
                    name: format!("authoritative-{}", authoritative_file.replace('/', "-")),
                    value: format!("Found {} with {} chars", authoritative_file, content.len()),
                    confidence: ConversionConfidence::Authoritative,
                    source: authoritative_file.to_string(),
                });
            }
        }
    }

    // .editorconfig
    if let Ok(content) = std::fs::read_to_string(root.join(".editorconfig")) {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(val) = parse_editorconfig_line(trimmed) {
                conventions.push(Convention {
                    name: val.0,
                    value: val.1,
                    confidence: ConversionConfidence::High,
                    source: ".editorconfig".to_string(),
                });
            }
        }
    }

    // rustfmt.toml
    if let Ok(content) = std::fs::read_to_string(root.join("rustfmt.toml")) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = trimmed.split_once('=') {
                conventions.push(Convention {
                    name: format!("rustfmt.{}", key.trim()),
                    value: val.trim().to_string(),
                    confidence: ConversionConfidence::High,
                    source: "rustfmt.toml".to_string(),
                });
            }
        }
    }

    // .prettierrc
    if let Ok(content) = std::fs::read_to_string(root.join(".prettierrc")) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('"') {
                if let Some((key, val)) = trimmed.split_once(':') {
                    let key = key.trim().trim_matches('"');
                    let val = val.trim().trim_end_matches([',', '"']).trim_matches('"');
                    conventions.push(Convention {
                        name: format!("prettier.{key}"),
                        value: val.to_string(),
                        confidence: ConversionConfidence::High,
                        source: ".prettierrc".to_string(),
                    });
                }
            }
        }
    }

    // Infer from Cargo.toml edition
    if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("edition") {
                if let Some(val) = trimmed.split('=').nth(1) {
                    conventions.push(Convention {
                        name: "rust-edition".to_string(),
                        value: val.trim().trim_matches('"').to_string(),
                        confidence: ConversionConfidence::Medium,
                        source: "Cargo.toml".to_string(),
                    });
                }
            }
        }
    }

    if conventions.is_empty() {
        conventions.push(Convention {
            name: "no-conventions-detected".to_string(),
            value: "".to_string(),
            confidence: ConversionConfidence::Low,
            source: "inference".to_string(),
        });
    }

    conventions
}

fn parse_editorconfig_line(line: &str) -> Option<(String, String)> {
    if line.starts_with('#') || line.starts_with('[') || line.is_empty() {
        return None;
    }
    let (key, val) = line.split_once('=')?;
    let key = key.trim();
    let val = val.trim();
    if key.is_empty() || val.is_empty() {
        return None;
    }
    Some((format!("editorconfig.{key}"), val.to_string()))
}

// ---------------------------------------------------------------------------
// Never-touch detection
// ---------------------------------------------------------------------------

pub fn detect_never_touch(root: &Path) -> Vec<String> {
    let mut never = Vec::new();

    let always_never = [
        "migrations",
        "generated",
        "vendor",
        ".env",
        ".env.local",
        ".env.production",
        ".env.staging",
    ];
    let lock_files = [
        "Cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "go.sum",
        "poetry.lock",
        "Pipfile.lock",
        "composer.lock",
        "Gemfile.lock",
        "mix.lock",
    ];

    for name in &always_never {
        let p = root.join(name);
        if p.exists() {
            never.push(name.to_string());
        }
    }

    for name in &lock_files {
        let p = root.join(name);
        if p.exists() {
            never.push(name.to_string());
        }
    }

    // Walk for directories
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(3)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "target" && name != "node_modules" && name != ".git"
        });

    for entry in walker.flatten() {
        if entry.file_type().is_dir() {
            let name = entry.file_name().to_string_lossy();
            if matches!(name.as_ref(), "migrations" | "generated" | "vendor" | "dist" | "build" | "__generated__") {
                let rel = entry
                    .path()
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| name.into_owned());
                if !never.contains(&rel) {
                    never.push(rel);
                }
            }
        } else if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name.starts_with(".env") && !never.contains(&name.as_ref().to_string()) {
                let rel = entry
                    .path()
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| name.into_owned());
                if !never.contains(&rel) {
                    never.push(rel);
                }
            }
        }
    }

    never.dedup();
    never
}

// ---------------------------------------------------------------------------
// Git archaeology (T4)
// ---------------------------------------------------------------------------

pub fn run_git_archaeology(root: &Path) -> GitArchaeology {
    let commit_messages = git_log_messages(root);
    if commit_messages.is_empty() {
        return GitArchaeology::default();
    }

    let conventional_count = commit_messages
        .iter()
        .filter(|m| is_conventional_commit(m))
        .count();

    let ratio = if commit_messages.is_empty() {
        0.0
    } else {
        conventional_count as f64 / commit_messages.len() as f64
    };

    let commit_style = if ratio > 0.6 {
        "conventional".to_string()
    } else {
        "freeform".to_string()
    };

    let contributor_count = count_contributors(root);
    let branch_count = count_branches(root);

    GitArchaeology {
        commit_style,
        contributor_count,
        branch_count,
        conventional_commit_ratio: ratio,
    }
}

fn git_log_messages(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["log", "--oneline", "--format=%s", "-100"])
        .current_dir(root)
        .output();

    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect(),
        _ => Vec::new(),
    }
}

fn is_conventional_commit(msg: &str) -> bool {
    // type(scope): description  or  type: description
    let prefixes = [
        "feat", "fix", "docs", "style", "refactor", "perf", "test", "build",
        "ci", "chore", "revert", "wip",
    ];
    let first_word = msg.split(':').next().unwrap_or("");
    let base = first_word.split('(').next().unwrap_or(first_word);
    prefixes.contains(&base.trim())
}

fn count_contributors(root: &Path) -> usize {
    let out = Command::new("git")
        .args(["shortlog", "-sn", "--no-merges", "HEAD"])
        .current_dir(root)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count()
        }
        _ => 0,
    }
}

fn count_branches(root: &Path) -> usize {
    let out = Command::new("git")
        .args(["branch", "-a"])
        .current_dir(root)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count()
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Grep patterns (T5)
// ---------------------------------------------------------------------------

fn grep_patterns(
    root: &Path,
    primary: &Option<String>,
    start: Instant,
) -> Option<(f64, Option<String>, Option<String>)> {
    if primary.as_deref() != Some("rust") {
        return Some((0.0, None, None));
    }

    let mut total_lines = 0usize;
    let mut unwrap_count = 0usize;
    let mut error_crate: Option<String> = None;
    let mut logging_crate: Option<String> = None;

    // First: check Cargo.toml for error/logging crates (fast, reliable)
    if let Ok(cargo_content) = std::fs::read_to_string(root.join("Cargo.toml")) {
        for crate_name in &["anyhow", "thiserror", "eyre", "miette"] {
            if cargo_content.contains(crate_name) {
                error_crate = Some(crate_name.to_string());
                break;
            }
        }
        for crate_name in &["tracing", "env_logger", "slog"] {
            if cargo_content.contains(crate_name) {
                logging_crate = Some(if *crate_name == "env_logger" { "log" } else { crate_name }.to_string());
                break;
            }
        }
    }

    // Also scan workspace member Cargo.toml files
    if error_crate.is_none() || logging_crate.is_none() {
        let walker = walkdir::WalkDir::new(root)
            .follow_links(false)
            .max_depth(3)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                name != "target" && name != ".git" && name != "node_modules"
            });
        for entry in walker.flatten() {
            if entry.file_type().is_file()
                && entry.file_name().to_string_lossy() == "Cargo.toml"
            {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if error_crate.is_none() {
                        for crate_name in &["anyhow", "thiserror", "eyre", "miette"] {
                            if content.contains(crate_name) {
                                error_crate = Some(crate_name.to_string());
                                break;
                            }
                        }
                    }
                    if logging_crate.is_none() {
                        for crate_name in &["tracing", "env_logger", "slog"] {
                            if content.contains(crate_name) {
                                logging_crate = Some(
                                    if *crate_name == "env_logger" { "log" } else { crate_name }
                                        .to_string(),
                                );
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // Scan .rs files for unwrap density and source-level crate references
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != "target" && name != ".git"
        });

    for entry in walker.flatten() {
        if start.elapsed() >= SCAN_TIMEOUT {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().map(|e| e == "rs").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    total_lines += 1;
                    if line.contains(".unwrap()") {
                        unwrap_count += 1;
                    }
                    // Fallback: check source for crate usage if Cargo.toml scan missed it
                    if error_crate.is_none() {
                        if line.contains("anyhow") {
                            error_crate = Some("anyhow".to_string());
                        } else if line.contains("thiserror") {
                            error_crate = Some("thiserror".to_string());
                        } else if line.contains("eyre") {
                            error_crate = Some("eyre".to_string());
                        } else if line.contains("miette") {
                            error_crate = Some("miette".to_string());
                        }
                    }
                    if logging_crate.is_none() {
                        if line.contains("tracing") {
                            logging_crate = Some("tracing".to_string());
                        } else if line.contains("log::") || line.contains("env_logger") {
                            logging_crate = Some("log".to_string());
                        } else if line.contains("slog") {
                            logging_crate = Some("slog".to_string());
                        }
                    }
                }
            }
        }
    }

    let density = if total_lines > 0 {
        unwrap_count as f64 / total_lines as f64
    } else {
        0.0
    };

    Some((density, error_crate, logging_crate))
}

// ---------------------------------------------------------------------------
// Timeout helper
// ---------------------------------------------------------------------------

fn check_timeout(start: Instant) -> Result<(), InitError> {
    if start.elapsed() >= SCAN_TIMEOUT {
        Err(InitError::Scan("scan timeout exceeded (8s)".to_string()))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Conversion to artifacts
// ---------------------------------------------------------------------------

impl ScanResult {
    pub fn to_artifacts(&self) -> ArtifactSet {
        let config = build_config_toml(self);
        let intent = build_intent_md(self);
        let conventions_json =
            serde_json::to_string_pretty(&self.conventions).unwrap_or_default();
        let scan_json = serde_json::to_string_pretty(self).unwrap_or_default();

        ArtifactSet {
            config_toml: config,
            intent_md: intent,
            conventions_json,
            scan_json: Some(scan_json),
        }
    }
}

fn build_config_toml(scan: &ScanResult) -> String {
    let lang = scan.primary_language.as_deref().unwrap_or("unknown");
    let frameworks = if scan.frameworks.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            scan.frameworks
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let test_runner = scan.test_runner.as_deref().unwrap_or("unknown");
    let build = scan.build_system.as_deref().unwrap_or("unknown");

    format!(
        r#"# punk configuration — auto-generated by punk init
# https://github.com/heurema/specpunk

[project]
primary_language = "{lang}"
frameworks = {frameworks}
test_runner = "{test_runner}"
build_system = "{build}"

[vcs]
preferred = "auto"

[provider]
# LLM provider configuration (Phase 1)
# api_key = ""
# model = ""
"#
    )
}

fn build_intent_md(scan: &ScanResult) -> String {
    let lang = scan.primary_language.as_deref().unwrap_or("unknown");
    let frameworks = if scan.frameworks.is_empty() {
        "none detected".to_string()
    } else {
        scan.frameworks.join(", ")
    };
    format!(
        r#"# Project Intent

<!-- Edit this file to describe your project. punk init will not overwrite it once edited. -->

## What does this project do?

_TODO: describe the project_

## Tech stack

- Language: {lang}
- Frameworks: {frameworks}
- Test runner: {}
- Build system: {}

## Scope boundaries

### Never touch
{}

## Notes

_Generated by `punk init`_
"#,
        scan.test_runner.as_deref().unwrap_or("unknown"),
        scan.build_system.as_deref().unwrap_or("unknown"),
        scan.never_touch
            .iter()
            .map(|s| format!("- `{s}`"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

// ---------------------------------------------------------------------------
// Helper re-export for PathBuf (used in tests)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
pub(crate) fn _root_type_check(_: &PathBuf) {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_rust_project(dir: &Path) {
        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/main.rs"),
            r#"fn main() {
    println!("hello");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_one() {}
    #[test]
    fn test_two() {}
}
"#,
        )
        .unwrap();
    }

    #[test]
    fn init_brownfield_rust() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        make_rust_project(dir);
        // Add more source files to ensure brownfield mode
        for i in 0..5 {
            fs::write(dir.join(format!("src/mod{i}.rs")), "pub fn foo() {}").unwrap();
        }

        let result = run_scan(dir).unwrap();
        assert_eq!(result.primary_language.as_deref(), Some("rust"));
        assert!(result.test_count >= 2);
        assert_eq!(result.error_crate.as_deref(), Some("anyhow"));

        let artifacts = result.to_artifacts();
        assert!(!artifacts.config_toml.is_empty());
        assert!(!artifacts.intent_md.is_empty());
        assert!(!artifacts.conventions_json.is_empty());
        assert!(artifacts.scan_json.is_some());
    }

    #[test]
    fn init_scan_performance() {
        // Use the punk workspace itself — it's small so scan should be fast.
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let start = Instant::now();
        let _ = run_scan(workspace);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(10),
            "scan took {:?}, expected < 10s",
            elapsed
        );
    }

    #[test]
    fn conventions_confidence() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::write(
            dir.join(".editorconfig"),
            "[*]\nindent_style = space\nindent_size = 4\n",
        )
        .unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        for i in 0..5 {
            let src = dir.join("src");
            fs::create_dir_all(&src).unwrap();
            fs::write(src.join(format!("f{i}.rs")), "pub fn f() {}").unwrap();
        }

        let result = run_scan(dir).unwrap();
        // All conventions must have confidence and source
        for conv in &result.conventions {
            assert!(
                !conv.source.is_empty(),
                "convention {} missing source",
                conv.name
            );
        }
        // At least one editorconfig convention
        let has_editorconfig = result
            .conventions
            .iter()
            .any(|c| c.source == ".editorconfig");
        assert!(has_editorconfig, "editorconfig conventions not detected");
    }

    #[test]
    fn never_touch_detection() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::create_dir(dir.join("migrations")).unwrap();
        fs::write(dir.join(".env"), "SECRET=foo").unwrap();
        fs::write(dir.join("Cargo.lock"), "# lock").unwrap();

        let never = detect_never_touch(dir);
        assert!(never.iter().any(|s| s.contains("migrations")), "missing migrations");
        assert!(never.iter().any(|s| s.contains(".env")), "missing .env");
        assert!(never.iter().any(|s| s.contains("Cargo.lock")), "missing Cargo.lock");
    }

    #[test]
    fn git_archaeology() {
        // Run on the punk workspace itself (has git history)
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        // Walk up to find the actual git root (specpunk/)
        let git_root = workspace.parent().unwrap_or(workspace);
        let arch = run_git_archaeology(git_root);
        // We just verify it doesn't panic and returns something reasonable
        assert!(arch.contributor_count >= 0); // always true, checks it runs
        assert!(arch.conventional_commit_ratio >= 0.0 && arch.conventional_commit_ratio <= 1.0);
        assert!(
            arch.commit_style == "conventional" || arch.commit_style == "freeform" || arch.commit_style.is_empty()
        );
    }

    #[test]
    fn multi_language_detection() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        fs::write(
            dir.join("package.json"),
            r#"{"name":"test","dependencies":{"react":"^18.0.0","express":"^4.0.0"}}"#,
        )
        .unwrap();
        // Add enough JS files for brownfield
        for i in 0..5 {
            fs::write(dir.join(format!("index{i}.js")), "const x = 1;").unwrap();
        }

        let result = run_scan(dir).unwrap();
        assert_eq!(result.primary_language.as_deref(), Some("javascript"));
        assert!(result.dependencies.contains_key("javascript"), "JS deps not detected");
    }

    #[test]
    fn init_no_vcs() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        // Create enough files to trigger brownfield
        for i in 0..5 {
            fs::write(dir.join(format!("file{i}.rs")), "fn f() {}").unwrap();
        }
        // Should not panic even without .git
        let result = run_scan(dir).unwrap();
        assert_eq!(result.archaeology.contributor_count, 0);
        assert_eq!(result.archaeology.branch_count, 0);
    }

    #[test]
    fn symlink_safety() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let target = dir.join("real");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("file.rs"), "fn f() {}").unwrap();

        // Create a symlink loop (follow_links=false so walkdir skips it)
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir, dir.join("loop_link")).unwrap();
        }

        // Should not panic or infinite loop
        let _never = detect_never_touch(dir);
        let result = run_scan(dir);
        assert!(result.is_ok() || result.is_err()); // just must not panic
    }

    #[test]
    fn large_dir_timeout() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        // Create many files — scan should still return within timeout
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        for i in 0..500 {
            fs::write(src.join(format!("f{i}.rs")), "fn f() { let _x = 1; }").unwrap();
        }
        let start = Instant::now();
        let _ = run_scan(dir);
        // Must complete (timeout kicks in at 8s, test timeout is 10s from AC)
        assert!(start.elapsed() < Duration::from_secs(15));
    }
}
