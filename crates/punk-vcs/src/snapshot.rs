use std::path::Path;

use anyhow::{anyhow, Result};
use punk_domain::council::RepoSnapshotRef;
use punk_domain::VcsKind;

use crate::{GitBackend, JjBackend, VcsBackend};

pub fn current_snapshot_ref(path: impl AsRef<Path>) -> Result<RepoSnapshotRef> {
    let path = path.as_ref();

    if JjBackend::is_repo(path) {
        return Ok(match JjBackend::new(path) {
            Ok(backend) => RepoSnapshotRef {
                vcs: Some(VcsKind::Jj),
                head_ref: backend.current_change_ref().ok(),
                dirty: backend
                    .changed_files()
                    .map(|files| !files.is_empty())
                    .unwrap_or(false),
            },
            Err(_) => RepoSnapshotRef {
                vcs: Some(VcsKind::Jj),
                head_ref: None,
                dirty: false,
            },
        });
    }

    if GitBackend::is_repo(path) {
        let backend = GitBackend::new(path)?;
        return Ok(RepoSnapshotRef {
            vcs: Some(VcsKind::Git),
            head_ref: backend.current_change_ref().ok(),
            dirty: backend
                .changed_files()
                .map(|files| !files.is_empty())
                .unwrap_or(false),
        });
    }

    Err(anyhow!("no supported VCS detected for snapshot capture"))
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use super::*;
    use crate::run_capture;

    #[test]
    fn current_snapshot_ref_reports_git_head_and_dirty_status() {
        let root = std::env::temp_dir().join(format!(
            "punk-vcs-snapshot-git-{}-{}",
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

        let clean = current_snapshot_ref(&root).unwrap();
        assert_eq!(clean.vcs, Some(VcsKind::Git));
        assert!(clean.head_ref.is_some());
        assert!(!clean.dirty);

        fs::write(root.join("tracked.txt"), "base\nupdated\n").unwrap();
        let dirty = current_snapshot_ref(&root).unwrap();
        assert_eq!(dirty.vcs, Some(VcsKind::Git));
        assert_eq!(dirty.head_ref, clean.head_ref);
        assert!(dirty.dirty);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn current_snapshot_ref_prefers_jj_in_colocated_repo() {
        if Command::new("jj").arg("--version").output().is_err() {
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "punk-vcs-snapshot-jj-{}-{}",
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
        run_capture(&root, "jj", &["git", "init", "--colocate", "."]).unwrap();

        let snapshot = current_snapshot_ref(&root).unwrap();
        assert_eq!(snapshot.vcs, Some(VcsKind::Jj));
        assert!(snapshot.head_ref.is_some());

        let _ = fs::remove_dir_all(&root);
    }
}
