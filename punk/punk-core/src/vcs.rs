use std::path::Path;
use std::process::Command;

/// The type of version control system detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsType {
    Jj,
    Git,
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

    /// Returns the unified diff of the current working change/commit.
    fn diff(&self) -> Result<String, VcsError>;
}

/// Auto-detect the VCS for a given directory. JJ is checked before Git.
pub fn detect(path: &Path) -> Result<Box<dyn Vcs>, VcsError> {
    // Check jj first — it takes priority over git
    if is_jj_repo(path) {
        return Ok(Box::new(JjVcs { root: path.to_path_buf() }));
    }
    if is_git_repo(path) {
        return Ok(Box::new(GitVcs { root: path.to_path_buf() }));
    }
    Err(VcsError::NotDetected)
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
            .args(["diff", "--name-only", "HEAD"])
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
