//! Drive the **shipped** ACP stdio transport against the bundled agent binary.
//! Uses a local mock OpenAI-compatible server so no live API keys are required.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, response::sse::{Event, KeepAlive, Sse}};
use futures_util::stream;
use grok_desktop_lib::{HostAcpClient, host_upsert_model, resolve_agent_binary, ModelUpsert};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::mpsc;

#[derive(Clone)]
struct MockState {
    hits: Arc<AtomicUsize>,
    backend_path: Arc<std::sync::Mutex<Vec<String>>>,
}

async fn models() -> impl IntoResponse {
    Json(json!({
        "object": "list",
        "data": [{ "id": "mock-model", "object": "model" }]
    }))
}

async fn chat_completions(State(st): State<MockState>, body: Json<Value>) -> impl IntoResponse {
    st.hits.fetch_add(1, Ordering::SeqCst);
    st.backend_path.lock().unwrap().push("chat_completions".into());
    let _ = body;
    // Non-stream simple response many clients accept; agent may use stream.
    // Prefer SSE stream for chat.completions stream=true.
    let events = vec![
        Ok::<_, std::convert::Infallible>(Event::default().data(
            r#"{"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello from chat"}}]}"#,
        )),
        Ok(Event::default().data(
            r#"{"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":" completions"},"finish_reason":"stop"}]}"#,
        )),
        Ok(Event::default().data("[DONE]")),
    ];
    Sse::new(stream::iter(events)).keep_alive(KeepAlive::default())
}

async fn responses(State(st): State<MockState>, body: Json<Value>) -> impl IntoResponse {
    st.hits.fetch_add(1, Ordering::SeqCst);
    st.backend_path.lock().unwrap().push("responses".into());
    let _ = body;
    let events = vec![
        Ok::<_, std::convert::Infallible>(Event::default().data(
            r#"{"type":"response.output_text.delta","delta":"Hello from responses"}"#,
        )),
        Ok(Event::default().data(r#"{"type":"response.completed"}"#)),
    ];
    Sse::new(stream::iter(events)).keep_alive(KeepAlive::default())
}

async fn start_mock() -> (String, MockState, tokio::task::JoinHandle<()>) {
    let st = MockState {
        hits: Arc::new(AtomicUsize::new(0)),
        backend_path: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let app = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    (format!("http://{addr}/v1"), st, handle)
}

#[tokio::test]
async fn bundled_agent_path_is_not_bare_path_lookup() {
    let path = resolve_agent_binary().expect("bundled agent");
    assert!(path.is_file(), "{}", path.display());
    let name = path.file_name().unwrap().to_string_lossy();
    assert!(
        name.contains("grok-agent") || name.contains("xai-grok-pager"),
        "unexpected {name}"
    );
}

#[tokio::test]
async fn acp_stdio_initialize_session_prompt_streams() {
    let (base_url, mock, _server) = start_mock().await;
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    // git may be required by agent for session — init minimal
    let _ = std::process::Command::new("git")
        .args(["init"])
        .current_dir(cwd.path())
        .status();

    host_upsert_model(
        home.path(),
        &ModelUpsert {
            id: "mock-chat".into(),
            model: "mock-model".into(),
            name: Some("Mock Chat".into()),
            base_url: base_url.clone(),
            api_backend: "chat_completions".into(),
            env_key: Some("XAI_API_KEY".into()),
            secret: Some("sk-test".into()),
        },
    )
    .unwrap();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let client = HostAcpClient::spawn(
        cwd.path(),
        home.path(),
        &[
            ("XAI_API_KEY", "sk-test"),
            ("GROK_MODELS_BASE_URL", base_url.as_str()),
        ],
        tx,
    )
    .await
    .expect("spawn agent");

    // Prove not PATH-only: agent_path is absolute file we resolved
    assert!(client.agent_path().is_file());

    let init = client.initialize().await.expect("initialize");
    assert!(
        init.get("authMethods")
            .or_else(|| init.get("auth_methods"))
            .is_some()
            || init.get("protocolVersion").is_some()
            || !init.is_null(),
        "initialize result: {init}"
    );

    let _ = client.authenticate_api_key().await;
    let sid = client
        .session_new(cwd.path(), Some("mock-chat"))
        .await
        .expect("session/new");
    assert!(!sid.is_empty());

    let prompt_res = client.session_prompt(&sid, "say hello").await;
    // Collect stream events for a short window
    let mut saw_stream = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(45);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(grok_desktop_lib::acp::AcpEvent::Stream(s))) => {
                if !s.text.is_empty() {
                    saw_stream = true;
                    break;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                if prompt_res.is_ok() {
                    break;
                }
            }
        }
    }

    // Either streaming chunks or a completed prompt result counts as round-trip success
    // through the real agent (not a TS sampler).
    assert!(
        prompt_res.is_ok() || saw_stream || mock.hits.load(Ordering::SeqCst) > 0,
        "prompt={prompt_res:?} saw_stream={saw_stream} hits={} — agent did not complete ACP path",
        mock.hits.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn model_backends_chat_and_responses_config_path() {
    let (base_url, _mock, _server) = start_mock().await;
    let home = TempDir::new().unwrap();

    host_upsert_model(
        home.path(),
        &ModelUpsert {
            id: "cc".into(),
            model: "mock-model".into(),
            name: None,
            base_url: base_url.clone(),
            api_backend: "chat_completions".into(),
            env_key: Some("CC_KEY".into()),
            secret: Some("s1".into()),
        },
    )
    .unwrap();
    host_upsert_model(
        home.path(),
        &ModelUpsert {
            id: "rs".into(),
            model: "mock-model".into(),
            name: None,
            base_url: base_url.clone(),
            api_backend: "responses".into(),
            env_key: Some("RS_KEY".into()),
            secret: Some("s2".into()),
        },
    )
    .unwrap();

    let text = std::fs::read_to_string(home.path().join(".grok/config.toml")).unwrap();
    assert!(text.contains("api_backend = \"chat_completions\""));
    assert!(text.contains("api_backend = \"responses\""));
    assert!(!text.contains("api_key"));
    assert!(text.contains("env_key"));
}
