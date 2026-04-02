use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use punk_domain::VcsKind;

pub mod snapshot;
pub use snapshot::current_snapshot_ref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsMode {
    Jj,
    GitOnly,
    GitWithJjAvailableButDisabled,
    NoVcs,
}

#[derive(Debug, Clone)]
pub struct IsolatedChange {
    pub workspace_ref: String,
    pub change_ref: String,
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProvenanceBaseline {
    snapshots: BTreeMap<String, FileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileSnapshot {
    Missing,
    Present { len: u64, modified_ns: Option<u128> },
}

pub trait VcsBackend {
    fn kind(&self) -> VcsKind;
    fn workspace_root(&self) -> Result<PathBuf>;
    fn create_isolated_change(&self, name: &str) -> Result<IsolatedChange>;
    fn current_change_ref(&self) -> Result<String>;
    fn changed_files(&self) -> Result<Vec<String>>;
    fn diff(&self) -> Result<String>;
    fn capture_provenance_baseline(&self) -> Result<ProvenanceBaseline> {
        let root = self.workspace_root()?;
        let changed_files = self.changed_files()?;
        ProvenanceBaseline::capture(&root, &changed_files)
    }
    fn changed_files_since(&self, baseline: &ProvenanceBaseline) -> Result<Vec<String>> {
        let root = self.workspace_root()?;
        let current = self.changed_files()?;
        baseline.changed_files_since(&root, current)
    }
}

pub fn detect_backend(path: impl AsRef<Path>) -> Result<Box<dyn VcsBackend>> {
    let path = path.as_ref();
    if JjBackend::is_repo(path) {
        return Ok(Box::new(JjBackend::new(path)?));
    }
    if GitBackend::is_repo(path) {
        return Ok(Box::new(GitBackend::new(path)?));
    }
    Err(anyhow!(
        "no supported VCS detected (jj preferred, git fallback)"
    ))
}

pub fn detect_mode(path: impl AsRef<Path>) -> VcsMode {
    let path = path.as_ref();
    classify_mode(
        JjBackend::is_repo(path),
        GitBackend::is_repo(path),
        is_jj_available(),
    )
}

pub fn enable_jj(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    match detect_mode(path) {
        VcsMode::Jj => Ok(()),
        VcsMode::GitWithJjAvailableButDisabled => {
            run_capture(path, "jj", &["git", "init", "--colocate", "."])?;
            Ok(())
        }
        VcsMode::GitOnly => Err(anyhow!(
            "jj is not installed; cannot enable jj for this repo"
        )),
        VcsMode::NoVcs => Err(anyhow!(
            "no supported VCS detected (jj preferred, git fallback)"
        )),
    }
}

pub struct JjBackend {
    root: PathBuf,
}

impl JjBackend {
    pub fn new(path: &Path) -> Result<Self> {
        let root = run_capture(path, "jj", &["root"])?;
        Ok(Self {
            root: PathBuf::from(root.trim()),
        })
    }

    pub fn is_repo(path: &Path) -> bool {
        Command::new("jj")
            .args(["root"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

fn classify_mode(is_jj_repo: bool, is_git_repo: bool, jj_available: bool) -> VcsMode {
    if is_jj_repo {
        VcsMode::Jj
    } else if is_git_repo {
        if jj_available {
            VcsMode::GitWithJjAvailableButDisabled
        } else {
            VcsMode::GitOnly
        }
    } else {
        VcsMode::NoVcs
    }
}

fn is_jj_available() -> bool {
    Command::new("jj")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

impl VcsBackend for JjBackend {
    fn kind(&self) -> VcsKind {
        VcsKind::Jj
    }
    fn workspace_root(&self) -> Result<PathBuf> {
        Ok(self.root.clone())
    }
    fn create_isolated_change(&self, name: &str) -> Result<IsolatedChange> {
        let base = self.current_change_ref().ok();
        run_capture(&self.root, "jj", &["new", "-m", name])?;
        let change_ref = self.current_change_ref()?;
        Ok(IsolatedChange {
            workspace_ref: self.root.display().to_string(),
            change_ref,
            base_ref: base,
        })
    }
    fn current_change_ref(&self) -> Result<String> {
        Ok(run_capture(
            &self.root,
            "jj",
            &["log", "--no-graph", "-r", "@", "--template", "change_id"],
        )?
        .trim()
        .to_string())
    }
    fn changed_files(&self) -> Result<Vec<String>> {
        Ok(lines(run_capture(
            &self.root,
            "jj",
            &["diff", "--name-only"],
        )?))
    }
    fn diff(&self) -> Result<String> {
        run_capture(&self.root, "jj", &["diff"])
    }
}

pub struct GitBackend {
    root: PathBuf,
}

impl GitBackend {
    pub fn new(path: &Path) -> Result<Self> {
        let root = run_capture(path, "git", &["rev-parse", "--show-toplevel"])?;
        Ok(Self {
            root: PathBuf::from(root.trim()),
        })
    }

    pub fn is_repo(path: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl ProvenanceBaseline {
    fn capture(root: &Path, changed_files: &[String]) -> Result<Self> {
        let mut snapshots = BTreeMap::new();
        for path in changed_files
            .iter()
            .filter(|path| !is_generated_target_path(path))
        {
            snapshots.insert(path.clone(), snapshot_file(root, path)?);
        }
        Ok(Self { snapshots })
    }

    fn changed_files_since(&self, root: &Path, current: Vec<String>) -> Result<Vec<String>> {
        let mut changed = Vec::new();
        for path in current
            .into_iter()
            .filter(|path| !is_generated_target_path(path))
        {
            let current_snapshot = snapshot_file(root, &path)?;
            match self.snapshots.get(&path) {
                None => changed.push(path),
                Some(previous_snapshot) if *previous_snapshot != current_snapshot => {
                    changed.push(path);
                }
                Some(_) => {}
            }
        }
        Ok(changed)
    }
}

impl VcsBackend for GitBackend {
    fn kind(&self) -> VcsKind {
        VcsKind::Git
    }
    fn workspace_root(&self) -> Result<PathBuf> {
        Ok(self.root.clone())
    }
    fn create_isolated_change(&self, name: &str) -> Result<IsolatedChange> {
        let base = self.current_change_ref().ok();
        let branch = unique_branch_name(name);
        let workspace_root = unique_git_worktree_path(&self.root, &branch);
        if let Some(parent) = workspace_root.parent() {
            fs::create_dir_all(parent)?;
        }
        run_capture(
            &self.root,
            "git",
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                workspace_root.to_string_lossy().as_ref(),
                "HEAD",
            ],
        )?;
        let change_ref = run_capture(&workspace_root, "git", &["rev-parse", "HEAD"])?
            .trim()
            .to_string();
        Ok(IsolatedChange {
            workspace_ref: workspace_root.display().to_string(),
            change_ref,
            base_ref: base,
        })
    }
    fn current_change_ref(&self) -> Result<String> {
        match run_capture(&self.root, "git", &["rev-parse", "HEAD"]) {
            Ok(value) => Ok(value.trim().to_string()),
            Err(_) => Ok(
                run_capture(&self.root, "git", &["rev-parse", "--abbrev-ref", "HEAD"])?
                    .trim()
                    .to_string(),
            ),
        }
    }
    fn changed_files(&self) -> Result<Vec<String>> {
        let mut changed = match run_capture(
            &self.root,
            "git",
            &["-c", "core.quotepath=false", "diff", "--name-only", "HEAD"],
        ) {
            Ok(output) => lines(output),
            Err(_) => {
                let output = run_capture(
                    &self.root,
                    "git",
                    &["-c", "core.quotepath=false", "status", "--porcelain"],
                )?;
                output
                    .lines()
                    .filter_map(|line| line.get(3..).map(str::trim))
                    .filter(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            }
        };
        for path in lines(run_capture(
            &self.root,
            "git",
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--",
                ":(exclude)target",
            ],
        )?) {
            push_unique(&mut changed, path);
        }
        Ok(changed)
    }
    fn diff(&self) -> Result<String> {
        match run_capture(&self.root, "git", &["diff", "HEAD"]) {
            Ok(output) => Ok(output),
            Err(_) => run_capture(&self.root, "git", &["diff"]),
        }
    }
}

fn sanitize_branch_name(input: &str) -> String {
    let sanitized = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "punk-change".to_string()
    } else {
        sanitized
    }
}

fn unique_branch_name(input: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{}", sanitize_branch_name(input), nanos)
}

fn unique_git_worktree_path(root: &Path, branch: &str) -> PathBuf {
    root.join(".punk")
        .join("worktrees")
        .join(branch.replace('/', "-"))
}

fn lines(output: String) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn push_unique(paths: &mut Vec<String>, path: String) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn is_generated_target_path(relative: &str) -> bool {
    matches!(
        Path::new(relative).components().next(),
        Some(std::path::Component::Normal(first)) if first == "target"
    )
}

fn snapshot_file(root: &Path, relative: &str) -> Result<FileSnapshot> {
    let path = root.join(relative);
    match fs::metadata(path) {
        Ok(metadata) => Ok(FileSnapshot::Present {
            len: metadata.len(),
            modified_ns: metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_nanos()),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileSnapshot::Missing),
        Err(error) => Err(error.into()),
    }
}

fn run_capture(dir: &Path, bin: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("spawn {bin} {args:?}"))?;
    if !output.status.success() {
        return Err(anyhow!(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn detect_git_repo() {
        let root = std::env::temp_dir().join(format!("punk-vcs-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        let backend = detect_backend(&root).unwrap();
        assert_eq!(backend.kind(), VcsKind::Git);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_changed_files_includes_tracked_dirty_and_untracked_files() {
        let root = std::env::temp_dir().join(format!(
            "punk-vcs-git-changed-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        run_capture(&root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_capture(&root, "git", &["config", "user.email", "test@example.com"]).unwrap();

        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_capture(&root, "git", &["add", "tracked.txt"]).unwrap();
        run_capture(&root, "git", &["commit", "-m", "init"]).unwrap();

        fs::write(root.join("tracked.txt"), "base\nupdated\n").unwrap();
        fs::write(root.join("untracked.txt"), "new\n").unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/build.log"), "generated\n").unwrap();

        let changed_files = GitBackend::new(&root)
            .unwrap()
            .changed_files()
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected = ["tracked.txt", "untracked.txt"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(changed_files, expected);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_provenance_excludes_top_level_target_generated_files() {
        let root = std::env::temp_dir().join(format!(
            "punk-vcs-git-provenance-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        run_capture(&root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_capture(&root, "git", &["config", "user.email", "test@example.com"]).unwrap();

        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_capture(&root, "git", &["add", "tracked.txt"]).unwrap();
        run_capture(&root, "git", &["commit", "-m", "init"]).unwrap();

        let backend = GitBackend::new(&root).unwrap();
        let baseline = backend.capture_provenance_baseline().unwrap();

        fs::write(root.join("tracked.txt"), "base\nupdated\n").unwrap();
        fs::write(root.join("untracked.txt"), "new\n").unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/build.log"), "generated\n").unwrap();

        let raw_changed = backend
            .changed_files()
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected_raw = ["tracked.txt", "untracked.txt"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(raw_changed, expected_raw);

        let provenance_changed = backend
            .changed_files_since(&baseline)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected_provenance = ["tracked.txt", "untracked.txt"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(provenance_changed, expected_provenance);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_provenance_only_filters_top_level_target_paths() {
        let root = std::env::temp_dir().join(format!(
            "punk-vcs-git-provenance-target-scope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        run_capture(&root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_capture(&root, "git", &["config", "user.email", "test@example.com"]).unwrap();

        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        run_capture(&root, "git", &["add", "tracked.txt"]).unwrap();
        run_capture(&root, "git", &["commit", "-m", "init"]).unwrap();

        let backend = GitBackend::new(&root).unwrap();
        let baseline = backend.capture_provenance_baseline().unwrap();

        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/build.log"), "generated\n").unwrap();
        fs::create_dir_all(root.join("nested/target")).unwrap();
        fs::write(root.join("nested/target/output.txt"), "keep-me\n").unwrap();

        let raw_changed = backend
            .changed_files()
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected_raw = ["nested/target/output.txt"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(raw_changed, expected_raw);

        let provenance_changed = backend
            .changed_files_since(&baseline)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let expected_provenance = ["nested/target/output.txt"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(provenance_changed, expected_provenance);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn classify_mode_reports_git_with_jj_available_but_disabled() {
        assert_eq!(
            classify_mode(false, true, true),
            VcsMode::GitWithJjAvailableButDisabled
        );
    }

    #[test]
    fn classify_mode_reports_git_only_without_jj() {
        assert_eq!(classify_mode(false, true, false), VcsMode::GitOnly);
    }

    #[test]
    fn classify_mode_prefers_jj() {
        assert_eq!(classify_mode(true, true, true), VcsMode::Jj);
    }

    #[test]
    fn git_isolated_change_uses_separate_worktree() {
        let root = std::env::temp_dir().join(format!(
            "punk-vcs-git-isolated-worktree-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        run_capture(&root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_capture(&root, "git", &["config", "user.email", "test@example.com"]).unwrap();
        fs::write(root.join("README.md"), "init\n").unwrap();
        run_capture(&root, "git", &["add", "README.md"]).unwrap();
        run_capture(&root, "git", &["commit", "-m", "init"]).unwrap();
        let original_ref = run_capture(&root, "git", &["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap()
            .trim()
            .to_string();

        let isolated = GitBackend::new(&root)
            .unwrap()
            .create_isolated_change("Stage 4 polish")
            .unwrap();
        let workspace_root = PathBuf::from(&isolated.workspace_ref);
        let canonical_workspace_root = fs::canonicalize(&workspace_root).unwrap();
        let canonical_worktrees_root = fs::canonicalize(root.join(".punk/worktrees")).unwrap();

        assert_ne!(canonical_workspace_root, fs::canonicalize(&root).unwrap());
        assert!(canonical_workspace_root.starts_with(&canonical_worktrees_root));
        assert_eq!(
            run_capture(&root, "git", &["rev-parse", "--abbrev-ref", "HEAD"])
                .unwrap()
                .trim(),
            original_ref
        );

        let _ = run_capture(
            &root,
            "git",
            &["worktree", "remove", "--force", &isolated.workspace_ref],
        );
        let _ = fs::remove_dir_all(&root);
    }
}
