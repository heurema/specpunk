use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

pub fn relative_ref(repo_root: &Path, path: &Path) -> Result<String> {
    Ok(path.strip_prefix(repo_root)?.to_string_lossy().to_string())
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes)?;
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn find_object_path(dir: &Path, id: &str) -> Result<PathBuf> {
    if !dir.exists() {
        return Err(anyhow!("no objects under {}", dir.display()));
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let value: serde_json::Value =
                read_json(&path).with_context(|| format!("read object {}", path.display()))?;
            if value.get("id").and_then(|v| v.as_str()) == Some(id) {
                return Ok(path);
            }
        }
    }
    Err(anyhow!("unknown id: {id}"))
}
