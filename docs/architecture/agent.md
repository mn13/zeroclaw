# ZeroClaw Agent (zc)

The agent is the core runtime that executes LLM-powered conversations with tool use. Each agent runs as an isolated process exposing a gRPC service defined in `proto/zeroclaw.proto`.

**Binary**: `zc` | **Default gRPC port**: `50051`

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    ZeroClaw Agent                        │
│                                                         │
│  ┌──────────────────────────────────────────────────┐  │
│  │              gRPC Service (tonic)                 │  │
│  │  ClawAgentService implements ClawAgent proto      │  │
│  └───────────────────┬──────────────────────────────┘  │
│                      │                                  │
│  ┌───────────────────▼──────────────────────────────┐  │
│  │           Session Manager (actor)                 │  │
│  │                                                   │  │
│  │  - Command channel (SendMessage, Cancel)          │  │
│  │  - History store (SQLite)                         │  │
│  │  - Turn counter                                   │  │
│  │  - Busy flag (atomic)                             │  │
│  │  - Event broadcast                                │  │
│  └───────────────────┬──────────────────────────────┘  │
│                      │                                  │
│  ┌───────────────────▼──────────────────────────────┐  │
│  │              Agent Loop                           │  │
│  │                                                   │  │
│  │  1. Build system prompt                           │  │
│  │  2. Call LLM provider (streaming)                 │  │
│  │  3. Execute tool calls (if any)                   │  │
│  │  4. Loop back to step 2 until done                │  │
│  │  5. Return final response + token counts          │  │
│  └───────┬──────────────┬───────────────────────────┘  │
│          │              │                               │
│  ┌───────▼──────┐ ┌────▼──────────┐                   │
│  │  Providers    │ │    Tools      │                   │
│  │  (LLM APIs)  │ │  (execution)  │                   │
│  └──────────────┘ └───────────────┘                   │
└─────────────────────────────────────────────────────────┘
```

## Session Manager

The `SessionManager` (`src/serve/session.rs`) is the coordination layer between gRPC requests and the agent loop. It runs an **actor pattern** — a single async task processes commands sequentially from an MPSC channel.

### How It Works

1. **Construction**: On startup, the session manager opens (or creates) a SQLite history database, spawns the actor task, and recovers the turn counter from existing history.

2. **Message processing**: When a `SendMessage` command arrives:
   - The actor loads recent history from SQLite (up to `max_history_messages` entries).
   - It builds a context prefix from the history to give the agent conversation continuity.
   - It creates a delta-forwarding channel so streaming text from the LLM reaches the gRPC response stream.
   - It calls the message handler (which invokes the agent loop).
   - On completion, both the user message and assistant response are saved to the history store.

3. **Concurrency**: The `busy` atomic flag prevents concurrent message processing. If a new message arrives while the agent is busy, a `Queued` response is sent.

### Commands

| Command | Description |
|---------|-------------|
| `SendMessage { message, reply }` | Process a user message. Responses (deltas, tool events, done/error) are sent back via the `reply` channel. |
| `Cancel` | Cancel the current turn. Currently a no-op (cooperative cancellation not yet implemented). |

### Responses

| Response | Description |
|----------|-------------|
| `TurnStarted { turn_index }` | Sent immediately when processing begins. |
| `Delta(text)` | Streaming text chunk from the LLM. |
| `DraftClear` | Signal to clear accumulated draft content before the final answer. |
| `Done { content, turn_index, input_tokens, output_tokens }` | Turn completed successfully. |
| `Error(message)` | An error occurred during the turn. |
| `Queued { position }` | Message queued because agent is busy. |

**Note on tool events**: `ToolStart` and `ToolResult` frames visible in the gRPC `ChatOutput` stream are emitted by the agent loop via the delta/streaming channel during tool execution. They are separate from the `AgentResponse` enum — the gRPC service layer maps them into the protobuf `ChatOutput` oneof before forwarding to the gateway.

## Agent Loop

The agent loop (`src/agent/loop_.rs`) is the core orchestration logic that converts a user message into an assistant response, potentially using multiple rounds of tool calls.

### Flow

1. **System prompt construction**: Builds from identity files, configuration, available tools, current date/time, and memory context.

2. **LLM provider call**: Sends the conversation (system prompt + history + user message) to the configured LLM provider with tool definitions in OpenAI function-calling format.

3. **Tool execution loop**: If the LLM returns tool calls:
   - Each tool is executed with the provided arguments.
   - Tool outputs are scrubbed for credentials (API keys, tokens, passwords are redacted).
   - Results are appended to the conversation as tool messages.
   - The loop returns to step 2 for the LLM to process tool results.
   - This continues up to `max_tool_iterations` (default: 10) to prevent runaway loops.

4. **Streaming**: Text deltas are sent via an optional `on_delta` channel as they arrive from the LLM. Before streaming the final answer, a `CLEAR` sentinel is sent to reset draft content from progress messages.

5. **History compaction**: When the conversation exceeds `max_history_messages` (default: 50), older messages are summarized by the LLM and replaced with a compaction summary. The 20 most recent messages are always preserved.

6. **Auto-save to memory**: Sufficiently long user messages (20+ characters) can be auto-saved to the agent's memory for later retrieval.

### Security

- **Credential scrubbing**: A regex-based scanner detects patterns like `api_key=...`, `token: ...`, `password="..."` in tool outputs and redacts all but the first 4 characters.
- **Security policy**: The `SecurityPolicy` module controls which tools are available and what operations are permitted.
- **Tool approval**: An `ApprovalManager` can require explicit approval for certain tool invocations.

## gRPC Service

The `ClawAgentService` (`src/serve/grpc_server.rs`) implements the `ClawAgent` protobuf service. Key RPCs:

| RPC | Type | Description |
|-----|------|-------------|
| `SendMessage` | Server-streaming | Process a message. Returns a stream of `ChatOutput` frames (deltas, tool events, completion). |
| `CancelTurn` | Unary | Cancel the active turn. Returns whether a turn was running. |
| `GetHistory` | Unary | Paginated conversation history from SQLite. |
| `ClearHistory` | Unary | Clear all history (requires `confirm: true`). |
| `GetStatus` | Unary | Agent state (idle/busy), uptime, turn count, model/provider info. |
| `HealthCheck` | Unary | Returns `healthy: true` and the agent version. |
| `GetConfig` | Unary | Serialized agent config as JSON (API keys redacted). |
| `UpdateConfig` | Unary | JSON merge patch for runtime config. Persists to disk. |
| `ListMemory` | Unary | List memory entries. **Currently a stub** — returns an empty list. |
| `SearchMemory` | Unary | Search memory by query. **Currently a stub** — returns an empty list. |
| `StoreMemory` | Unary | Store a memory entry. **Currently unimplemented** — returns `UNIMPLEMENTED` status. |
| `ForgetMemory` | Unary | Delete a memory entry. **Currently unimplemented** — returns `UNIMPLEMENTED` status. |
| `ListTools` | Unary | List available tools. **Currently a stub** — returns an empty list. |
| `SubscribeEvents` | Server-streaming | Real-time event stream for observability. Broadcasts `turn_started`, `turn_complete`, and `turn_error` events with JSON data payloads and ISO 8601 timestamps. |

## Providers

Providers implement the `Provider` trait (`src/providers/traits.rs`) to connect to different LLM APIs. The agent uses an OpenAI-compatible chat completion format with function calling support.

Key types:
- **`ChatMessage`**: `{ role: string, content: string }` — supports `system`, `user`, `assistant`, and `tool` roles.
- **`ChatRequest`**: Messages + optional tool definitions + streaming options.
- **`ChatResponse`**: Text content + tool calls + token usage + optional reasoning content.
- **`ToolCall`**: `{ id, name, arguments }` — a tool invocation requested by the LLM.

The `ReliableProvider` wrapper adds retry logic and fallback handling around providers.

## Tools

The agent has a rich tool surface (41 implementations) registered via `all_tools_with_runtime()`. Each tool implements the `Tool` trait (`src/tools/traits.rs`) with `name()`, `description()`, `parameters_schema()`, and `execute()`.

**Categories**:
- **File I/O**: `file_read`, `file_write`, `file_edit`, `glob_search`, `content_search`
- **Shell**: `shell` (with runtime adapter and security policy integration)
- **Memory**: `memory_recall`, `memory_store`, `memory_forget`
- **Scheduling**: `cron_add`, `cron_list`, `cron_remove`, `cron_run`, `cron_update`, `schedule`
- **Vision**: `screenshot`, `image_info`, `pdf_read`
- **Web**: `web_fetch`, `web_search_tool`, `http_request`, `browser_open`, `browser`
- **Config**: `model_routing_config`, `proxy_config`, `git_operations`, `cli_discovery`
- **Integrations**: `composio`, `pushover`, `delegate` (agent-to-agent)
- **Hardware** (feature-gated): `hardware_board_info`, `hardware_memory_map`, `hardware_memory_read`

Tools are constructed with references to the security policy (for command approval, workspace boundaries) and runtime adapter.

## Channels

Channels (`src/channels/`) are long-running listeners that connect the agent to messaging platforms. Each implements the `Channel` trait with `listen(tx)` and `send(message)`.

**Supported channels** (18+): Telegram, Discord, Slack, Signal, WhatsApp, Mattermost, DingTalk, Email, MQTT, IRC, Nextcloud Talk, CLI, and more (some feature-gated).

Key design:
- Up to 4 concurrent messages per channel (`CHANNEL_PARALLELISM_PER_CHANNEL`)
- Per-sender conversation history with compaction
- Draft update support for streaming (Slack, Discord)
- Exponential backoff on reconnect (2s → 60s)
- Thread support via `thread_ts` field

## Memory

Memory backends (`src/memory/`) provide persistent knowledge storage. The `Memory` trait supports `store`, `recall` (scored search), `get`, `list`, `forget`, and `count`.

**Backends**: `sqlite` (default, with BM25 FTS5 + vector embeddings), `markdown` (plain files), `lucid` (layered local+remote), `qdrant` (vector-only), `postgres` (feature-gated), `none` (disabled).

The SQLite backend uses hybrid scoring: `vector_weight * cosine_score + keyword_weight * bm25_score`. Embeddings are cached in an LRU cache (up to 10k entries).

## Observability

The `Observer` trait (`src/observability/traits.rs`) records events and metrics throughout the agent lifecycle.

**Event types**: `AgentStart`, `LlmRequest`, `LlmResponse` (with token counts), `ToolCallStart`, `ToolCall` (with timing), `ChannelMessage`, `TurnComplete`, `Error`.

**Backends**: `log` (tracing), `prometheus` (metrics with labels), `otel` (OpenTelemetry OTLP, feature-gated), `noop` (default), `multi` (composite).

## History Store

Conversation history is persisted to SQLite (`data/history.db`). Each row stores:
- `turn_index` — groups user+assistant messages into turns
- `role` — `"user"` or `"assistant"`
- `content` — message text
- `created_at` — ISO 8601 timestamp

The store supports paginated loading, count queries, and clearing.
