# Grok Desktop

Integrated desktop app (Tauri 2 + React + TypeScript) that embeds a **bundled** Grok Build agent and speaks **ACP over stdio**.

## Architecture

- `src/` — React command-center UI
- `src-tauri/` — Tauri host: agent lifecycle, ACP client, config/secrets/trust
- `src-tauri/resources/agent/grok-agent.exe` — bundled agent (not PATH-required)
- `../../packages/acp-client` — shared NDJSON ACP helpers

See `docs/DESKTOP_DEV.md` and `docs/UPSTREAM_SYNC.md`.

## Dev

```bash
cd apps/desktop
npm install
npm run desktop:dev
```

Optional: `GROK_DESKTOP_AGENT_PATH` points at a custom agent binary (dev only).

## Test

```bash
cd apps/desktop/src-tauri
cargo test
cd ..
npm run test:unit
```

## Package

```bash
cd apps/desktop
npm run desktop:build
```
