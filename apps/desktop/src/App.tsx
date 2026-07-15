import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  type AgentInfo,
  type DesktopSession,
  type ModelEntry,
  type PermissionRequest,
  type TrustStatus,
} from "./lib/api";
import { onError, onPermission, onStream } from "./lib/api";

export default function App() {
  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sessions, setSessions] = useState<DesktopSession[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [trust, setTrust] = useState<TrustStatus | null>(null);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [modelId, setModelId] = useState<string>("grok-build");
  const [perm, setPerm] = useState<PermissionRequest | null>(null);
  const [streamBuf, setStreamBuf] = useState<Record<string, string>>({});

  const active = useMemo(
    () => sessions.find((s) => s.id === activeId) ?? null,
    [sessions, activeId],
  );

  const refreshSessions = useCallback(async () => {
    const list = await api.listSessions();
    setSessions(list);
  }, []);

  useEffect(() => {
    let unsubs: Array<() => void> = [];
    (async () => {
      try {
        setAgent(await api.resolveAgentInfo());
        setTrust(await api.getTrust());
        setModels(await api.listModels());
      } catch (e) {
        setError(String(e));
      }
      unsubs.push(
        await onStream((c) => {
          setStreamBuf((prev) => ({
            ...prev,
            [c.session_id]: (prev[c.session_id] ?? "") + c.text,
          }));
          void api
            .appendTranscript(c.session_id, "assistant", c.text, c.kind)
            .then(refreshSessions);
        }),
        await onPermission((p) => setPerm(p)),
        await onError((m) => setError(m)),
      );
    })();
    return () => {
      unsubs.forEach((u) => u());
    };
  }, [refreshSessions]);

  async function start() {
    setBusy(true);
    setError(null);
    try {
      const info = await api.startAgent();
      setAgent(info);
      setTrust(await api.getTrust());
      setModels(await api.listModels());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function createSession() {
    setBusy(true);
    setError(null);
    try {
      const s = await api.newSession(modelId, `Agent ${sessions.length + 1}`);
      setActiveId(s.id);
      await refreshSessions();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function dispatchIdle() {
    // Second top-level session for multi-agent overview without prompt.
    const s = await api.registerSessionRow(`Idle ${sessions.length + 1}`);
    setActiveId(s.id);
    await refreshSessions();
  }

  async function send() {
    if (!active || !draft.trim()) return;
    setBusy(true);
    setError(null);
    const text = draft;
    setDraft("");
    try {
      await api.sendPrompt(active.id, text);
      await refreshSessions();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function addProvider(backend: "chat_completions" | "responses") {
    const id = backend === "chat_completions" ? "desktop-chat" : "desktop-responses";
    await api.upsertModel({
      id,
      model: "mock-model",
      name: backend === "chat_completions" ? "Desktop Chat" : "Desktop Responses",
      base_url: "http://127.0.0.1:0/v1",
      api_backend: backend,
      env_key: backend === "chat_completions" ? "DESKTOP_CHAT_KEY" : "DESKTOP_RESP_KEY",
      secret: "sk-desktop-test",
    });
    setModels(await api.listModels());
    setModelId(id);
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <h1>Grok Desktop</h1>
        <h2>Agents</h2>
        {sessions.map((s) => (
          <div
            key={s.id}
            className={`session-item ${s.id === activeId ? "active" : ""}`}
            onClick={() => {
              setActiveId(s.id);
              void api.setActiveSession(s.id);
            }}
          >
            <div>{s.title}</div>
            <div className="meta">
              <span className="badge">{s.status}</span> {s.cwd}
            </div>
          </div>
        ))}
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 12 }}>
          <button disabled={busy} onClick={() => void createSession()}>
            New session
          </button>
          <button className="secondary" disabled={busy} onClick={() => void dispatchIdle()}>
            Dispatch idle agent
          </button>
        </div>
      </aside>

      <main className="main">
        <div className="topbar">
          <button disabled={busy} onClick={() => void start()}>
            Start agent
          </button>
          <button className="secondary" disabled={busy} onClick={() => void createSession()}>
            New chat
          </button>
          {trust && (
            <span className={`badge ${trust.trusted ? "ok" : "warn"}`}>
              {trust.trusted ? "Trusted workspace" : "Untrusted workspace"}
            </span>
          )}
          {!trust?.trusted && (
            <button className="secondary" onClick={() => void api.grantTrust().then(setTrust)}>
              Grant trust
            </button>
          )}
          <select value={modelId} onChange={(e) => setModelId(e.target.value)}>
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name || m.id} ({m.api_backend})
              </option>
            ))}
          </select>
          {agent && (
            <code title={agent.path}>
              agent: {agent.path.split(/[/\\]/).pop()}
              {agent.fromEnvOverride ? " (env)" : " (bundled)"}
            </code>
          )}
        </div>

        {error && (
          <div className="bubble system" style={{ margin: 12, borderColor: "var(--danger)" }}>
            {error}
          </div>
        )}

        {perm && (
          <div className="perm" style={{ margin: 12 }}>
            <strong>Permission required</strong>
            <div>{perm.tool_name || "tool"}</div>
            <pre style={{ whiteSpace: "pre-wrap" }}>{perm.summary}</pre>
            <div style={{ display: "flex", gap: 8 }}>
              <button
                onClick={() => {
                  void api.respondPermission(perm.request_id, "allow_once").then(() => setPerm(null));
                }}
              >
                Allow once
              </button>
              <button
                className="danger"
                onClick={() => {
                  void api
                    .respondPermission(perm.request_id, "reject_once")
                    .then(() => setPerm(null));
                }}
              >
                Deny
              </button>
            </div>
          </div>
        )}

        <div className="transcript">
          {active?.transcript.map((line, i) => (
            <div key={i} className={`bubble ${line.role === "user" ? "user" : line.kind}`}>
              <div className="meta" style={{ marginBottom: 4 }}>
                {line.role} · {line.kind}
              </div>
              {line.text}
            </div>
          ))}
          {active && streamBuf[active.id] && (
            <div className="bubble">
              <div className="meta">assistant · streaming</div>
              {streamBuf[active.id]}
            </div>
          )}
          {!active && <div className="bubble system">Start the agent and open a session.</div>}
        </div>

        <div className="composer">
          <textarea
            value={draft}
            placeholder="Message Grok…"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
          />
          <button disabled={busy || !active} onClick={() => void send()}>
            Send
          </button>
        </div>
      </main>

      <aside className="right">
        <h2>Models / Providers</h2>
        <p style={{ color: "var(--muted)", fontSize: 13 }}>
          Native Grok plus third-party OpenAI Chat Completions and Responses via shared config.
          Secrets use env_key / keyring — not plain api_key in TOML.
        </p>
        <button className="secondary" onClick={() => void addProvider("chat_completions")}>
          Add Chat Completions provider
        </button>
        <div style={{ height: 8 }} />
        <button className="secondary" onClick={() => void addProvider("responses")}>
          Add Responses provider
        </button>
        <ul style={{ paddingLeft: 18, fontSize: 13 }}>
          {models.map((m) => (
            <li key={m.id}>
              <strong>{m.name || m.id}</strong> — {m.api_backend}
              {m.env_key ? ` · env:${m.env_key}` : ""}
              {m.has_plain_api_key ? " · ⚠ plain api_key" : ""}
            </li>
          ))}
        </ul>
        <h2>Resume from disk</h2>
        <button
          className="secondary"
          onClick={async () => {
            const rows = await api.listDiskSessions();
            if (!rows.length) {
              setError("No sessions on disk under ~/.grok/sessions");
              return;
            }
            const id =
              (rows[0] as { sessionId?: string }).sessionId ||
              (rows[0] as { id?: string }).id;
            if (!id) {
              setError("summary missing session id");
              return;
            }
            const s = await api.resumeSession(String(id));
            setActiveId(s.id);
            await refreshSessions();
          }}
        >
          Resume latest disk session
        </button>
      </aside>
    </div>
  );
}
