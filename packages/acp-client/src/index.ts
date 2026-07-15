/**
 * Lightweight ACP (Agent Client Protocol) types + NDJSON framing helpers.
 * Runtime transport lives in the Tauri host (Rust); this package is shared
 * typing / pure protocol utilities for the React UI and tests.
 */

export type JsonRpcId = string | number;

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: JsonRpcId;
  method: string;
  params?: unknown;
}

export interface JsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: unknown;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: JsonRpcId;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
}

export type JsonRpcMessage = JsonRpcRequest | JsonRpcNotification | JsonRpcResponse;

/** Encode one ACP message as a single newline-terminated line. */
export function encodeAcpLine(message: JsonRpcMessage): string {
  return `${JSON.stringify(message)}\n`;
}

/**
 * Parse complete NDJSON lines from a byte/string buffer.
 * Returns parsed messages and leftover incomplete trailing data.
 */
export function feedAcpLines(
  buffer: string,
  chunk: string,
): { messages: JsonRpcMessage[]; rest: string } {
  const combined = buffer + chunk;
  const parts = combined.split("\n");
  const rest = parts.pop() ?? "";
  const messages: JsonRpcMessage[] = [];
  for (const line of parts) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    messages.push(JSON.parse(trimmed) as JsonRpcMessage);
  }
  return { messages, rest };
}

export function isResponse(msg: JsonRpcMessage): msg is JsonRpcResponse {
  return "id" in msg && ("result" in msg || "error" in msg) && !("method" in msg);
}

export function isRequest(msg: JsonRpcMessage): msg is JsonRpcRequest {
  return "method" in msg && "id" in msg;
}

export function isNotification(msg: JsonRpcMessage): msg is JsonRpcNotification {
  return "method" in msg && !("id" in msg);
}

export interface SessionRow {
  id: string;
  title: string;
  cwd: string;
  status: "idle" | "working" | "needs_input" | "error";
  modelId?: string;
}

export interface StreamChunk {
  sessionId: string;
  kind: "agent_text" | "thought" | "tool" | "system" | "permission";
  text: string;
  raw?: unknown;
}
