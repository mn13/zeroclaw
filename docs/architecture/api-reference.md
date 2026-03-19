# Gateway REST API Reference

All endpoints require authentication via `Authorization: Bearer <token>` header unless noted otherwise. The token is configured via `ZCGW_AUTH_TOKEN`.

Base URL: `http://<gateway-host>:8080`

---

## Health

### `GET /health`

**Authentication**: None (public endpoint)

Returns gateway health status.

**Response** `200 OK`:
```json
{"status": "ok"}
```

---

## Instance Operations

### `GET /api/instances`

List all registered agent instances with their health status.

**Response** `200 OK`:
```json
[
  {
    "id": "agent-ava",
    "display_name": "ava",
    "grpc_address": "http://agent-ava:50051",
    "health": "healthy"
  }
]
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique instance identifier. |
| `display_name` | string | Human-readable name. |
| `grpc_address` | string | gRPC endpoint address. |
| `health` | string | One of `"healthy"`, `"unhealthy"`, `"unknown"`. |

---

### `GET /api/instances/{id}/status`

Get the runtime status of a specific agent instance. Proxied via gRPC `GetStatus`.

**Response** `200 OK`:
```json
{
  "state": "idle",
  "uptime_secs": 3600,
  "total_turns": 42,
  "history_length": 84,
  "model": "anthropic/claude-sonnet-4-20250514",
  "provider": "openrouter"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `state` | string | `"idle"` or `"busy"`. |
| `uptime_secs` | number | Seconds since the agent process started. |
| `total_turns` | number | Total conversation turns processed. |
| `history_length` | number | Number of messages in the history store. |
| `model` | string | Currently configured LLM model identifier. |
| `provider` | string | Currently configured LLM provider. |

**Errors**: `404` if instance not found, `502` if agent is unreachable.

---

## History

### `GET /api/instances/{id}/history`

Retrieve paginated conversation history.

**Query Parameters**:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `offset` | number | `0` | Number of messages to skip. |
| `limit` | number | `50` | Maximum messages to return. |

**Response** `200 OK`:
```json
{
  "total": 84,
  "offset": 0,
  "limit": 50,
  "messages": [
    {
      "turn_index": 0,
      "role": "user",
      "content": "Hello!",
      "created_at": "2026-03-17T10:00:00Z"
    },
    {
      "turn_index": 0,
      "role": "assistant",
      "content": "Hi there! How can I help?",
      "created_at": "2026-03-17T10:00:02Z"
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `total` | number | Total number of history messages. |
| `offset` | number | Applied offset. |
| `limit` | number | Applied limit. |
| `messages[].turn_index` | number | Turn number (both user and assistant messages in a turn share the same index). |
| `messages[].role` | string | `"user"` or `"assistant"`. |
| `messages[].content` | string | Message text. Error responses are prefixed with `[error]`. |
| `messages[].created_at` | string | ISO 8601 timestamp. |

### `DELETE /api/instances/{id}/history`

Clear all conversation history for the instance.

**Response** `200 OK`:
```json
{"messages_cleared": 84}
```

---

## Chat (REST)

### `POST /api/instances/{id}/chat`

Send a message and receive the complete response (non-streaming). For streaming responses, use the WebSocket endpoint instead.

**Request Body**:
```json
{"message": "What is 2 + 2?"}
```

**Response** `200 OK`:
```json
{
  "content": "2 + 2 = 4.",
  "turn_index": 5,
  "input_tokens": 150,
  "output_tokens": 12
}
```

| Field | Type | Description |
|-------|------|-------------|
| `content` | string | The full assistant response. |
| `turn_index` | number | Turn number for this exchange. |
| `input_tokens` | number | Tokens consumed for the prompt. |
| `output_tokens` | number | Tokens generated for the response. |

---

## Configuration

### `GET /api/instances/{id}/config`

Get the agent's runtime configuration (secrets are redacted).

**Response** `200 OK`:
```json
{
  "default_model": "anthropic/claude-sonnet-4-20250514",
  "default_provider": "openrouter",
  "api_key": "[REDACTED]",
  "system_prompt": "You are a helpful assistant.",
  "agent": {
    "max_tool_iterations": 10,
    "max_history_messages": 50
  }
}
```

### `PUT /api/instances/{id}/config`

Update the agent's configuration using a JSON merge patch (RFC 7396). Only include fields you want to change.

**Request Body**:
```json
{
  "default_model": "anthropic/claude-haiku-4-5-20251001",
  "agent": {"max_tool_iterations": 5}
}
```

**Response** `200 OK`:
```json
{
  "updated_fields": ["default_model", "agent"],
  "requires_restart": false
}
```

Fields set to `"[REDACTED]"` are automatically stripped to prevent overwriting secrets with the redacted placeholder.

---

## Memory

### `GET /api/instances/{id}/memory`

List stored memory entries.

> **Note**: The agent-side gRPC implementation currently returns stub data (empty list). Full memory listing via the gateway will work once the agent's `ListMemory` RPC is wired to the memory backend.

**Query Parameters**:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `category` | string | _(empty)_ | Filter by memory category. |
| `offset` | number | `0` | Number of entries to skip. |
| `limit` | number | `200` | Maximum entries to return. |

**Response** `200 OK`:
```json
{
  "entries": [
    {
      "key": "user_preference_123",
      "content": "User prefers concise answers",
      "category": "preference",
      "timestamp": "2026-03-17T10:00:00Z",
      "score": 0.0
    }
  ],
  "total": 1
}
```

### `GET /api/instances/{id}/memory/search`

Search memory entries by semantic similarity.

> **Note**: The agent-side gRPC implementation currently returns stub data (empty list).

**Query Parameters**:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `query` | string | _(required)_ | Search query. |
| `limit` | number | `50` | Maximum entries to return. |

**Response**: Same format as `GET /api/instances/{id}/memory`, with `score` indicating relevance.

### `POST /api/instances/{id}/memory`

Store a new memory entry.

> **Note**: Not yet implemented on the agent side. Returns an `UNIMPLEMENTED` gRPC error (mapped to `502 Bad Gateway` by the gateway).

**Request Body**:
```json
{
  "key": "my-memory-key",
  "content": "Important information to remember",
  "category": "general"
}
```

### `DELETE /api/instances/{id}/memory/{key}`

Delete a memory entry by key.

> **Note**: Not yet implemented on the agent side. Returns an `UNIMPLEMENTED` gRPC error.

---

## Tools

### `GET /api/instances/{id}/tools`

List available tools for the agent.

> **Note**: The agent-side gRPC implementation currently returns a stub (empty list). The tool definitions are available within the agent loop but not yet exposed via the gRPC `ListTools` RPC.

**Response** `200 OK`:
```json
{
  "tools": [
    {
      "name": "shell",
      "description": "Execute a shell command",
      "parameters_json": "{\"type\":\"object\",\"properties\":{\"command\":{\"type\":\"string\"}}}"
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tools[].name` | string | Tool name. |
| `tools[].description` | string | Human-readable description. |
| `tools[].parameters_json` | string | JSON Schema for the tool's parameters (as a JSON string). |

---

## Identity Files

Identity files define the agent's persona and behavior (system prompt components stored as individual files).

### `GET /api/instances/{id}/identity`

List all identity files.

**Response** `200 OK`:
```json
{
  "files": [
    {"filename": "persona.md", "content": "You are a helpful coding assistant."}
  ],
  "known_files": ["persona.md", "instructions.md", "knowledge.md"]
}
```

### `GET /api/instances/{id}/identity/{filename}`

Get a single identity file.

### `PUT /api/instances/{id}/identity/{filename}`

Create or update an identity file.

**Request Body**:
```json
{"content": "Updated persona content..."}
```

### `DELETE /api/instances/{id}/identity/{filename}`

Delete an identity file.

### `PUT /api/instances/{id}/identity`

Batch update multiple identity files at once.

**Request Body**:
```json
{
  "files": [
    {"filename": "persona.md", "content": "..."},
    {"filename": "instructions.md", "content": "..."}
  ]
}
```

**Response** `200 OK`:
```json
{"ok": true, "saved": 2}
```

---

## Connectors (Channels)

### `GET /api/instances/{id}/connectors`

Get the agent's channel configuration and available channel schemas.

**Response** `200 OK`:
```json
{
  "channels_config": {
    "telegram": {"bot_token": "[REDACTED]", "allowed_users": []}
  },
  "channel_schema": [
    {
      "channel_type": "telegram",
      "label": "Telegram",
      "fields": [
        {
          "name": "bot_token",
          "label": "Bot Token",
          "field_type": "string",
          "required": true,
          "sensitive": true,
          "help": "Telegram bot API token"
        }
      ]
    }
  ]
}
```

### `PUT /api/instances/{id}/connectors`

Update channel configuration. Requires agent restart to take effect.

**Request Body**:
```json
{"channels_config": {"telegram": {"bot_token": "123:ABC", "allowed_users": []}}}
```

**Response** `200 OK`:
```json
{"ok": true, "requires_restart": true}
```

---

## MCP Servers

### `GET /api/instances/{id}/mcp-servers`

List configured MCP (Model Context Protocol) servers.

**Response** `200 OK`:
```json
{
  "mcp_servers": [
    {
      "name": "filesystem",
      "transport": "stdio",
      "command": "mcp-server-filesystem",
      "args": ["/data"],
      "env": {},
      "enabled": true
    }
  ]
}
```

### `PUT /api/instances/{id}/mcp-servers`

Update MCP server configuration. Requires restart.

**Request Body**:
```json
{"mcp_servers": [...]}
```

---

## Integrations

### `GET /api/instances/{id}/integrations/composio`

Get Composio integration status.

**Response** `200 OK`:
```json
{"enabled": false, "entity_id": "", "has_api_key": false}
```

### `PUT /api/instances/{id}/integrations/composio`

Update Composio configuration.

**Request Body**:
```json
{"enabled": true, "api_key": "...", "entity_id": "default"}
```

### `GET /api/instances/{id}/integrations/google`

Get Google integration status (GOGCLI-based OAuth).

**Response** `200 OK`:
```json
{
  "enabled": false,
  "has_credentials": false,
  "has_redirect_host": false,
  "accounts": [],
  "auto_whitelist_gog": true
}
```

| Field | Type | Description |
|-------|------|-------------|
| `enabled` | boolean | Whether Google integration is active. |
| `has_credentials` | boolean | Whether `ZEROCLAW_GOOGLE_CREDENTIALS_JSON` is set on the gateway. |
| `has_redirect_host` | boolean | Whether `ZEROCLAW_GOOGLE_REDIRECT_HOST` is set on the gateway. |
| `accounts` | string[] | List of authorized Google account emails. |
| `auto_whitelist_gog` | boolean | Auto-whitelist Google domains for the agent. |

### `PUT /api/instances/{id}/integrations/google`

Update Google integration configuration.

**Request Body**:
```json
{
  "enabled": true,
  "auto_whitelist_gog": true
}
```

All fields are optional.

### `POST /api/instances/{id}/integrations/google/auth/init`

Initiate the redirect-based Google OAuth flow. Writes the gateway's Google credentials to the agent container, starts GOG CLI's remote auth flow (step 1), and returns the Google consent URL. The frontend should redirect the browser to `auth_url`.

Requires `ZEROCLAW_GOOGLE_CREDENTIALS_JSON` and `ZEROCLAW_GOOGLE_REDIRECT_HOST` to be set on the gateway (returns `400` otherwise).

**Request Body**:
```json
{"email": "user@example.com"}
```

**Response** `200 OK`:
```json
{"auth_url": "https://accounts.google.com/o/oauth2/...", "email": "user@example.com"}
```

### `GET /oauth2/callback`

**Authentication**: None (public endpoint — protected by unguessable OAuth `state` parameter)

Google redirects the browser here after the user grants consent. The gateway:
1. Validates the `state` parameter against its pending-auth map
2. Completes the token exchange via GOG CLI (step 2)
3. Adds the email to the agent's `google.accounts` config
4. Redirects the browser to `/integrations?google=success&email=<email>` (or `?google=error&message=<msg>` on failure)

**Query Parameters** (set by Google): `code`, `state`, `error`

### `DELETE /api/instances/{id}/integrations/google/accounts/{email}`

Remove a Google account from the agent's configuration.

**Response** `200 OK`:
```json
{"ok": true}
```

---

## Cron Jobs

### `GET /api/instances/{id}/cron`

List scheduled cron jobs.

**Response** `200 OK`:
```json
{
  "jobs": [
    {
      "id": "uuid-123",
      "name": "Daily summary",
      "expression": "0 9 * * *",
      "schedule": "daily at 9am",
      "command": "",
      "prompt": "Summarize today's news",
      "job_type": "agent",
      "enabled": true,
      "next_run": "2026-03-18T09:00:00Z",
      "last_run": "2026-03-17T09:00:00Z",
      "last_status": "success",
      "last_output": "...",
      "created_at": "2026-03-01T00:00:00Z",
      "session_target": "",
      "model": "",
      "delivery": "",
      "delete_after_run": false
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique job identifier (UUID). |
| `name` | string | Human-readable job name. |
| `expression` | string | Cron expression (e.g., `"0 9 * * *"`). |
| `job_type` | string | `"shell"` (run a command) or `"agent"` (send a prompt to the agent). |
| `command` | string | Shell command (for `shell` type). |
| `prompt` | string | Agent prompt (for `agent` type). |
| `enabled` | boolean | Whether the job is active. |
| `delete_after_run` | boolean | If true, the job is removed after its first execution. |

### `POST /api/instances/{id}/cron`

Create a new cron job.

**Request Body**:
```json
{
  "name": "Hourly check",
  "expression": "0 * * * *",
  "job_type": "agent",
  "prompt": "Check system status"
}
```

**Response** `200 OK`:
```json
{"ok": true, "id": "uuid-456"}
```

### `PUT /api/instances/{id}/cron/{job_id}`

Update an existing cron job (partial update).

### `DELETE /api/instances/{id}/cron/{job_id}`

Delete a cron job.

### `GET /api/instances/{id}/cron/{job_id}/runs`

Get execution history for a cron job.

**Query Parameters**:

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `limit` | number | `20` | Maximum runs to return. |

**Response** `200 OK`:
```json
{
  "runs": [
    {
      "id": 1,
      "job_id": "uuid-123",
      "started_at": "2026-03-17T09:00:00Z",
      "finished_at": "2026-03-17T09:00:05Z",
      "status": "success",
      "output": "...",
      "duration_ms": 5000
    }
  ]
}
```

---

## Skills

### `GET /api/instances/{id}/skills`

List agent skills (custom knowledge/capability files).

**Response** `200 OK`:
```json
{
  "skills": [
    {"name": "code-review", "content": "When reviewing code, focus on..."}
  ]
}
```

### `PUT /api/instances/{id}/skills/{name}`

Create or update a skill.

**Request Body**:
```json
{"content": "Skill definition content..."}
```

### `DELETE /api/instances/{id}/skills/{name}`

Delete a skill.

---

## Admin Endpoints

### `GET /api/admin/stats`

Gateway-level statistics.

**Response** `200 OK`:
```json
{
  "uptime_secs": 86400,
  "started_at": "2026-03-16T10:00:00Z",
  "total_instances": 3,
  "healthy_count": 2,
  "unhealthy_count": 1
}
```

### `GET /api/admin/config`

Get the raw gateway TOML configuration.

**Response** `200 OK`:
```json
{"raw": "listen_addr = \"0.0.0.0:8080\"\n..."}
```

### `PUT /api/admin/config`

Update the gateway configuration. The new TOML is validated before saving. Changes require a gateway restart to take full effect.

**Request Body**:
```json
{"raw": "listen_addr = \"0.0.0.0:8080\"\n[instances.agent-1]\n..."}
```

**Response** `200 OK`:
```json
{"ok": true, "requires_restart": true}
```

### `GET /api/admin/instances`

List all instances with detailed information including Docker container status.

**Response** `200 OK`:
```json
[
  {
    "id": "agent-ava",
    "display_name": "ava",
    "grpc_address": "http://agent-ava:50051",
    "health": "healthy",
    "container_status": "running",
    "desired_state": "running"
  }
]
```

### `POST /api/admin/instances`

Create a new agent instance. Creates a Docker container, registers the instance, and persists to config.

**Request Body**:
```json
{
  "id": "agent-neo",
  "display_name": "Neo",
  "config_toml": "system_prompt = \"You are Neo.\"\n..."
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | Yes | Unique instance identifier (used as container name prefix). |
| `display_name` | string | Yes | Human-readable name. |
| `config_toml` | string | No | Agent TOML configuration. Falls back to the config template if empty. |

**Response** `200 OK`:
```json
{"ok": true, "id": "agent-neo"}
```

A background health check is triggered after creation (with a 2-second delay for the container to boot).

### `POST /api/admin/instances/{id}/action`

Perform a lifecycle action on an agent instance.

**Request Body**:
```json
{"action": "restart"}
```

| Action | Description |
|--------|-------------|
| `start` | Start the agent's Docker container. Sets desired state to `running`. Triggers health check. |
| `stop` | Stop the container. Invalidates gRPC client. Sets desired state to `stopped`. |
| `restart` | Stop then start the container. Invalidates gRPC client. Sets desired state to `running`. Triggers health check. |
| `destroy` | Remove the container and its data directory. Removes from registry and config file. |
| `reconnect` | Invalidate the cached gRPC client and trigger a health check to re-establish connection. Does not affect the container. |

**Response** `200 OK`:
```json
{"ok": true, "action": "restart", "id": "agent-ava"}
```

### `GET /api/admin/template`

Get the agent configuration template used as default for new instances.

**Response** `200 OK`:
```json
{"raw": "system_prompt = \"...\"\ndefault_model = \"...\"\n..."}
```

---

## WebSocket

### `GET /ws/chat?instance={id}&token={token}`

Upgrade to WebSocket for real-time streaming chat. See [Gateway documentation](gateway.md#websocket-chat-protocol) for the full protocol specification.
