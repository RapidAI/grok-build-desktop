pub mod acp;
pub mod agent_path;
pub mod config_models;
pub mod secrets;
pub mod trust;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

use acp::{AcpClient, AcpEvent, PermissionDecision, SessionInfo};
use config_models::{ModelEntryView, UpsertModelRequest};

/// Desktop session row for multi-agent overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSession {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub status: String,
    pub model_id: Option<String>,
    pub transcript: Vec<TranscriptLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    pub role: String,
    pub text: String,
    pub kind: String,
}

struct InnerState {
    home: PathBuf,
    workspace: PathBuf,
    client: Option<Arc<AcpClient>>,
    sessions: HashMap<String, DesktopSession>,
    active_session: Option<String>,
    initialized: bool,
}

pub struct AppState {
    inner: Mutex<InnerState>,
    runtime: tokio::runtime::Handle,
}

impl AppState {
    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let workspace = std::env::current_dir().unwrap_or_else(|_| home.clone());
        // Ensure a multi-thread runtime exists for spawn (Tauri 2 provides one).
        let runtime = tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
            // Tests / early init
            static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
            RT.get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("runtime")
            })
            .handle()
            .clone()
        });
        Self {
            inner: Mutex::new(InnerState {
                home,
                workspace,
                client: None,
                sessions: HashMap::new(),
                active_session: None,
                initialized: false,
            }),
            runtime,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub path: String,
    pub from_env_override: bool,
    pub home: String,
    pub workspace: String,
}

#[tauri::command]
fn resolve_agent_info(state: State<'_, AppState>) -> Result<AgentInfo, String> {
    let path = agent_path::resolve_agent_binary()?;
    let g = state.inner.lock();
    Ok(AgentInfo {
        path: path.display().to_string(),
        from_env_override: agent_path::is_env_override(),
        home: g.home.display().to_string(),
        workspace: g.workspace.display().to_string(),
    })
}

#[tauri::command]
async fn start_agent(app: AppHandle, state: State<'_, AppState>) -> Result<AgentInfo, String> {
    let (home, workspace) = {
        let g = state.inner.lock();
        (g.home.clone(), g.workspace.clone())
    };
    config_models::ensure_grok_home(&home)?;

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AcpEvent>();
    let client = AcpClient::spawn(&workspace, &home, &[], event_tx).await?;
    let agent_path = client.agent_path().display().to_string();

    let init = client.initialize().await?;
    let _ = client.authenticate_api_key().await;

    {
        let mut g = state.inner.lock();
        g.client = Some(Arc::new(client));
        g.initialized = true;
    }

    let app2 = app.clone();
    let state_sessions = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            match &ev {
                AcpEvent::Stream(s) => {
                    let _ = app2.emit("acp://stream", s);
                    // Also mirror into session transcript via event payload only;
                    // UI owns append. Host keeps lightweight cache via command.
                }
                AcpEvent::Permission(p) => {
                    let _ = app2.emit("acp://permission", p);
                }
                AcpEvent::Error { message } => {
                    let _ = app2.emit("acp://error", message);
                }
                AcpEvent::AgentExited { code } => {
                    let _ = app2.emit("acp://agent-exited", code);
                }
                AcpEvent::SessionClosed { session_id } => {
                    let _ = state_sessions.emit("acp://session-closed", session_id);
                }
            }
        }
    });

    let _ = init; // available for capability probe
    Ok(AgentInfo {
        path: agent_path,
        from_env_override: agent_path::is_env_override(),
        home: home.display().to_string(),
        workspace: workspace.display().to_string(),
    })
}

#[tauri::command]
fn set_workspace(state: State<'_, AppState>, path: String) -> Result<trust::TrustStatus, String> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(format!("not a directory: {path}"));
    }
    let mut g = state.inner.lock();
    g.workspace = dunce::canonicalize(&p).unwrap_or(p);
    Ok(trust::workspace_trust_status(&g.home, &g.workspace))
}

#[tauri::command]
fn get_trust(state: State<'_, AppState>) -> trust::TrustStatus {
    let g = state.inner.lock();
    trust::workspace_trust_status(&g.home, &g.workspace)
}

#[tauri::command]
fn grant_workspace_trust(state: State<'_, AppState>) -> Result<trust::TrustStatus, String> {
    let g = state.inner.lock();
    trust::grant_trust(&g.home, &g.workspace)
}

fn client_arc(state: &AppState) -> Result<Arc<AcpClient>, String> {
    state
        .inner
        .lock()
        .client
        .clone()
        .ok_or_else(|| "agent not started".into())
}

#[tauri::command]
async fn new_session(
    state: State<'_, AppState>,
    model_id: Option<String>,
    title: Option<String>,
) -> Result<DesktopSession, String> {
    let client = client_arc(&state)?;
    let cwd = state.inner.lock().workspace.clone();
    let trust = trust::workspace_trust_status(&state.inner.lock().home, &cwd);
    // Untrusted workspaces still may open sessions (read-only exploration),
    // but UI must show the gate — we attach status note.
    let sid = client
        .session_new(&cwd, model_id.as_deref())
        .await?;
    let session = DesktopSession {
        id: sid.clone(),
        title: title.unwrap_or_else(|| format!("Session {}", &sid[..8.min(sid.len())])),
        cwd: cwd.display().to_string(),
        status: if trust.trusted {
            "idle".into()
        } else {
            "needs_input".into()
        },
        model_id,
        transcript: if trust.trusted {
            vec![]
        } else {
            vec![TranscriptLine {
                role: "system".into(),
                text: trust.reason.clone(),
                kind: "system".into(),
            }]
        },
    };
    let mut g = state.inner.lock();
    g.sessions.insert(sid.clone(), session.clone());
    g.active_session = Some(sid);
    Ok(session)
}

