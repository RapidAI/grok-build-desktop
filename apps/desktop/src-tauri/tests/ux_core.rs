//! Permission / resume / trust gates on host-side paths used by the desktop app.

use std::fs;

use grok_desktop_lib::{
    grant_trust, workspace_trust_status, host_upsert_model, ModelUpsert,
};
use tempfile::TempDir;

#[test]
fn trust_gate_blocks_silent_full_power() {
    let home = TempDir::new().unwrap();
    let proj = home.path().join("untrusted-proj");
    fs::create_dir_all(&proj).unwrap();
    let st = workspace_trust_status(home.path(), &proj);
    assert!(!st.trusted, "must not be trusted by default");
    assert!(!st.reason.is_empty());

    let st2 = grant_trust(home.path(), &proj).unwrap();
    assert!(st2.trusted);
}

#[test]
fn resume_reads_session_summary_layout() {
    let home = TempDir::new().unwrap();
    let sess = home
        .path()
        .join(".grok")
        .join("sessions")
        .join("encoded-cwd")
        .join("sess-123");
    fs::create_dir_all(&sess).unwrap();
    fs::write(
        sess.join("summary.json"),
        r#"{"title":"prior work","modelId":"grok-build"}"#,
    )
    .unwrap();
    let rows = grok_desktop_lib::config_models::list_session_summaries(home.path()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["sessionId"], "sess-123");
    assert_eq!(rows[0]["title"], "prior work");
}

#[test]
fn deny_permission_decision_maps() {
    // pick_option_id is private; exercise respond path via decision enum serialization surface
    let d = grok_desktop_lib::acp::PermissionDecision::RejectOnce;
    let s = serde_json::to_string(&d).unwrap();
    assert!(s.contains("reject_once") || s.contains("RejectOnce"));
}

#[test]
fn secret_upsert_does_not_write_api_key() {
    let home = TempDir::new().unwrap();
    host_upsert_model(
        home.path(),
        &ModelUpsert {
            id: "x".into(),
            model: "m".into(),
            name: None,
            base_url: "http://127.0.0.1:1/v1".into(),
            api_backend: "chat_completions".into(),
            env_key: Some("NO_PLAIN".into()),
            secret: Some("secret".into()),
        },
    )
    .unwrap();
    let text = fs::read_to_string(home.path().join(".grok/config.toml")).unwrap();
    assert!(!text.contains("api_key"));
}
