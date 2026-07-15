//! Workspace / folder-trust helpers aligned with Grok conventions.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustStatus {
    pub cwd: String,
    pub trusted: bool,
    pub reason: String,
}

/// Read trust from `~/.grok` project trust files if present; default untrusted
/// for paths outside home unless marked trusted.
pub fn workspace_trust_status(home: &Path, cwd: &Path) -> TrustStatus {
    let cwd_s = cwd.display().to_string();
    let trust_file = home.join(".grok").join("trusted_folders");
    if let Ok(text) = fs::read_to_string(&trust_file) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if Path::new(line) == cwd || cwd.starts_with(line) {
                return TrustStatus {
                    cwd: cwd_s,
                    trusted: true,
                    reason: "listed in trusted_folders".into(),
                };
            }
        }
    }

    // Project-local marker (optional desktop convention)
    if cwd.join(".grok").join("trusted").is_file() || cwd.join(".grok-trusted").is_file() {
        return TrustStatus {
            cwd: cwd_s,
            trusted: true,
            reason: "project trust marker".into(),
        };
    }

    TrustStatus {
        cwd: cwd_s,
        trusted: false,
        reason: "workspace not trusted — grant trust before elevated tools".into(),
    }
}

pub fn grant_trust(home: &Path, cwd: &Path) -> Result<TrustStatus, String> {
    let grok = home.join(".grok");
    fs::create_dir_all(&grok).map_err(|e| e.to_string())?;
    let trust_file = grok.join("trusted_folders");
    let mut existing = fs::read_to_string(&trust_file).unwrap_or_default();
    let line = cwd.display().to_string();
    if !existing.lines().any(|l| l.trim() == line) {
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(&line);
        existing.push('\n');
        fs::write(&trust_file, existing).map_err(|e| e.to_string())?;
    }
    Ok(workspace_trust_status(home, cwd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn untrusted_by_default() {
        let tmp = TempDir::new().unwrap();
        let st = workspace_trust_status(tmp.path(), tmp.path().join("proj").as_path());
        // path may not exist; still untrusted
        assert!(!st.trusted);
    }

    #[test]
    fn grant_marks_trusted() {
        let tmp = TempDir::new().unwrap();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let st = grant_trust(tmp.path(), &proj).unwrap();
        assert!(st.trusted);
        let st2 = workspace_trust_status(tmp.path(), &proj);
        assert!(st2.trusted);
    }
}