#[tauri::command]
async fn resume_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<DesktopSession, String> {
    let client = client_arc(&state)?;
    let cwd = state.inner.lock().workspace.clone();
    let _ = client.session_load(&session_id, &cwd).await?;
    let session = DesktopSession {
        id: session_id.clone(),
        title: format!("Resumed {}", &session_id[..8.min(session_id.len())]),
        cwd: cwd.display().to_string(),
        status: "idle".into(),
        model_id: None,
        transcript: vec![TranscriptLine {
            role: "system".into(),
            text: format!("Resumed session {session_id}"),
            kind: "system".into(),
        }],
    };
    let mut g = state.inner.lock();
    g.sessions.insert(session_id.clone(), session.clone());
    g.active_session = Some(session_id);
    Ok(session)
}

#[tauri::command]
async fn send_prompt(
    state: State<'_, AppState>,
    session_id: String,
    text: String,
) -> Result<serde_json::Value, String> {
    let client = client_arc(&state)?;
    {
        let mut g = state.inner.lock();
        if let Some(s) = g.sessions.get_mut(&session_id) {
            s.status = "working".into();
            s.transcript.push(TranscriptLine {
                role: "user".into(),
                text: text.clone(),
                kind: "user".into(),
            });
        }
    }
    let result = client.session_prompt(&session_id, &text).await;
    {
        let mut g = state.inner.lock();
        if let Some(s) = g.sessions.get_mut(&session_id) {
            s.status = if result.is_ok() {
                "idle".into()
            } else {
                "error".into()
            };
        }
    }
    result
}

#[tauri::command]
fn append_transcript(
    state: State<'_, AppState>,
    session_id: String,
    role: String,
    text: String,
    kind: String,
) -> Result<(), String> {
    let mut g = state.inner.lock();
    if let Some(s) = g.sessions.get_mut(&session_id) {
        s.transcript.push(TranscriptLine { role, text, kind });
    }
    Ok(())
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Vec<DesktopSession> {
    let g = state.inner.lock();
    let mut v: Vec<_> = g.sessions.values().cloned().collect();
    v.sort_by(|a, b| a.title.cmp(&b.title));
    v
}

#[tauri::command]
fn get_active_session(state: State<'_, AppState>) -> Option<DesktopSession> {
    let g = state.inner.lock();
    g.active_session
        .as_ref()
        .and_then(|id| g.sessions.get(id).cloned())
}

#[tauri::command]
fn set_active_session(state: State<'_, AppState>, session_id: String) -> Result<DesktopSession, String> {
    let mut g = state.inner.lock();
    if !g.sessions.contains_key(&session_id) {
        return Err("unknown session".into());
    }
    g.active_session = Some(session_id.clone());
    Ok(g.sessions.get(&session_id).cloned().unwrap())
}

/// Create a local overview session row without round-tripping agent (for multi-session UI tests).
#[tauri::command]
fn register_session_row(
    state: State<'_, AppState>,
    title: String,
    cwd: Option<String>,
) -> DesktopSession {
    let id = Uuid::new_v4().to_string();
    let mut g = state.inner.lock();
    let session = DesktopSession {
        id: id.clone(),
        title,
        cwd: cwd.unwrap_or_else(|| g.workspace.display().to_string()),
        status: "idle".into(),
        model_id: None,
        transcript: vec![],
    };
    g.sessions.insert(id, session.clone());
    if g.active_session.is_none() {
        g.active_session = Some(session.id.clone());
    }
    session
}

#[tauri::command]
async fn respond_permission(
    state: State<'_, AppState>,
    request_id: u64,
    decision: String,
) -> Result<(), String> {
    let client = client_arc(&state)?;
    let d = match decision.as_str() {
        "allow_once" => PermissionDecision::AllowOnce,
        "allow_always" => PermissionDecision::AllowAlways,
        "reject_once" => PermissionDecision::RejectOnce,
        "reject_always" => PermissionDecision::RejectAlways,
        _ => PermissionDecision::Cancel,
    };
    client.respond_permission(request_id, d).await
}

#[tauri::command]
fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelEntryView>, String> {
    let home = state.inner.lock().home.clone();
    config_models::list_models(&home)
}

#[tauri::command]
fn upsert_model(state: State<'_, AppState>, req: UpsertModelRequest) -> Result<(), String> {
    let home = state.inner.lock().home.clone();
    config_models::upsert_model(&home, &req)
}

#[tauri::command]
fn list_disk_sessions(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let home = state.inner.lock().home.clone();
    config_models::list_session_summaries(&home)
}

#[tauri::command]
fn set_home_for_tests(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let p = PathBuf::from(path);
    std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    state.inner.lock().home = p;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,grok_desktop=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            resolve_agent_info,
            start_agent,
            set_workspace,
            get_trust,
            grant_workspace_trust,
            new_session,
            resume_session,
            send_prompt,
            append_transcript,
            list_sessions,
            get_active_session,
            set_active_session,
            register_session_row,
            respond_permission,
            list_models,
            upsert_model,
            list_disk_sessions,
            set_home_for_tests,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Grok Desktop");
}

// Re-exports for integration tests
pub use acp::AcpClient as HostAcpClient;
pub use agent_path::resolve_agent_binary;
pub use config_models::{UpsertModelRequest as ModelUpsert, upsert_model as host_upsert_model};
pub use trust::{grant_trust, workspace_trust_status};
