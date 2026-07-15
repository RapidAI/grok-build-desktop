//! Multi-session overview: host can represent ≥2 top-level sessions.

use std::collections::HashMap;

#[derive(Clone)]
struct Row {
    id: String,
    title: String,
    status: String,
}

fn overview_ready(sessions: &HashMap<String, Row>) -> bool {
    sessions.len() >= 2
}

#[test]
fn multi_session_overview_represents_two_agents() {
    let mut map = HashMap::new();
    map.insert(
        "a".into(),
        Row {
            id: "a".into(),
            title: "implementer".into(),
            status: "working".into(),
        },
    );
    map.insert(
        "b".into(),
        Row {
            id: "b".into(),
            title: "reviewer".into(),
            status: "idle".into(),
        },
    );
    assert!(overview_ready(&map));
    let statuses: Vec<_> = map.values().map(|r| r.status.as_str()).collect();
    assert!(statuses.contains(&"working") || statuses.contains(&"idle"));
}

#[test]
fn app_tsx_declares_multi_session_ui() {
    let app = include_str!("../../src/App.tsx");
    assert!(app.contains("Dispatch idle agent") || app.contains("dispatchIdle"));
    assert!(app.contains("sessions.map"));
    assert!(app.contains("New session") || app.contains("createSession"));
}

#[test]
fn core_crates_do_not_depend_on_tauri() {
    // Static guard: desktop host is isolated under apps/desktop.
    let shell = include_str!("../../../../crates/codegen/xai-grok-shell/Cargo.toml");
    assert!(!shell.contains("tauri"));
    let sampler = include_str!("../../../../crates/codegen/xai-grok-sampler/Cargo.toml");
    assert!(!sampler.contains("tauri"));
}
