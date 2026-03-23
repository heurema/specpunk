pub mod init;
pub mod vcs;

pub use vcs::{Vcs, VcsError, VcsType, detect};
