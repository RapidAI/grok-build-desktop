//! Subprocess ACP client: spawn agent, NDJSON I/O, request/response + notifications.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{Duration, timeout};

use super::framing::{decode_line, encode_line};
use crate::agent_path::resolve_agent_binary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub model_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamText {
    pub session_id: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub request_id: u64,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub summary: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpEvent {
    Stream(StreamText),
    Permission(PermissionRequest),
    SessionClosed { session_id: String },
    AgentExited { code: Option<i32> },
    Error { message: String },
}

struct Pending {
    tx: oneshot::Sender<Result<Value, String>>,
}

pub struct AcpClient {
    _child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending: Arc<std::sync::Mutex<HashMap<u64, Pending>>>,
    event_tx: mpsc::UnboundedSender<AcpEvent>,
    /// Cached permission options by JSON-RPC request id for respond_permission.
    permission_opts: Arc<std::sync::Mutex<HashMap<u64, Vec<PermissionOption>>>>,
    pub agent_path: PathBuf,
    pub home: PathBuf,
}

impl AcpClient {
    /// Spawn the bundled agent with ACP stdio.
    pub async fn spawn(
        cwd: &Path,
        home: &Path,
        extra_env: &[(&str, &str)],
        event_tx: mpsc::UnboundedSender<AcpEvent>,
    ) -> Result<Self, String> {
        let agent = resolve_agent_binary()?;
        let mut cmd = Command::new(&agent);
        cmd.args(["agent", "stdio", "--no-leader"])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("GROK_HOME", home.join(".grok"))
            // Prefer API key / config paths over interactive browser login.
            .env("XAI_API_KEY", std::env::var("XAI_API_KEY").unwrap_or_else(|_| "test-key".into()));

        // On Windows, USERPROFILE is the usual home.
        #[cfg(windows)]
        {
            cmd.env("USERPROFILE", home);
        }

        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn agent {}: {e}", agent.display()))?;

        let stdin = child.stdin.take().ok_or("agent stdin missing")?;
        let stdout = child.stdout.take().ok_or("agent stdout missing")?;
        let stderr = child.stderr.take();

        let pending: Arc<std::sync::Mutex<HashMap<u64, Pending>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let permission_opts: Arc<std::sync::Mutex<HashMap<u64, Vec<PermissionOption>>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let pending_r = pending.clone();
        let perm_r = permission_opts.clone();
        let ev = event_tx.clone();

        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                match decode_line(&line) {
                    Ok(msg) => {
                        handle_incoming(msg, &pending_r, &perm_r, &ev);
                    }
                    Err(e) => {
                        let _ = ev.send(AcpEvent::Error {
                            message: format!("bad ACP line: {e}; {line}"),
                        });
                    }
                }
            }
            let _ = ev.send(AcpEvent::AgentExited { code: None });
        });

        if let Some(stderr) = stderr {
            let ev2 = event_tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "grok_agent", "{line}");
                    // Surface fatal-looking lines
                    if line.contains("panic") || line.contains("FATAL") {
                        let _ = ev2.send(AcpEvent::Error { message: line });
                    }
                }
            });
        }

        Ok(Self {
            _child: child,
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: AtomicU64::new(1),
            pending,
            event_tx,
            permission_opts,
            agent_path: agent,
            home: home.to_path_buf(),
        })
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, Pending { tx });
        {
            let mut stdin = self.stdin.lock().await;
            let line = encode_line(&msg);
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| format!("write ACP: {e}"))?;
            stdin.flush().await.map_err(|e| format!("flush ACP: {e}"))?;
        }
        match timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err("ACP response channel closed".into()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("ACP request timed out: {method}"))
            }
        }
    }

    pub async fn initialize(&self) -> Result<Value, String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false
                },
                "_meta": {
                    "startupHints": {
                        "nonInteractive": true,
                        "skipGitStatus": true,
                        "skipProjectLayout": true
                    },
                    "clientType": "grok-desktop",
                    "clientVersion": env!("CARGO_PKG_VERSION")
                }
            }),
        )
        .await
    }

    pub async fn authenticate_api_key(&self) -> Result<Value, String> {
        self.request(
            "authenticate",
            json!({
                "methodId": "xai.api_key",
                "_meta": { "headless": true }
            }),
        )
        .await
    }

    pub async fn session_new(&self, cwd: &Path, model_id: Option<&str>) -> Result<String, String> {
        let mut params = json!({
            "cwd": cwd,
            "mcpServers": []
        });
        if let Some(m) = model_id {
            params["_meta"] = json!({ "modelId": m });
        }
        let res = self.request("session/new", params).await?;
        res.get("sessionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("session/new missing sessionId: {res}"))
    }

    pub async fn session_load(&self, session_id: &str, cwd: &Path) -> Result<Value, String> {
        self.request(
            "session/load",
            json!({
                "sessionId": session_id,
                "cwd": cwd,
                "mcpServers": []
            }),
        )
        .await
    }

    pub async fn session_prompt(&self, session_id: &str, text: &str) -> Result<Value, String> {
        self.request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }),
        )
        .await
    }

    pub async fn respond_permission(
        &self,
        request_id: u64,
        decision: PermissionDecision,
    ) -> Result<(), String> {
        let opts = self
            .permission_opts
            .lock()
            .unwrap()
            .get(&request_id)
            .cloned()
            .unwrap_or_default();

        let option_id = pick_option_id(&opts, &decision);
        let result = if matches!(decision, PermissionDecision::Cancel) || option_id.is_none() {
            json!({ "outcome": { "outcome": "cancelled" } })
        } else {
            json!({
                "outcome": {
                    "outcome": "selected",
                    "optionId": option_id.unwrap()
                }
            })
        };

        let msg = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": result,
        });
        let mut stdin = self.stdin.lock().await;
        let line = encode_line(&msg);
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("write permission response: {e}"))?;
        stdin.flush().await.map_err(|e| e.to_string())?;
        self.permission_opts.lock().unwrap().remove(&request_id);
        Ok(())
    }

    pub fn agent_path(&self) -> &Path {
        &self.agent_path
    }
}

