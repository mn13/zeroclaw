# ZeroClaw System Overview

ZeroClaw is a Rust-first autonomous agent runtime. It runs LLM-powered agents that can use tools, maintain memory, and communicate over multiple channels. This document describes the high-level architecture and the interaction flow between components.

## Components

| Component | Crate / Directory | Role |
|-----------|-------------------|------|
| **ZeroClaw Agent** (`zc`) | `src/` | The autonomous agent runtime. Runs as a standalone process exposing a gRPC service. Handles LLM inference, tool execution, memory, and security. |
| **ZeroClaw Gateway** (`zcgw`) | `zcgw/` | A lightweight multi-instance orchestrator. Exposes an HTTP/WebSocket API that proxies to one or more agent instances via gRPC. Manages Docker containers for agents. |
| **Web UI** | `web/` | A React single-page application served by the gateway. Provides a chat interface, admin panel, and configuration UI for agent instances. |
| **Docker setup** | `docker/` | Dockerfiles and compose configuration for deploying the full stack. The gateway manages agent containers via the Docker socket. |

## High-Level Interaction Flow

```
 User (browser)
      |
      | HTTP / WebSocket
      v
 ┌──────────────────────────────┐
 │    ZeroClaw Gateway (zcgw)   │
 │                              │
 │  - Auth middleware            │
 │  - REST API (instance mgmt)  │
 │  - WebSocket (real-time chat)│
 │  - Docker container mgmt    │
 │  - Static file serving (SPA) │
 │  - Health check loop         │
 └───────────┬──────────────────┘
             │ gRPC (protobuf)
             v
 ┌──────────────────────────────┐
 │  ZeroClaw Agent (zc)         │
 │                              │
 │  - Session manager (actor)   │
 │  - Agent loop                │
 │    - LLM provider call       │
 │    - Tool execution          │
 │    - History auto-compaction │
 │  - History store (SQLite)    │
 │  - Memory backends           │
 │  - Security policy           │
 │  - Observability / events    │
 └───────────┬──────────────────┘
             │
             v
 ┌──────────────────────────────┐
 │  LLM Provider (external)     │
 │  OpenRouter / Anthropic /    │
 │  OpenAI / Venice / etc.      │
 └──────────────────────────────┘
```

## Message Lifecycle

A complete user-to-response cycle works as follows:

1. **User sends a message** via the Web UI. The frontend sends a JSON message over WebSocket: `{"type": "message", "content": "..."}`.

2. **Gateway receives the WebSocket message** (`zcgw/src/ws.rs`). It looks up the target agent instance in its registry and establishes a gRPC connection.

3. **Gateway calls `SendMessage` RPC** on the agent. This is a server-streaming RPC — the gateway receives a stream of `ChatOutput` frames.

4. **Agent's `SessionManager` receives the command** via an internal actor channel. It enriches the user message with recent conversation history (loaded from the SQLite history store) to provide context continuity.

5. **Agent loop runs** (`src/agent/loop_.rs`). This is the core orchestration:
   - Builds a system prompt from the agent's identity and configuration.
   - Calls the configured LLM provider (OpenRouter, Anthropic, etc.) with the message, conversation context, and available tool definitions.
   - If the LLM requests tool calls, executes them (shell, file read/write, memory operations, browser, etc.), appends results, and loops back to the LLM. This continues up to `max_tool_iterations` (default: 10).
   - Streams text deltas back to the session as they arrive from the LLM.
   - When complete, returns the final response with token usage.

6. **Session manager saves to history** — both the user message and assistant response are appended to the SQLite history store with turn index.

7. **gRPC stream sends frames back to gateway** — `TurnStarted`, `Delta` (streaming text), `ToolStart`/`ToolResult` (tool use), and finally `Done` (with full content and token counts).

8. **Gateway translates gRPC frames to JSON** and forwards them over the WebSocket to the frontend.

9. **Web UI renders the response** — streaming deltas are accumulated into the assistant message bubble, tool calls are displayed with their status, and token usage is shown.

## Authentication

- **Gateway API**: Protected by a bearer token (`ZCGW_AUTH_TOKEN`). All `/api/*` and `/ws/*` endpoints require the token via `Authorization: Bearer <token>` header or `?token=<token>` query parameter. The `/health` endpoint and static assets are public.
- **Agent gRPC**: Protected by a separate secret (`ZCGW_GRPC_SECRET`). The gateway attaches this as a bearer token in gRPC metadata on every call to agent instances.

## Dual Gateway Architecture

ZeroClaw has two gateway layers that serve different purposes:

| Layer | Location | Purpose |
|-------|----------|---------|
| **zcgw** (ZeroClaw Gateway) | `zcgw/` | Multi-instance orchestrator. Manages multiple agent containers, provides a unified REST/WebSocket API and Web UI. Communicates with agents over gRPC. |
| **Built-in gateway** | `src/gateway/` | Per-agent webhook server. Handles inbound webhooks (Telegram, Discord, WhatsApp, etc.), serves a single-agent dashboard API, and provides SSE for real-time events. Runs inside each agent process. |

In a typical Docker deployment, only the `zcgw` gateway is exposed to the outside world. The built-in gateway is used when running a single agent standalone or for direct channel integrations.

## Key Design Decisions

- **Actor model for sessions**: Each agent instance runs a single actor loop that processes messages sequentially. This prevents concurrent mutations to conversation state and ensures tool execution is serialized.
- **gRPC between gateway and agents**: Enables language-agnostic agent implementations and clean process isolation. Server-streaming RPCs provide natural support for real-time deltas.
- **Docker container management**: The gateway can dynamically create, start, stop, restart, and destroy agent containers. Each agent gets its own isolated filesystem and configuration.
- **History compaction**: To prevent unbounded context growth, the agent automatically compacts conversation history when it exceeds `max_history_messages` — it summarizes older messages and keeps only the most recent ones.
- **Credential scrubbing**: Tool outputs are automatically scanned for sensitive patterns (API keys, tokens, passwords) and redacted before being sent to the LLM.
