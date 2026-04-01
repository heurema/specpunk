use std::path::Path;
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
    // Check jj first — it takes priority over git
    if is_jj_repo(path) {
        return Ok(Box::new(JjVcs {
            root: path.to_path_buf(),
        }));
    }
    if is_git_repo(path) {
        return Ok(Box::new(GitVcs {
            root: path.to_path_buf(),
        }));
    }
    Err(VcsError::NotDetected)
}

/// Detect the current user-facing VCS mode for a repository path.
pub fn detect_mode(path: &Path) -> VcsMode {
    classify_mode(is_jj_repo(path), is_git_repo(path), is_jj_available())
}

/// Explicitly enable jj for an existing Git repo without auto-mutating on detection.
pub fn enable_jj(path: &Path) -> Result<(), VcsError> {
    match detect_mode(path) {
        VcsMode::Jj => Ok(()),
        VcsMode::GitWithJjAvailableButDisabled => {
            let out = Command::new("jj")
                .args(["git", "init", "--colocate", "."])
                .current_dir(path)
                .output()
                .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
            if !out.status.success() {
                return Err(VcsError::CommandFailed(
                    String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                ));
            }
            Ok(())
        }
        VcsMode::GitOnly => Err(VcsError::CommandFailed(
            "jj is not installed; cannot enable jj for this repo".to_string(),
        )),
        VcsMode::NoVcs => Err(VcsError::NotDetected),
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

fn is_jj_repo(path: &Path) -> bool {
    Command::new("jj")
        .args(["root"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// JJ implementation
// ---------------------------------------------------------------------------

pub struct JjVcs {
    root: std::path::PathBuf,
}

impl Vcs for JjVcs {
    fn vcs_type(&self) -> VcsType {
        VcsType::Jj
    }

    fn change_id(&self) -> Result<String, VcsError> {
        let out = Command::new("jj")
            .args(["log", "--no-graph", "-r", "@", "--template", "change_id"])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn changed_files(&self) -> Result<Vec<String>, VcsError> {
        let out = Command::new("jj")
            .args(["diff", "--name-only"])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        let files = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect();
        Ok(files)
    }

    fn untracked_files(&self) -> Result<Vec<String>, VcsError> {
        // jj tracks all files in the working copy — no concept of "untracked"
        Ok(Vec::new())
    }

    fn diff(&self) -> Result<String, VcsError> {
        let out = Command::new("jj")
            .args(["diff"])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_mode, VcsMode};

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
}

// ---------------------------------------------------------------------------
// Git implementation
// ---------------------------------------------------------------------------

pub struct GitVcs {
    root: std::path::PathBuf,
}

impl Vcs for GitVcs {
    fn vcs_type(&self) -> VcsType {
        VcsType::Git
    }

    fn change_id(&self) -> Result<String, VcsError> {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn changed_files(&self) -> Result<Vec<String>, VcsError> {
        let out = Command::new("git")
            .args(["-c", "core.quotepath=false", "diff", "--name-only", "HEAD"])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        let files = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect();
        Ok(files)
    }

    fn untracked_files(&self) -> Result<Vec<String>, VcsError> {
        let out = Command::new("git")
            .args([
                "-c",
                "core.quotepath=false",
                "ls-files",
                "--others",
                "--exclude-standard",
            ])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        let files = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect();
        Ok(files)
    }

    fn diff(&self) -> Result<String, VcsError> {
        let out = Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&self.root)
            .output()
            .map_err(|e| VcsError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(VcsError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}
