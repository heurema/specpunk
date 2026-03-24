pub mod artifacts;
pub mod greenfield;
pub mod scan;

use std::path::Path;

pub use artifacts::write_artifacts;
pub use greenfield::{GreenFieldAnswers, run_greenfield};
pub use scan::{ScanResult, run_scan};

/// How the project was detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitMode {
    Brownfield,
    Greenfield,
}

/// High-level result from an init run.
#[derive(Debug)]
pub struct InitResult {
    pub mode: InitMode,
    pub artifacts_written: Vec<String>,
}

/// Run the init flow on the given path.
/// If the directory looks empty (<5 non-config source files) and `answers`
/// is provided, greenfield mode is used. Otherwise brownfield scan is used.
pub fn run_init(root: &Path, answers: Option<GreenFieldAnswers>) -> Result<InitResult, InitError> {
    let mode = detect_mode(root)?;
    match mode {
        InitMode::Greenfield => {
            let gf = answers.unwrap_or_default();
            let artifacts = run_greenfield(root, &gf)?;
            write_artifacts(root, &artifacts, false)?;
            let names = artifacts.artifact_names();
            Ok(InitResult { mode: InitMode::Greenfield, artifacts_written: names })
        }
        InitMode::Brownfield => {
            let scan = run_scan(root)?;
            let artifacts = scan.to_artifacts();
            write_artifacts(root, &artifacts, true)?;
            let names = artifacts.artifact_names();
            Ok(InitResult { mode: InitMode::Brownfield, artifacts_written: names })
        }
    }
}

/// Detect whether the project is greenfield or brownfield.
fn detect_mode(root: &Path) -> Result<InitMode, InitError> {
    let source_count = count_source_files(root);
    if source_count < 5 {
        Ok(InitMode::Greenfield)
    } else {
        Ok(InitMode::Brownfield)
    }
}

/// Count non-config source files. We look for files with code extensions,
/// excluding common config-only extensions.
fn count_source_files(root: &Path) -> usize {
    let code_exts = [
        "rs", "go", "py", "ts", "js", "tsx", "jsx", "java", "kt",
        "cpp", "c", "h", "cs", "rb", "php", "swift",
    ];
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 { return true; }
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "target" && name != "node_modules" && name != "vendor"
        });

    let mut count = 0usize;
    for entry in walker.flatten() {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if code_exts.contains(&ext.as_str()) {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Errors from the init flow.
#[derive(Debug)]
pub enum InitError {
    Io(std::io::Error),
    Scan(String),
    Serialize(String),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitError::Io(e) => write!(f, "I/O error: {e}"),
            InitError::Scan(s) => write!(f, "scan error: {s}"),
            InitError::Serialize(s) => write!(f, "serialize error: {s}"),
        }
    }
}

impl std::error::Error for InitError {}

impl From<std::io::Error> for InitError {
    fn from(e: std::io::Error) -> Self {
        InitError::Io(e)
    }
}
