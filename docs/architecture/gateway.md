# ZeroClaw Gateway (zcgw)

The gateway is a lightweight multi-instance orchestrator that sits between clients (Web UI, external integrations) and one or more ZeroClaw agent instances. It is implemented in Rust using Axum for HTTP/WebSocket and Tonic for gRPC client connections.

**Crate**: `zcgw/` | **Binary**: `zcgw` | **Default port**: `8080`

## Architecture

```
                     ┌─────────────────────────────┐
  HTTP/WS clients ──>│         Axum Router          │
                     │                              │
                     │  Auth Middleware              │
                     │  CORS Layer                   │
                     │                              │
                     │  ┌──────────┐ ┌────────────┐ │
                     │  │ REST API │ │ WebSocket  │ │
                     │  └────┬─────┘ └─────┬──────┘ │
                     │       │             │        │
                     │  ┌────┴─────────────┴──────┐ │
                     │  │   Instance Registry      │ │
                     │  │   (gRPC client pool)     │ │
                     │  └────┬─────────────┬──────┘ │
                     │       │             │        │
                     │  ┌────┴──┐     ┌────┴──┐    │
                     │  │Docker │     │Health │    │
                     │  │Mgmt   │     │Loop   │    │
                     │  └───────┘     └───────┘    │
                     └─────────────────────────────┘
                          │gRPC          │gRPC
                          v              v
                     ┌─────────┐    ┌─────────┐
                     │ Agent 1 │    │ Agent 2 │  ...
                     └─────────┘    └─────────┘
```

## Source Files

| File | Purpose |
|------|---------|
| `main.rs` | Entrypoint. Loads config and env, builds Axum router, starts server. |
| `config.rs` | `GatewayConfig` and `InstanceConfig` structs. TOML parsing. |
| `registry.rs` | `InstanceRegistry` — manages instance metadata, gRPC client pool, and health state. |
| `api.rs` | REST API handlers for instance-scoped operations (status, history, config, memory, chat, identity, connectors, MCP, Google, Signal, cron, skills) and gateway-level integration management (Google OAuth, Signal device linking). |
| `admin.rs` | Admin API handlers (stats, gateway config, instance CRUD, container actions). |
| `ws.rs` | WebSocket chat handler. Bridges WS messages to gRPC `SendMessage` streams. |
| `auth.rs` | Bearer token authentication middleware. |
| `docker.rs` | Docker container lifecycle management (create, start, stop, restart, destroy). |
| `app_state.rs` | Shared application state struct. |

**Note**: The gateway is a pure API server — it does not serve the Web UI. The frontend is deployed as a separate nginx container (see [Docker documentation](docker.md)).

## Configuration

The gateway is configured via a TOML file (default: `./zcgw.toml`, override with `ZCGW_CONFIG_PATH`).

```toml
listen_addr = "0.0.0.0:8080"

[instances.agent-ava]
grpc_address = "http://agent-ava:50051"
display_name = "ava"
desired_state = "running"      # "running" or "stopped"

[instances.agent-morph]
grpc_address = "http://agent-morph:50051"
display_name = "morpheus"
desired_state = "running"
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `listen_addr` | string | Address and port the gateway listens on. |
| `instances` | map | Keyed by instance ID. Each entry defines an agent to manage. |
| `instances.<id>.grpc_address` | string | The gRPC endpoint of the agent. Can be `host:port` or `http://host:port`. |
| `instances.<id>.display_name` | string | Human-readable name shown in the UI. |
| `instances.<id>.desired_state` | string | `"running"` or `"stopped"`. Controls whether the gateway starts/stops the container. |

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ZCGW_CONFIG_PATH` | `./zcgw.toml` | Path to the gateway config file. |
| `ZCGW_AUTH_TOKEN` | _(empty)_ | Bearer token for API authentication. If empty, all endpoints are unprotected. |
| `ZCGW_GRPC_SECRET` | _(empty)_ | Bearer token attached to gRPC calls to agent instances. |
| `ZCGW_ENV_FILE` | _(auto)_ | Path to `.env` file. Falls back to `docker/.env` then `.env`. |
| `ZCGW_HOST_MODE` | `true` | Set to `false` when gateway runs inside Docker. Controls how agent containers are addressed. |
| `ZCGW_DOCKER_IMAGE` | `zeroclaw:latest` | Docker image used when creating new agent containers. |
| `ZCGW_DOCKER_NETWORK` | `zeroclaw-net` | Docker network for inter-container communication. |
| `ZCGW_DOCKER_GRPC_PORT` | `50051` | Internal gRPC port inside agent containers. |
| `ZCGW_DOCKER_MEMORY_LIMIT` | `512m` | Memory limit for agent containers. |
| `ZCGW_DOCKER_CONFIG_TEMPLATE` | _(empty)_ | Path to a config template file used as default for new agents. |
| `ZCGW_DOCKER_ENV_VARS` | _(empty)_ | Comma-separated `KEY=VALUE` pairs forwarded to agent containers. |
| `ZCGW_AGENTS_DIR` | `docker/agents` | Directory for agent data (config and persistent storage). |
| `ZCGW_HOST_AGENTS_DIR` | _(same as ZCGW_AGENTS_DIR)_ | Host-side path for bind mounts when gateway runs in Docker. |
| `ZCGW_BASE_PORT` | `50051` | Base port for sequential host port assignment when creating agents. |

#### Google Integration

| Variable | Default | Description |
|----------|---------|-------------|
| `ZEROCLAW_GOOGLE_CREDENTIALS_JSON` | _(empty)_ | Google OAuth app credentials JSON. Required for Google account linking. |
| `ZEROCLAW_GOOGLE_REDIRECT_HOST` | _(empty)_ | Public hostname for OAuth redirect URI (e.g. `gateway.example.com:8080`). Required for Google account linking. |

#### Signal Integration

| Variable | Default | Description |
|----------|---------|-------------|
| `ZCGW_SIGNAL_CLI_PATH` | `signal-cli` | Path to the `signal-cli` binary. |
| `ZCGW_SIGNAL_CLI_PORT` | `8686` | HTTP port for the signal-cli JSON-RPC daemon. |

API key environment variables (`VENICE_API_KEY`, `OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) are automatically forwarded to new agent containers.