fn pick_option_id(opts: &[PermissionOption], decision: &PermissionDecision) -> Option<String> {
    let want = match decision {
        PermissionDecision::AllowOnce => "allow_once",
        PermissionDecision::AllowAlways => "allow_always",
        PermissionDecision::RejectOnce => "reject_once",
        PermissionDecision::RejectAlways => "reject_always",
        PermissionDecision::Cancel => return None,
    };
    opts.iter()
        .find(|o| o.kind.eq_ignore_ascii_case(want) || o.option_id.contains(want))
        .or_else(|| {
            if matches!(
                decision,
                PermissionDecision::AllowOnce | PermissionDecision::AllowAlways
            ) {
                opts.iter().find(|o| {
                    o.kind.to_ascii_lowercase().contains("allow")
                        || o.name.to_ascii_lowercase().contains("allow")
                })
            } else {
                opts.iter().find(|o| {
                    o.kind.to_ascii_lowercase().contains("reject")
                        || o.name.to_ascii_lowercase().contains("deny")
                        || o.name.to_ascii_lowercase().contains("reject")
                })
            }
        })
        .map(|o| o.option_id.clone())
        .or_else(|| opts.first().map(|o| o.option_id.clone()))
}

fn handle_incoming(
    msg: Value,
    pending: &Arc<std::sync::Mutex<HashMap<u64, Pending>>>,
    permission_opts: &Arc<std::sync::Mutex<HashMap<u64, Vec<PermissionOption>>>>,
    event_tx: &mpsc::UnboundedSender<AcpEvent>,
) {
    // Response to our request
    if let Some(id) = msg.get("id").and_then(|v| v.as_u64()).or_else(|| {
        msg.get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
    }) {
        if msg.get("method").is_none() {
            if let Some(p) = pending.lock().unwrap().remove(&id) {
                if let Some(err) = msg.get("error") {
                    let _ = p.tx.send(Err(err.to_string()));
                } else {
                    let result = msg.get("result").cloned().unwrap_or(Value::Null);
                    let _ = p.tx.send(Ok(result));
                }
            }
            return;
        }
        // Server request (permission etc.)
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            if method == "session/request_permission" || method.ends_with("request_permission") {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let options = parse_permission_options(&params);
                permission_opts.lock().unwrap().insert(id, options.clone());
                let _ = event_tx.send(AcpEvent::Permission(PermissionRequest {
                    request_id: id,
                    session_id: params
                        .pointer("/sessionId")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    tool_name: params
                        .pointer("/toolCall/title")
                        .or_else(|| params.pointer("/toolCall/kind"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    summary: params
                        .pointer("/toolCall/rawInput")
                        .map(|v| v.to_string())
                        .or_else(|| {
                            params
                                .pointer("/toolCall/title")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| "Tool permission required".into()),
                    options,
                }));
                return;
            }
        }
    }

    // Notifications
    if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
        if method == "session/update" {
            if let Some(stream) = extract_stream(&msg) {
                let _ = event_tx.send(AcpEvent::Stream(stream));
            }
        }
    }
}

fn parse_permission_options(params: &Value) -> Vec<PermissionOption> {
    params
        .get("options")
        .and_then(|o| o.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| {
                    Some(PermissionOption {
                        option_id: o.get("optionId")?.as_str()?.to_string(),
                        name: o
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        kind: o
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_stream(msg: &Value) -> Option<StreamText> {
    let params = msg.get("params")?;
    let session_id = params.get("sessionId")?.as_str()?.to_string();
    let update = params.get("update")?;
    let session_update = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Agent message chunk
    if let Some(content) = update.get("content") {
        if let Some(text) = content.get("text").and_then(|t| t.as_str()) {
            if !text.is_empty() {
                return Some(StreamText {
                    session_id,
                    kind: if session_update.contains("thought") {
                        "thought".into()
                    } else {
                        "agent_text".into()
                    },
                    text: text.to_string(),
                });
            }
        }
    }

    // Tool call updates
    if session_update.contains("tool") || update.get("toolCallId").is_some() {
        let title = update
            .get("title")
            .or_else(|| update.pointer("/toolCall/title"))
            .and_then(|v| v.as_str())
            .unwrap_or("tool");
        return Some(StreamText {
            session_id,
            kind: "tool".into(),
            text: title.to_string(),
        });
    }

    None
}
