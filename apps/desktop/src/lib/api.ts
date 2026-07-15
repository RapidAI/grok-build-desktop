import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface AgentInfo {
  path: string;
  fromEnvOverride: boolean;
  home: string;
  workspace: string;
}

export interface DesktopSession {
  id: string;
  title: string;
  cwd: string;
  status: string;
  modelId?: string | null;
  transcript: { role: string; text: string; kind: string }[];
}

export interface TrustStatus {
  cwd: string;
  trusted: boolean;
  reason: string;
}

export interface ModelEntry {
  id: string;
  model: string;
  name?: string | null;
  base_url?: string | null;
  api_backend: string;
  env_key?: string | null;
  has_plain_api_key: boolean;
}

export interface PermissionRequest {
  request_id: number;
  session_id?: string | null;
  tool_name?: string | null;
  summary: string;
  options: { option_id: string; name: string; kind: string }[];
}

export interface StreamChunk {
  session_id: string;
  kind: string;
  text: string;
}

export const api = {
  resolveAgentInfo: () => invoke<AgentInfo>("resolve_agent_info"),
  startAgent: () => invoke<AgentInfo>("start_agent"),
  setWorkspace: (path: string) => invoke<TrustStatus>("set_workspace", { path }),
  getTrust: () => invoke<TrustStatus>("get_trust"),
  grantTrust: () => invoke<TrustStatus>("grant_workspace_trust"),
  newSession: (modelId?: string, title?: string) =>
    invoke<DesktopSession>("new_session", { modelId, title }),
  resumeSession: (sessionId: string) =>
    invoke<DesktopSession>("resume_session", { sessionId }),
  sendPrompt: (sessionId: string, text: string) =>
    invoke<unknown>("send_prompt", { sessionId, text }),
  listSessions: () => invoke<DesktopSession[]>("list_sessions"),
  setActiveSession: (sessionId: string) =>
    invoke<DesktopSession>("set_active_session", { sessionId }),
  registerSessionRow: (title: string, cwd?: string) =>
    invoke<DesktopSession>("register_session_row", { title, cwd }),
  respondPermission: (requestId: number, decision: string) =>
    invoke<void>("respond_permission", { requestId, decision }),
  listModels: () => invoke<ModelEntry[]>("list_models"),
  upsertModel: (req: Record<string, unknown>) => invoke<void>("upsert_model", { req }),
  listDiskSessions: () => invoke<unknown[]>("list_disk_sessions"),
  appendTranscript: (sessionId: string, role: string, text: string, kind: string) =>
    invoke<void>("append_transcript", { sessionId, role, text, kind }),
};

export function onStream(cb: (c: StreamChunk) => void): Promise<UnlistenFn> {
  return listen<StreamChunk>("acp://stream", (e) => cb(e.payload));
}

export function onPermission(cb: (p: PermissionRequest) => void): Promise<UnlistenFn> {
  return listen<PermissionRequest>("acp://permission", (e) => cb(e.payload));
}

export function onError(cb: (m: string) => void): Promise<UnlistenFn> {
  return listen<string>("acp://error", (e) => cb(e.payload));
}

/** Pure helper used by UI tests — multi-session overview needs ≥2 rows. */
export function canShowOverview(sessions: { id: string }[]): boolean {
  return sessions.length >= 2;
}
