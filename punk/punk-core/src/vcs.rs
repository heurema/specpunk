use std::path::{Path, PathBuf};
use std::process::Command;

/// The type of version control system detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsType {
    Jj,
    Git,
}

/// User-facing VCS operating mode for the current repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsMode {
    Jj,
    GitOnly,
    GitWithJjAvailableButDisabled,
    NoVcs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedChange {
    pub workspace_root: PathBuf,
    pub change_id: String,
    pub base_change_id: Option<String>,
}

/// Errors returned by VCS operations.
#[derive(Debug)]
pub enum VcsError {
    NotDetected,
    CommandFailed(String),
}

impl std::fmt::Display for VcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VcsError::NotDetected => write!(f, "no VCS detected in current directory"),
            VcsError::CommandFailed(msg) => write!(f, "VCS command failed: {msg}"),
        }
    }
}

impl std::error::Error for VcsError {}

/// Abstraction over jj and git version control systems.
pub trait Vcs {
    /// Returns the VCS type backing this instance.
    fn vcs_type(&self) -> VcsType;

    /// Returns the current change/commit identifier.
    fn change_id(&self) -> Result<String, VcsError>;

    /// Returns a list of files changed in the current working change/commit.
    fn changed_files(&self) -> Result<Vec<String>, VcsError>;

    /// Returns a list of untracked files (not yet added to VCS).
    fn untracked_files(&self) -> Result<Vec<String>, VcsError>;

    /// Returns the unified diff of the current working change/commit.
    fn diff(&self) -> Result<String, VcsError>;
}

/// Auto-detect the VCS for a given directory. JJ is checked before Git.
pub fn detect(path: &Path) -> Result<Box<dyn Vcs>, VcsError> {
    if JjVcs::is_repo(path) {
        return Ok(Box::new(JjVcs::new(path)?));
    }
    if GitVcs::is_repo(path) {
        return Ok(Box::new(GitVcs::new(path)?));
    }
    Err(VcsError::NotDetected)
}

/// Detect the current user-facing VCS mode for a repository path.
pub fn detect_mode(path: &Path) -> VcsMode {
    classify_mode(
        JjVcs::is_repo(path),
        GitVcs::is_repo(path),
        is_jj_available(),
    )
}

/// Explicitly enable jj for an existing Git repo without auto-mutating on detection.
pub fn enable_jj(path: &Path) -> Result<(), VcsError> {
    match detect_mode(path) {
        VcsMode::Jj => Ok(()),
        VcsMode::GitWithJjAvailableButDisabled => {
            run_capture(path, "jj", &["git", "init", "--colocate", "."])?;
            Ok(())
        }
        VcsMode::GitOnly => Err(VcsError::CommandFailed(
            "jj is not installed; cannot enable jj for this repo".to_string(),
        )),
        VcsMode::NoVcs => Err(VcsError::NotDetected),
    }
}

