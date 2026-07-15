//! Shared Grok config.toml model entries (native + third-party backends).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntryView {
    pub id: String,
    pub model: String,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_backend: String,
    pub env_key: Option<String>,
    pub has_plain_api_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertModelRequest {
    pub id: String,
    pub model: String,
    pub name: Option<String>,
    pub base_url: String,
    pub api_backend: String,
    /// Prefer env var name; secret material is not written as api_key when set.
    pub env_key: Option<String>,
    /// Optional secret stored via OS keyring / env, not TOML when possible.
    pub secret: Option<String>,
}

pub fn config_path(home: &Path) -> PathBuf {
    home.join(".grok").join("config.toml")
}

pub fn ensure_grok_home(home: &Path) -> Result<PathBuf, String> {
    let grok = home.join(".grok");
    fs::create_dir_all(&grok).map_err(|e| e.to_string())?;
    fs::create_dir_all(grok.join("sessions")).map_err(|e| e.to_string())?;
    Ok(grok)
}

pub fn read_document(path: &Path) -> Result<DocumentMut, String> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    text.parse::<DocumentMut>().map_err(|e| e.to_string())
}

pub fn list_models(home: &Path) -> Result<Vec<ModelEntryView>, String> {
    let path = config_path(home);
    let doc = read_document(&path)?;
    let mut out = Vec::new();

    // Built-in placeholder (actual catalog comes from agent; UI still shows custom ones).
    out.push(ModelEntryView {
        id: "grok-build".into(),
        model: "grok-build".into(),
        name: Some("Grok Build (native)".into()),
        base_url: None,
        api_backend: "responses".into(),
        env_key: None,
        has_plain_api_key: false,
    });

    if let Some(table) = doc.as_table().get("model").and_then(|i| i.as_table()) {
        for (id, item) in table.iter() {
            if let Some(t) = item.as_table() {
                out.push(ModelEntryView {
                    id: id.to_string(),
                    model: t
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id)
                        .to_string(),
                    name: t.get("name").and_then(|v| v.as_str()).map(str::to_string),
                    base_url: t
                        .get("base_url")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    api_backend: t
                        .get("api_backend")
                        .and_then(|v| v.as_str())
                        .unwrap_or("chat_completions")
                        .to_string(),
                    env_key: t
                        .get("env_key")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    has_plain_api_key: t.get("api_key").is_some(),
                });
            }
        }
    }
    Ok(out)
}

/// Upsert `[model.<id>]` preferring `env_key` over plain `api_key`.
pub fn upsert_model(home: &Path, req: &UpsertModelRequest) -> Result<(), String> {
    if req.api_backend != "chat_completions"
        && req.api_backend != "responses"
        && req.api_backend != "messages"
    {
        return Err(format!(
            "unsupported api_backend: {} (use chat_completions|responses|messages)",
            req.api_backend
        ));
    }

    ensure_grok_home(home)?;
    let path = config_path(home);
    let mut doc = read_document(&path)?;

    if doc.get("model").is_none() {
        doc["model"] = Item::Table(Table::new());
    }
    let model_root = doc["model"].as_table_mut().ok_or("model table")?;
    let entry = model_root
        .entry(&req.id)
        .or_insert(Item::Table(Table::new()));
    let t = entry.as_table_mut().ok_or("model entry")?;

    t["model"] = value(&req.model);
    t["base_url"] = value(&req.base_url);
    t["api_backend"] = value(&req.api_backend);
    if let Some(name) = &req.name {
        t["name"] = value(name);
    }

    // Prefer env_key; never write raw secret as api_key when env_key is provided.
    if let Some(env_key) = &req.env_key {
        t["env_key"] = value(env_key);
        t.remove("api_key");
        if let Some(secret) = &req.secret {
            // Set process env for current desktop session + store in keyring.
            // SAFETY: single-process desktop host; intentional credential injection.
            unsafe {
                std::env::set_var(env_key, secret);
            }
            let _ = crate::secrets::store_secret(env_key, secret);
        }
    } else if let Some(secret) = &req.secret {
        // Last resort: still avoid TOML — store keyring under model id and set env.
        let env_name = format!("GROK_DESKTOP_MODEL_{}", req.id.to_ascii_uppercase());
        t["env_key"] = value(&env_name);
        t.remove("api_key");
        unsafe {
            std::env::set_var(&env_name, secret);
        }
        let _ = crate::secrets::store_secret(&env_name, secret);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

/// List session summary.json files under ~/.grok/sessions for resume UI.
pub fn list_session_summaries(home: &Path) -> Result<Vec<serde_json::Value>, String> {
    let root = home.join(".grok").join("sessions");
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for group in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let group = group.map_err(|e| e.to_string())?;
        if !group.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        for sess in fs::read_dir(group.path()).map_err(|e| e.to_string())? {
            let sess = sess.map_err(|e| e.to_string())?;
            let summary = sess.path().join("summary.json");
            if summary.is_file() {
                if let Ok(text) = fs::read_to_string(&summary) {
                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert(
                                "sessionDir".into(),
                                serde_json::json!(sess.path().display().to_string()),
                            );
                            obj.insert(
                                "sessionId".into(),
                                serde_json::json!(sess.file_name().to_string_lossy()),
                            );
                        }
                        out.push(v);
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn upsert_prefers_env_key_not_plain_api_key() {
        let tmp = TempDir::new().unwrap();
        upsert_model(
            tmp.path(),
            &UpsertModelRequest {
                id: "mock-chat".into(),
                model: "mock-model".into(),
                name: Some("Mock Chat".into()),
                base_url: "http://127.0.0.1:9/v1".into(),
                api_backend: "chat_completions".into(),
                env_key: Some("MOCK_OPENAI_KEY".into()),
                secret: Some("sk-test".into()),
            },
        )
        .unwrap();
        let text = fs::read_to_string(config_path(tmp.path())).unwrap();
        assert!(text.contains("api_backend = \"chat_completions\""));
        assert!(text.contains("env_key = \"MOCK_OPENAI_KEY\""));
        assert!(!text.contains("api_key"));
        assert_eq!(std::env::var("MOCK_OPENAI_KEY").unwrap(), "sk-test");
    }

    #[test]
    fn upsert_responses_backend() {
        let tmp = TempDir::new().unwrap();
        upsert_model(
            tmp.path(),
            &UpsertModelRequest {
                id: "mock-resp".into(),
                model: "mock-model".into(),
                name: None,
                base_url: "http://127.0.0.1:9/v1".into(),
                api_backend: "responses".into(),
                env_key: Some("MOCK_RESP_KEY".into()),
                secret: Some("sk-r".into()),
            },
        )
        .unwrap();
        let models = list_models(tmp.path()).unwrap();
        let m = models.iter().find(|m| m.id == "mock-resp").unwrap();
        assert_eq!(m.api_backend, "responses");
        assert_eq!(m.has_plain_api_key, false);
    }
}
