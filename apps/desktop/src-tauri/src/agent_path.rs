//! Resolve the bundled / in-tree Grok agent binary.
//!
//! Resolution order (first hit wins):
//! 1. `GROK_DESKTOP_AGENT_PATH` — explicit dev override (non-default)
//! 2. Resource path next to the desktop app (`resources/agent/grok-agent[.exe]`)
//! 3. In-tree cargo artifacts: `target/{debug,release}/xai-grok-pager[.exe]`
//!    relative to workspace root (ancestor of this crate)
//! 4. `resources/agent/grok-agent` under the desktop crate directory (dev)
//!
//! **Never** defaults to `PATH` lookup of `grok` — that is not a production path.

use std::env;
use std::path::{Path, PathBuf};

#[cfg(windows)]
const AGENT_NAME: &str = "grok-agent.exe";
#[cfg(not(windows))]
const AGENT_NAME: &str = "grok-agent";

#[cfg(windows)]
const PAGER_NAME: &str = "xai-grok-pager.exe";
#[cfg(not(windows))]
const PAGER_NAME: &str = "xai-grok-pager";

/// Resolve the agent executable used for ACP stdio.
pub fn resolve_agent_binary() -> Result<PathBuf, String> {
    if let Ok(override_path) = env::var("GROK_DESKTOP_AGENT_PATH") {
        let p = PathBuf::from(override_path);
        if p.is_file() {
            return Ok(dunce::canonicalize(&p).unwrap_or(p));
        }
        return Err(format!(
            "GROK_DESKTOP_AGENT_PATH is set but not a file: {}",
            p.display()
        ));
    }

    if let Ok(resource) = resource_agent_path() {
        if resource.is_file() {
            return Ok(resource);
        }
    }

    if let Some(in_tree) = in_tree_pager_binary() {
        return Ok(in_tree);
    }

    if let Some(dev_res) = desktop_crate_resource_agent() {
        if dev_res.is_file() {
            return Ok(dev_res);
        }
    }

    Err(
        "could not locate bundled Grok agent binary (set GROK_DESKTOP_AGENT_PATH for dev override)"
            .into(),
    )
}

/// True when the resolved path came from env override (dev-only).
pub fn is_env_override() -> bool {
    env::var_os("GROK_DESKTOP_AGENT_PATH").is_some()
}

fn resource_agent_path() -> Result<PathBuf, String> {
    // Tauri resource dir when packaged; fall back during unit tests.
    if let Ok(dir) = env::var("GROK_DESKTOP_RESOURCE_DIR") {
        return Ok(PathBuf::from(dir).join("agent").join(AGENT_NAME));
    }
    // When running under `cargo test` / `cargo run` from src-tauri:
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest.join("resources").join("agent").join(AGENT_NAME))
}

fn desktop_crate_resource_agent() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("agent")
        .join(AGENT_NAME);
    p.is_file().then_some(p)
}

fn in_tree_pager_binary() -> Option<PathBuf> {
    let workspace = find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR")))?;
    for profile in ["release", "debug"] {
        let candidate = workspace.join("target").join(profile).join(PAGER_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join("Cargo.toml").is_file() && dir.join("crates").is_dir() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_resource_agent_exists_in_tree() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("agent")
            .join(AGENT_NAME);
        assert!(
            p.is_file(),
            "expected bundled agent at {} — copy grok-agent into resources/agent",
            p.display()
        );
    }

    #[test]
    fn resolve_prefers_non_path_location() {
        // Ensure we don't accidentally require PATH `grok`.
        let path = resolve_agent_binary().expect("resolve agent");
        assert!(path.is_file(), "{}", path.display());
        let s = path.to_string_lossy();
        // Should be under our tree or absolute resource, not a bare "grok"
        assert!(
            s.contains("grok-agent") || s.contains("xai-grok-pager"),
            "unexpected agent path: {s}"
        );
    }
}