pub fn create_isolated_change(path: &Path, name: &str) -> Result<IsolatedChange, VcsError> {
    if JjVcs::is_repo(path) {
        let vcs = JjVcs::new(path)?;
        let base_change_id = vcs.change_id().ok();
        run_capture(&vcs.root, "jj", &["new", "-m", name])?;
        let change_id = vcs.change_id()?;
        return Ok(IsolatedChange {
            workspace_root: vcs.root,
            change_id,
            base_change_id,
        });
    }
    if GitVcs::is_repo(path) {
        let vcs = GitVcs::new(path)?;
        let base_change_id = vcs.change_id().ok();
        let branch = sanitize_branch_name(name);
        if run_capture(&vcs.root, "git", &["switch", "-c", &branch]).is_err() {
            run_capture(&vcs.root, "git", &["checkout", "--orphan", &branch])?;
        }
        let change_id = vcs.change_id()?;
        return Ok(IsolatedChange {
            workspace_root: vcs.root,
            change_id,
            base_change_id,
        });
    }
    Err(VcsError::NotDetected)
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

// ---------------------------------------------------------------------------
// JJ implementation
// ---------------------------------------------------------------------------

pub struct JjVcs {
    root: PathBuf,
}

impl JjVcs {
    fn new(path: &Path) -> Result<Self, VcsError> {
        Ok(Self {
            root: PathBuf::from(run_capture(path, "jj", &["root"])?),
        })
    }

    fn is_repo(path: &Path) -> bool {
        Command::new("jj")
            .args(["root"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Vcs for JjVcs {
    fn vcs_type(&self) -> VcsType {
        VcsType::Jj
    }

    fn change_id(&self) -> Result<String, VcsError> {
        Ok(run_capture(
            &self.root,
            "jj",
            &["log", "--no-graph", "-r", "@", "--template", "change_id"],
        )?
        .trim()
        .to_owned())
    }

    fn changed_files(&self) -> Result<Vec<String>, VcsError> {
        Ok(lines(run_capture(
            &self.root,
            "jj",
            &["diff", "--name-only"],
        )?))
    }

    fn untracked_files(&self) -> Result<Vec<String>, VcsError> {
        // jj tracks all files in the working copy — no concept of "untracked"
        Ok(Vec::new())
    }

    fn diff(&self) -> Result<String, VcsError> {
        run_capture(&self.root, "jj", &["diff"])
    }
}

// ---------------------------------------------------------------------------
// Git implementation
// ---------------------------------------------------------------------------

pub struct GitVcs {
    root: PathBuf,
}

impl GitVcs {
    fn new(path: &Path) -> Result<Self, VcsError> {
        Ok(Self {
            root: PathBuf::from(run_capture(path, "git", &["rev-parse", "--show-toplevel"])?),
        })
    }

    fn is_repo(path: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

impl Vcs for GitVcs {
    fn vcs_type(&self) -> VcsType {
        VcsType::Git
    }

    fn change_id(&self) -> Result<String, VcsError> {
        match run_capture(&self.root, "git", &["rev-parse", "HEAD"]) {
            Ok(value) => Ok(value.trim().to_owned()),
            Err(_) => Ok(
                run_capture(&self.root, "git", &["rev-parse", "--abbrev-ref", "HEAD"])?
                    .trim()
                    .to_owned(),
            ),
        }
    }

    fn changed_files(&self) -> Result<Vec<String>, VcsError> {
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

        for path in self.untracked_files()? {
            push_unique(&mut changed, path);
        }
        Ok(changed)
    }

    fn untracked_files(&self) -> Result<Vec<String>, VcsError> {
        Ok(lines(run_capture(
            &self.root,
            "git",
            &[
                "-c",
                "core.quotepath=false",
                "ls-files",
                "--others",
                "--exclude-standard",
            ],
        )?))
    }

    fn diff(&self) -> Result<String, VcsError> {
        match run_capture(&self.root, "git", &["diff", "HEAD"]) {
            Ok(output) => Ok(output),
            Err(_) => run_capture(&self.root, "git", &["diff"]),
        }
    }
}

fn run_capture(dir: &Path, bin: &str, args: &[&str]) -> Result<String, VcsError> {
    let out = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
    if !out.status.success() {
        return Err(VcsError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn lines(output: String) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn push_unique(paths: &mut Vec<String>, path: String) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn sanitize_branch_name(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{classify_mode, create_isolated_change, detect, run_capture, VcsMode};
    use std::fs;

    fn temp_repo(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn classify_mode_prefers_jj_repo() {
        assert_eq!(classify_mode(true, true, true), VcsMode::Jj);
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
    fn classify_mode_reports_no_vcs() {
        assert_eq!(classify_mode(false, false, true), VcsMode::NoVcs);
    }

    #[test]
    fn enable_jj_preconditions_are_mode_gated() {
        assert_eq!(
            classify_mode(false, true, true),
            VcsMode::GitWithJjAvailableButDisabled
        );
        assert_eq!(classify_mode(false, true, false), VcsMode::GitOnly);
        assert_eq!(classify_mode(true, true, true), VcsMode::Jj);
    }

    #[test]
    fn detect_uses_repo_root_for_nested_git_path() {
        let root = temp_repo("punk-core-vcs-git-root");
        let nested = root.join("nested/deep");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&nested).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        run_capture(&root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_capture(&root, "git", &["config", "user.email", "test@example.com"]).unwrap();
        fs::write(root.join("README.md"), "init\n").unwrap();
        run_capture(&root, "git", &["add", "README.md"]).unwrap();
        run_capture(&root, "git", &["commit", "-m", "init"]).unwrap();
        fs::write(root.join("README.md"), "changed\n").unwrap();

        let vcs = detect(&nested).expect("git repo should be detected from nested path");
        assert_eq!(vcs.vcs_type(), super::VcsType::Git);
        assert_eq!(vcs.changed_files().unwrap(), vec!["README.md".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_isolated_change_returns_change_for_git_repo() {
        let root = temp_repo("punk-core-vcs-isolated-change");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        run_capture(&root, "git", &["init"]).unwrap();
        run_capture(&root, "git", &["config", "user.name", "Test User"]).unwrap();
        run_capture(&root, "git", &["config", "user.email", "test@example.com"]).unwrap();
        fs::write(root.join("README.md"), "init\n").unwrap();
        run_capture(&root, "git", &["add", "README.md"]).unwrap();
        run_capture(&root, "git", &["commit", "-m", "init"]).unwrap();

        let change = create_isolated_change(&root, "Stage 4 polish").unwrap();
        assert_eq!(
            std::fs::canonicalize(&change.workspace_root).unwrap(),
            std::fs::canonicalize(&root).unwrap()
        );
        assert!(!change.change_id.is_empty());
        assert!(change.base_change_id.is_some());

        let _ = fs::remove_dir_all(&root);
    }
}