## Instance Registry

The `InstanceRegistry` (`registry.rs`) is the central coordination point:

- **Instance metadata**: Stores `InstanceConfig` entries (gRPC address, display name, desired state).
- **gRPC client pool**: Lazily creates and caches `ClawAgentClient` connections. Invalidated on stop/restart/reconnect.
- **Health tracking**: Maintains per-instance health status (`Healthy`, `Unhealthy`, `Unknown`).
- **Health loop**: Every 30 seconds, pings all instances via the `HealthCheck` RPC. New instances get retried up to 5 times with 2-second intervals to allow container boot time.

## WebSocket Chat Protocol

**Endpoint**: `GET /ws/chat?instance=<id>&token=<token>`

The WebSocket connection is scoped to a single agent instance. The client sends JSON messages and receives JSON frames.

### Client → Server Messages

| Type | Fields | Description |
|------|--------|-------------|
| `message` | `content: string` | Send a user message to the agent. |
| `cancel` | _(none)_ | Cancel the current turn. |

### Server → Client Frames

| Type | Fields | Description |
|------|--------|-------------|
| `turn_start` | `turn_id`, `turn_index` | A new turn has begun processing. |
| `delta` | `turn_id`, `content` | A streaming text chunk from the LLM. |
| `clear` | `turn_id` | Clear accumulated draft content (signals that the final answer is about to stream). |
| `tool_start` | `turn_id`, `tool`, `arguments` | The agent is invoking a tool. |
| `tool_result` | `turn_id`, `tool`, `success`, `output` | Tool execution completed. |
| `done` | `turn_id`, `content`, `input_tokens`, `output_tokens` | Turn completed successfully. Contains the full final response and token usage. |
| `error` | `turn_id`\*, `message` | An error occurred during the turn. |
| `queued` | `turn_id`, `position` | The message was queued because the agent is busy. |
| `status` | `turn_id`, `busy`, `current_turn_index`, `history_length` | Agent status update. |

\* **Note on `error` frames**: Errors originating from the gateway itself (invalid JSON, unknown message type) do not include a `turn_id` field. Only errors relayed from the agent's gRPC stream include `turn_id`.

### Example WebSocket Session

```json
// Client sends:
{"type": "message", "content": "What is the weather in Berlin?"}

// Server responds with a stream of frames:
{"type": "turn_start", "turn_id": "turn-0", "turn_index": 0}
{"type": "tool_start", "turn_id": "turn-0", "tool": "shell", "arguments": "{\"command\":\"curl wttr.in/Berlin?format=3\"}"}
{"type": "tool_result", "turn_id": "turn-0", "tool": "shell", "success": true, "output": "Berlin: ☀️ +18°C"}
{"type": "clear", "turn_id": "turn-0"}
{"type": "delta", "turn_id": "turn-0", "content": "The current weather in Berlin is "}
{"type": "delta", "turn_id": "turn-0", "content": "sunny at 18°C."}
{"type": "done", "turn_id": "turn-0", "content": "The current weather in Berlin is sunny at 18°C.", "input_tokens": 1234, "output_tokens": 56}
```

## Docker Container Management

When `ZCGW_HOST_MODE=false` (gateway running in Docker), the gateway manages agent containers via the Docker socket (`/var/run/docker.sock`). Key operations:

- **`create_agent`**: Creates a new container from the configured image, sets up config and data volumes, assigns a sequential host port, starts the container.
- **`start_agent`** / **`stop_agent`**: Start or stop an existing container.
- **`restart_agent`**: Stop, then start. Invalidates the cached gRPC client.
- **`destroy_agent`**: Removes the container and optionally its data directory.
- **`ensure_agents_from_config`**: On startup, reconciles running containers with the config file — creates missing containers, starts/stops based on `desired_state`.

Each agent gets a directory under `ZCGW_AGENTS_DIR/<id>/` containing:
- `config.toml` — the agent's configuration
- `data/` — persistent storage (history database, memory files)
