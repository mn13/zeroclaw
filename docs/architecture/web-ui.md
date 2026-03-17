# Web UI

The ZeroClaw Web UI is a React single-page application that provides a graphical interface for interacting with agent instances through the gateway.

**Directory**: `web/` | **Deployed as**: Separate nginx container (see [Docker documentation](docker.md))

## Technology Stack

- **React** with TypeScript
- **Vite** for build tooling
- **nginx** serves the built SPA and reverse-proxies `/api/*` and `/ws/*` to the gateway
- Connected to the gateway via REST API and WebSocket

## Pages and Features

### Chat Interface (`pages/Chat.tsx`)

The primary interface for conversing with an agent instance.

**Features**:
- Real-time streaming responses via WebSocket
- Tool call visualization with expandable details (shows tool name, arguments, status, and output)
- Thinking/progress display during agent processing
- Token usage display (input/output tokens per turn)
- Connection status indicator (connected/connecting/disconnected/error)
- Message history loaded on mount from the REST API
- Auto-scroll to newest messages

**Message flow**:
1. User types a message and presses send.
2. The frontend sends `{"type": "message", "content": "..."}` over the WebSocket.
3. Incoming frames update the UI state:
   - `turn_start` — creates a new assistant message bubble and sets streaming state.
   - `delta` — appends text to the current assistant message (progressive rendering).
   - `clear` — resets accumulated draft content before the final answer streams in.
   - `tool_start` — adds a tool call entry to the current message with "running" status.
   - `tool_result` — updates the tool call entry with success/fail status and output.
   - `done` — finalizes the message and displays token counts.
   - `error` — displays an error message.

### Other UI Sections

The Web UI also provides management interfaces for:
- **Instance selection** — switch between configured agent instances
- **Admin panel** — create/start/stop/restart/destroy instances, view stats
- **Agent configuration** — edit model, provider, system prompt, and agent settings
- **Identity editor** — manage persona and instruction files
- **Connectors** — configure Telegram, Discord, Slack, and other channels
- **MCP servers** — manage Model Context Protocol server connections
- **Google integration** — OAuth account linking, credential management
- **Memory browser** — view and search agent memory entries
- **Cron jobs** — schedule recurring tasks
- **Skills** — manage custom agent skills

## API Client (`api.ts`)

A lightweight fetch wrapper that handles:
- **Authentication**: Stores a bearer token in a cookie (`zc_token`, 30-day expiry) and attaches it to all requests via the `Authorization` header.
- **REST calls**: Typed functions for every API endpoint (`listInstances`, `getStatus`, `getHistory`, `updateConfig`, etc.).
- **Integration clients**: Google OAuth (`getGoogle`, `updateGoogle`, `googleAuthInit`, `googleAuthComplete`, `deleteGoogleAccount`) and Composio (`getComposio`, `updateComposio`).
- **WebSocket**: `connectChat(instanceId)` creates a WebSocket connection to `/ws/chat?instance=<id>&token=<token>`.

### Type Definitions (`types.ts`)

The frontend defines TypeScript types matching the WebSocket protocol:

```typescript
type WsIncoming =
  | { type: "clear"; turn_id: string }
  | { type: "delta"; turn_id: string; content: string }
  | { type: "tool_start"; turn_id: string; tool: string; arguments: string }
  | { type: "tool_result"; turn_id: string; tool: string; success: boolean; output: string }
  | { type: "done"; turn_id: string; content: string; input_tokens: number; output_tokens: number }
  | { type: "error"; turn_id: string; message: string }
  | { type: "queued"; turn_id: string; position: number }
  | { type: "turn_start"; turn_id: string; turn_index: number }
  | { type: "status"; turn_id: string; busy: boolean; current_turn_index: number; history_length: number };
```

A thinking step represents one round of agent processing before a `clear` event resets the draft:

```typescript
interface ThinkingStep {
  text: string;
  toolCalls?: ToolCallInfo[];
}
```

And local chat message types:

```typescript
interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "error";
  content: string;
  /** Thinking steps collected before the final answer. Each CLEAR adds one. */
  steps?: ThinkingStep[];
  /** Tool calls for the current (not-yet-cleared) round. */
  toolCalls?: ToolCallInfo[];
}

interface ToolCallInfo {
  tool: string;
  arguments: string;
  status: "running" | "success" | "fail";
  output?: string;
}
```

Each time a `clear` frame arrives, the accumulated draft text and tool calls are bundled into a `ThinkingStep` and pushed onto `steps`. This allows the UI to show collapsible "thinking" rounds before the final streamed answer.

## Authentication Flow

1. On app load, the token is restored from the `zc_token` cookie.
2. If no token is stored, the user is prompted to enter one.
3. The token is sent with every API request as `Authorization: Bearer <token>`.
4. For WebSocket connections, the token is passed as a query parameter (`?token=<token>`) since WebSocket doesn't support custom headers in the browser.
5. If any request returns `401 Unauthorized`, the user is prompted to re-authenticate.
