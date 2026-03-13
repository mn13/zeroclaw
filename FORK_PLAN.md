# ZeroClaw Gateway Fork — Implementation Plan (v3)

## 1. Vision

Two separate Rust binaries running in separate containers:

1. **`zcgw`** (ZeroClaw Gateway) — A lightweight Rust service that handles authentication, client connections (WebSocket/REST), and orchestrates multiple ZeroClaw instances via gRPC. No AI logic lives here.

2. **`zc`** (ZeroClaw Instance) — A modified ZeroClaw that exposes a gRPC server instead of the current gateway. Owns its own agent, memory, tools, cron, sandbox, secrets, and **persistent chat history**. Fully self-contained.

The gateway manages ~10 ZeroClaw instances. Each instance belongs to one user/tenant. The gateway routes traffic, manages lifecycle, and provides a unified API + Web UI.

```
┌─────────────────────────────────────────────────────────────────┐
│                         Clients                                 │
│   (Web UI, mobile app, CLI, automations, external webhooks)     │
└──────────────┬──────────────────────────────────────────────────┘
               │ HTTPS / WSS
               │ Bearer token (per-instance) + instance ID
┌──────────────▼──────────────────────────────────────────────────┐
│                         zcgw (Gateway)                          │
│                    Lightweight Rust binary                       │
│                    Single container / pod                        │
│                                                                 │
│  ┌──────────┐ ┌───────────┐ ┌──────┐ ┌──────┐ ┌─────────────┐ │
│  │ WS Chat  │ │ REST API  │ │ SSE  │ │Web UI│ │ Channel     │ │
│  │ (stream) │ │ (manage,  │ │Events│ │(htmx)│ │ Ingress     │ │
│  │          │ │  config,  │ │      │ │      │ │ (webhooks)  │ │
│  │          │ │  cost...) │ │      │ │      │ │             │ │
│  └────┬─────┘ └─────┬─────┘ └──┬───┘ └──────┘ └──────┬──────┘ │
│       │              │          │                      │        │
│  ┌────▼──────────────▼──────────▼──────────────────────▼──────┐ │
│  │   Instance Router + gRPC Connection Pool                   │ │
│  │   Rate Limiter │ Idempotency │ Metrics │ Circuit Breaker   │ │
│  └────┬──────────┬──────────┬─────────────────────────────────┘ │
└───────┼──────────┼──────────┼───────────────────────────────────┘
        │ gRPC+TLS │ gRPC+TLS │ gRPC+TLS
        │ (shared  │ (shared  │ (shared secret)
        │  secret) │  secret) │
┌───────▼───┐ ┌───▼──────┐ ┌▼──────────┐
│   zc #1   │ │  zc #2   │ │  zc #N    │
│ (container│ │(container│ │(container │   Each zc instance:
│  / pod)   │ │ / pod)   │ │ / pod)    │   - Agent Actor (owns Agent)
│           │ │          │ │           │   - Own memory (SQLite)
│ AgentActor│ │AgentActor│ │AgentActor │   - Own chat history (r/w split)
│ Memory    │ │ Memory   │ │ Memory    │   - Own sandbox (no firejail)
│ Tools     │ │ Tools    │ │ Tools     │   - Own cron scheduler
│ Cron      │ │ Cron     │ │ Cron      │   - Own encrypted secrets
│ Sandbox   │ │ Sandbox  │ │ Sandbox   │   - Channels (Telegram, etc.)
│ History   │ │ History  │ │ History   │   - gRPC server on :50051
│ Channels  │ │ Channels │ │ Channels  │
│ Secrets   │ │ Secrets  │ │ Secrets   │
└───────────┘ └──────────┘ └───────────┘
```

**Future target**: Docker Compose for dev, Kubernetes for prod. One `zcgw` Deployment + N `zc` StatefulSets (each with persistent volume for memory/history).

---

## 2. Channel Routing: Evaluation & Decision

### The Problem

Channels fall into two categories:

| Type | Examples | Mechanism | Stateful? |
|------|----------|-----------|-----------|
| **Pull-based** | Telegram (long-poll), Slack (polling), IRC, IMAP | Persistent connection or polling loop | Yes — offsets, cursors, connection state |
| **Push-based** | WhatsApp Cloud API, generic webhooks, Nextcloud Talk | HTTP POST callbacks | No — stateless request/response |

### Option A: All Channels in ZeroClaw Instance

- Pro: Channel state co-located with agent (no distributed state)
- Pro: Stateful channels (Telegram, Discord WebSocket) work naturally
- Pro: Channel has direct access to agent with zero latency
- Pro: Each user's channel credentials stay in their own container
- Con: Webhook channels need the gateway to route incoming HTTP to the right instance
- Con: Each instance needs network egress for channel APIs
- Con: Pull-based channels bypass the gateway entirely — no visibility into their traffic (see section 15, Observability)

### Option B: All Channels in Gateway

- Pro: Single ingress point for everything
- Con: Stateful channels (Telegram long-poll, Discord WebSocket) can't run in a shared stateless gateway — would require distributed state (Redis/etcd) for offsets, connection management
- Con: Channel credentials would live in the gateway (security concern — multi-tenant leakage risk)
- Con: Significantly more complex gateway

### Option C: Hybrid — Stateful in ZeroClaw, Webhooks in Gateway

- Pro: Best of both
- Con: Split responsibility is confusing — "where does my channel run?"
- Con: Webhook channels in gateway still need gRPC hop to reach the agent

### Decision: **Option A — All Channels in ZeroClaw Instance**

Rationale:
1. **Simplicity**: One place for all channel logic. No split responsibility.
2. **Security**: Each user's Telegram bot token, Discord bot token, etc. stays in their own isolated container. No multi-tenant credential storage in the gateway.
3. **Stateful channels just work**: Telegram polling, Discord WebSocket, Slack Socket Mode — all run naturally inside the instance.
4. **Webhook routing is solved by the gateway**: The gateway receives incoming webhooks (e.g., WhatsApp `POST /webhook/whatsapp/{instance_id}`) and forwards them via gRPC to the correct ZeroClaw instance. The instance processes the message and returns the response. The gateway sends the reply back to the platform.

**Tradeoff acknowledged**: Pull-based channels (Telegram, Discord) connect directly from the instance without passing through the gateway. The gateway has no visibility into this traffic. Observability for these channels relies on the instance's own event stream (see section 15).

```
WhatsApp webhook POST → zcgw /webhook/whatsapp/inst_abc123
  → zcgw looks up inst_abc123 → gRPC to zc #1
  → zc #1 processes message, returns response
  → zcgw sends reply to WhatsApp API
```

For pull-based channels (Telegram, Discord), the ZeroClaw instance connects directly — no gateway involvement.

```
Telegram API ←→ zc #1 (direct, long-poll, no gateway needed)
Discord Gateway ←→ zc #2 (direct, WebSocket, no gateway needed)
```

---

## 3. What Lives Where

### zcgw (Gateway) — Lightweight, Mostly Stateless

| Concern | Details |
|---------|---------|
| Authentication | Per-instance bearer token validation |
| WebSocket proxy | Client <-> Gateway <-> gRPC streaming to zc instance |
| REST API | Proxied to zc via gRPC, plus gateway-level management |
| SSE events | Aggregates events from connected zc instances |
| Web UI | Static files served (htmx + Preact, embedded) |
| Instance registry | Tracks which zc instances exist and their gRPC addresses |
| Webhook ingress | Receives platform webhooks, routes to correct zc instance |
| Idempotency store | In-memory TTL store to deduplicate webhook deliveries |
| Rate limiting | Per-instance and global rate limits on all incoming requests |
| gRPC connection pool | Persistent pooled connections to all zc instances |
| Metrics | Request counts, latency histograms, error rates, connection counts |
| Provider API key mgmt | Distributes API keys to instances (same key or per-instance) |
| Cost aggregation | Collects per-instance cost data for dashboard |

### zc (ZeroClaw Instance) — Fully Self-Contained

| Concern | Details |
|---------|---------|
| Agent Actor | Dedicated tokio task owning `Agent`, receives commands via channel |
| Chat history | SQLite-backed `HistoryStore` with read/write connection separation |
| Memory | SQLite + embeddings (own database, own vector store) |
| Tools | Shell, file, cron, memory, browser, HTTP, etc. |
| Cron scheduler | Runs inside the instance, triggers agent turns |
| Channels | Telegram, Discord, Slack, etc. — owned by instance |
| Security | Filesystem sandbox (seccomp, not firejail), encrypted secrets, security policy |
| Heartbeat | Instance-level health pings |
| Observability | Token tracking, cost, events — reported to gateway via gRPC |
| gRPC server | Exposes all instance capabilities to the gateway (shared-secret auth) |

---

## 4. gRPC Service Definition

The gRPC interface is the contract between gateway and instance. Everything the gateway needs to do with a ZeroClaw instance goes through this.

### `zeroclaw.proto`

```protobuf
syntax = "proto3";
package zeroclaw;

import "google/protobuf/empty.proto";

// ═══════════════════════════════════════════════
// Core Chat — the primary interaction surface
// ═══════════════════════════════════════════════

service ClawAgent {
  // Send a message and receive a server-stream of deltas, tool events, and final response.
  // Unary request → server-stream response. Cancel via separate CancelTurn RPC.
  rpc SendMessage (SendMessageRequest) returns (stream ChatOutput);

  // Cancel the currently running turn (if any).
  rpc CancelTurn (google.protobuf.Empty) returns (CancelTurnResponse);

  // Get conversation history (paginated).
  rpc GetHistory (HistoryRequest) returns (HistoryResponse);

  // Get compact summary of recent history.
  rpc GetHistorySummary (google.protobuf.Empty) returns (HistorySummaryResponse);

  // Clear conversation history and reset agent state.
  rpc ClearHistory (google.protobuf.Empty) returns (ClearHistoryResponse);

  // ═══════════════════════════════════════════════
  // Status & Health
  // ═══════════════════════════════════════════════

  // Instance status (agent state, memory stats, security info).
  rpc GetStatus (google.protobuf.Empty) returns (StatusResponse);

  // Lightweight health check.
  rpc HealthCheck (google.protobuf.Empty) returns (HealthResponse);

  // ═══════════════════════════════════════════════
  // Configuration
  // ═══════════════════════════════════════════════

  // Get current config (secrets masked).
  rpc GetConfig (google.protobuf.Empty) returns (ConfigResponse);

  // Update config fields. Returns which fields changed.
  rpc UpdateConfig (UpdateConfigRequest) returns (UpdateConfigResponse);

  // Restart agent from current config (preserves history).
  rpc RestartAgent (google.protobuf.Empty) returns (RestartResponse);

  // ═══════════════════════════════════════════════
  // Cost & Spending
  // ═══════════════════════════════════════════════

  rpc GetCost (google.protobuf.Empty) returns (CostResponse);

  // ═══════════════════════════════════════════════
  // Cron
  // ═══════════════════════════════════════════════

  rpc ListCronJobs (google.protobuf.Empty) returns (CronJobList);
  rpc CreateCronJob (CreateCronJobRequest) returns (CronJob);
  rpc GetCronJob (CronJobId) returns (CronJob);
  rpc UpdateCronJob (UpdateCronJobRequest) returns (CronJob);
  rpc DeleteCronJob (CronJobId) returns (google.protobuf.Empty);
  rpc RunCronJob (CronJobId) returns (CronRunResult);
  rpc GetCronRuns (CronJobId) returns (CronRunList);

  // ═══════════════════════════════════════════════
  // Memory
  // ═══════════════════════════════════════════════

  rpc ListMemory (ListMemoryRequest) returns (MemoryEntryList);
  rpc SearchMemory (SearchMemoryRequest) returns (MemoryEntryList);
  rpc StoreMemory (StoreMemoryRequest) returns (google.protobuf.Empty);
  rpc ForgetMemory (ForgetMemoryRequest) returns (google.protobuf.Empty);
  rpc GetMemoryStats (google.protobuf.Empty) returns (MemoryStatsResponse);

  // ═══════════════════════════════════════════════
  // Tools & Skills
  // ═══════════════════════════════════════════════

  rpc ListTools (google.protobuf.Empty) returns (ToolList);
  rpc ListSkills (google.protobuf.Empty) returns (SkillList);

  // ═══════════════════════════════════════════════
  // Heartbeat & Observability
  // ═══════════════════════════════════════════════

  rpc GetHeartbeatStatus (google.protobuf.Empty) returns (HeartbeatStatusResponse);

  // Server-streaming: subscribe to real-time events.
  rpc SubscribeEvents (google.protobuf.Empty) returns (stream AgentEvent);

  // ═══════════════════════════════════════════════
  // Channel Webhook Forwarding
  // ═══════════════════════════════════════════════

  // Gateway forwards an incoming webhook payload to the instance.
  // Instance processes it through the appropriate channel handler.
  // Returns immediately with ack. Instance processes asynchronously
  // and sends replies via the platform's API directly.
  rpc ForwardWebhook (WebhookRequest) returns (WebhookAck);
}

// ═══════════════════════════════════════════════
// Chat Messages
// ═══════════════════════════════════════════════

message SendMessageRequest {
  string message = 1;             // User message text
  repeated FileAttachment files = 2;  // Optional file/image attachments
}

message FileAttachment {
  string filename = 1;
  string mime_type = 2;
  bytes content = 3;
}

message ChatOutput {
  string turn_id = 1;            // Unique ID for this turn (correlates all deltas)
  oneof output {
    string delta = 2;                 // Streaming text chunk
    ToolCallStarted tool_start = 3;   // Tool execution began
    ToolCallResult tool_result = 4;   // Tool execution completed
    TurnComplete done = 5;            // Final response
    TurnError error = 6;              // Error during turn
    QueuePosition queued = 7;         // Message queued (turn in progress)
    TurnStarted turn_start = 8;      // Queued message now processing
    AgentStatus status = 9;           // Current agent status (sent on connect)
  }
}

message CancelTurnResponse {
  bool was_running = 1;           // True if a turn was actually cancelled
  string turn_id = 2;            // The turn that was cancelled (if any)
}

message ToolCallStarted {
  string tool = 1;
  string arguments = 2;
}

message ToolCallResult {
  string tool = 1;
  bool success = 2;
  string output = 3;
}

message TurnComplete {
  string content = 1;
  uint64 turn_index = 2;
  uint64 input_tokens = 3;
  uint64 output_tokens = 4;
  double cost_usd = 5;
}

message TurnError {
  string message = 1;
}

message QueuePosition {
  uint32 position = 1;
}

message TurnStarted {
  uint64 turn_index = 1;
}

message AgentStatus {
  bool busy = 1;
  uint64 current_turn_index = 2;
  uint64 history_length = 3;
}

// ═══════════════════════════════════════════════
// History
// ═══════════════════════════════════════════════

message HistoryRequest {
  uint64 offset = 1;
  uint64 limit = 2;    // 0 = default (50)
}

message HistoryResponse {
  uint64 total = 1;
  uint64 offset = 2;
  uint64 limit = 3;
  repeated HistoryEntry messages = 4;
}

message HistoryEntry {
  uint64 turn_index = 1;
  HistoryEntryType entry_type = 2;
  string data_json = 3;      // JSON-serialized ConversationMessage
  string created_at = 4;
}

enum HistoryEntryType {
  HISTORY_ENTRY_TYPE_UNSPECIFIED = 0;
  HISTORY_ENTRY_TYPE_CHAT = 1;
  HISTORY_ENTRY_TYPE_TOOL_CALLS = 2;
  HISTORY_ENTRY_TYPE_TOOL_RESULTS = 3;
}

message HistorySummaryResponse {
  string summary = 1;              // Compaction summary text (if available)
  repeated HistoryEntry recent = 2; // Last N messages
  uint64 total_messages = 3;
}

message ClearHistoryResponse {
  uint64 messages_cleared = 1;
}

// ═══════════════════════════════════════════════
// Status
// ═══════════════════════════════════════════════

message StatusResponse {
  AgentInfo agent = 1;
  MemoryInfo memory = 2;
  SecurityInfo security = 3;
  SystemInfo system = 4;
}

message AgentInfo {
  string state = 1;        // "idle", "busy", "error"
  uint64 uptime_secs = 2;
  uint64 total_turns = 3;
  uint64 history_length = 4;
  string model = 5;
  string provider = 6;
  double temperature = 7;
}

message MemoryInfo {
  string backend = 1;
  uint64 entry_count = 2;
  bool healthy = 3;
}

message SecurityInfo {
  string autonomy_level = 1;
  string sandbox = 2;
  bool secrets_encrypted = 3;
}

message SystemInfo {
  string version = 1;
  uint64 pid = 2;
  string started_at = 3;
}

message HealthResponse {
  bool healthy = 1;
  string version = 2;
}

// ═══════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════

message ConfigResponse {
  string config_json = 1;   // Full config as JSON (secrets masked)
  // Note: config_json is used because Config is a complex, evolving Rust struct.
  // Proto-native config fields would diverge from the Rust schema. JSON is the
  // pragmatic bridge. The instance validates before applying.
}

message UpdateConfigRequest {
  string partial_json = 1;  // Partial config — only fields to update
}

message UpdateConfigResponse {
  repeated string updated_fields = 1;
  bool requires_restart = 2;
}

message RestartResponse {
  bool success = 1;
  uint64 history_restored = 2;
}

// ═══════════════════════════════════════════════
// Cost
// ═══════════════════════════════════════════════

message CostResponse {
  double session_cost_usd = 1;
  uint64 session_input_tokens = 2;
  uint64 session_output_tokens = 3;
  uint64 session_turns = 4;
  string session_started_at = 5;
  double today_cost_usd = 6;
  uint64 today_input_tokens = 7;
  uint64 today_output_tokens = 8;
  double per_turn_avg_cost_usd = 9;
  uint32 budget_max_cents = 10;
  uint32 budget_remaining_cents = 11;
}

// ═══════════════════════════════════════════════
// Cron
// ═══════════════════════════════════════════════

message CronJobId {
  string id = 1;
}

message CronJob {
  string id = 1;
  string name = 2;
  string schedule_expression = 3;
  CronJobType job_type = 4;
  string command = 5;
  string prompt = 6;
  bool enabled = 7;
  string next_run = 8;
  string last_run = 9;
  string last_status = 10;
}

enum CronJobType {
  CRON_JOB_TYPE_UNSPECIFIED = 0;
  CRON_JOB_TYPE_SHELL = 1;
  CRON_JOB_TYPE_AGENT = 2;
}

message CronJobList {
  repeated CronJob jobs = 1;
}

message CreateCronJobRequest {
  string schedule_expression = 1;
  CronJobType job_type = 2;
  string command = 3;
  string prompt = 4;
  string name = 5;
}

message UpdateCronJobRequest {
  string id = 1;
  string schedule_expression = 2;
  string name = 3;
  bool enabled = 4;
  string command = 5;
  string prompt = 6;
}

message CronRunResult {
  string status = 1;
  string output = 2;
  int64 duration_ms = 3;
}

message CronRunList {
  repeated CronRun runs = 1;
}

message CronRun {
  int64 id = 1;
  string job_id = 2;
  string started_at = 3;
  string finished_at = 4;
  string status = 5;
  string output = 6;
  int64 duration_ms = 7;
}

// ═══════════════════════════════════════════════
// Memory
// ═══════════════════════════════════════════════

message ListMemoryRequest {
  string category = 1;     // "" = all
  uint64 offset = 2;
  uint64 limit = 3;
}

message SearchMemoryRequest {
  string query = 1;
  uint64 limit = 2;        // 0 = default (5)
}

message StoreMemoryRequest {
  string key = 1;
  string content = 2;
  string category = 3;     // "core", "daily", "conversation", or custom
}

message ForgetMemoryRequest {
  string key = 1;
}

message MemoryEntry {
  string key = 1;
  string content = 2;
  string category = 3;
  string timestamp = 4;
  double score = 5;         // Relevance score (for search results)
}

message MemoryEntryList {
  repeated MemoryEntry entries = 1;
  uint64 total = 2;
}

message MemoryStatsResponse {
  uint64 total_entries = 1;
  map<string, uint64> by_category = 2;
  string backend = 3;
  bool healthy = 4;
}

// ═══════════════════════════════════════════════
// Tools & Skills
// ═══════════════════════════════════════════════

message ToolSpecProto {
  string name = 1;
  string description = 2;
  string parameters_json = 3;  // JSON Schema
}

message ToolList {
  repeated ToolSpecProto tools = 1;
}

message SkillProto {
  string name = 1;
  string description = 2;
}

message SkillList {
  repeated SkillProto skills = 1;
}

// ═══════════════════════════════════════════════
// Heartbeat
// ═══════════════════════════════════════════════

message HeartbeatStatusResponse {
  bool enabled = 1;
  uint32 interval_minutes = 2;
  string last_tick = 3;
  string next_tick = 4;
}

// ═══════════════════════════════════════════════
// Events (observability stream)
// ═══════════════════════════════════════════════

message AgentEvent {
  string event_type = 1;    // "llm_request", "tool_call", "agent_end", etc.
  string data_json = 2;     // Event-specific payload as JSON
  string timestamp = 3;
}

// ═══════════════════════════════════════════════
// Webhook Forwarding
// ═══════════════════════════════════════════════

message WebhookRequest {
  string platform = 1;      // "whatsapp", "slack", "nextcloud-talk", etc.
  bytes body = 2;            // Raw HTTP body
  map<string, string> headers = 3;  // Relevant HTTP headers
  string idempotency_key = 4;       // For dedup (e.g., X-Request-Id or message ID)
}

// Webhook ack — returned immediately. Processing happens asynchronously.
// Instance sends replies to the platform via the platform's outbound API.
message WebhookAck {
  bool accepted = 1;
  string error = 2;          // Non-empty if rejected (bad signature, unknown platform)
}
```

### Proto Versioning Strategy

Proto field numbers are append-only. Never reuse a field number. Deprecate by adding a comment; remove by reserving the number. Gateway and instances must be deployed from the same commit (no cross-version compatibility required for v1). Add `option java_package` / `option go_package` if cross-language clients are needed later.

### Why gRPC?

| Concern | gRPC | REST/WebSocket |
|---------|------|----------------|
| Server streaming | Native (`stream` keyword) | Requires separate WebSocket |
| Schema enforcement | Protobuf — typed, versioned | JSON — loose, unversioned |
| Code generation | Auto-generated Rust clients/servers (tonic) | Manual |
| Performance | HTTP/2, binary, multiplexed | HTTP/1.1, JSON overhead |
| Kubernetes-native | gRPC health probes, load balancing | Works but less integrated |
| Cross-language | Free Python/Go/TS clients from .proto | Manual SDK per language |

### Why Not Bidirectional Streaming for Chat?

The original plan used `rpc Chat (stream ChatInput) returns (stream ChatOutput)`. This was changed to unary request + server-stream response for these reasons:

1. **Reconnection**: If the WebSocket between client and gateway drops, a bidi stream cannot be resumed. The client must re-establish the stream and somehow figure out where it left off.
2. **Simplicity**: Sending a message is a unary call. Receiving deltas is a server stream. Cancellation is a separate unary call. Each concern is isolated.
3. **Error handling**: Unary RPCs have clear request/response error semantics. Bidi streams have ambiguous error ownership.

---

## 5. ZeroClaw Instance Modifications

The ZeroClaw binary needs these changes to serve as a gRPC-enabled instance:

### 5.1 New: gRPC Server Mode

New CLI command:

```bash
zc serve --grpc-port 50051 --data-dir /data
```

This starts:
1. An **Agent Actor** (dedicated tokio task owning the Agent)
2. The cron scheduler (background task)
3. The heartbeat timer (background task)
4. Channel listeners (Telegram, Discord, etc. — if configured)
5. A `tonic` gRPC server on the specified port
6. A **graceful shutdown coordinator** (see section 14)

### 5.2 New: HistoryStore (Read/Write Separation)

Chat history persisted to SQLite inside the instance's data directory. Uses separate connections for reads and writes to avoid blocking reads during long agent turns.

```rust
pub struct HistoryStore {
    writer: Mutex<rusqlite::Connection>,  // Serialized writes
    reader: rusqlite::Connection,          // Concurrent reads (WAL mode)
}

impl HistoryStore {
    pub fn new(data_dir: &Path) -> Result<Self> {
        // Open two connections. Enable WAL mode for concurrent read/write.
        // Set busy_timeout to 5000ms for writer.
        // reader is read-only.
    }
    pub fn append(&self, turn_index: u64, msg: &ConversationMessage) -> Result<()>;
    pub fn load_all(&self) -> Result<Vec<ConversationMessage>>;
    pub fn load_page(&self, offset: u64, limit: u64) -> Result<(Vec<HistoryEntry>, u64)>;
    pub fn count(&self) -> Result<u64>;
    pub fn clear(&self) -> Result<u64>;
    pub fn flush_wal(&self) -> Result<()>;  // Called during graceful shutdown
}
```

Schema:
```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;

CREATE TABLE conversation_history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    turn_index  INTEGER NOT NULL,
    message     TEXT NOT NULL,          -- JSON ConversationMessage
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    compacted   BOOLEAN NOT NULL DEFAULT 0
);
CREATE INDEX idx_history_turn ON conversation_history(turn_index);
```

**SQLite on Docker volumes**: WAL mode performs well on Docker named volumes. For production (K8s), use local PVs or EBS-backed PVCs. Avoid NFS or distributed filesystems for SQLite — they break locking semantics.

### 5.3 Agent Actor Pattern

The `Agent` struct takes `&mut self` in its `turn()` method and is not `Send`-safe across await points. Wrapping it in `Mutex<Agent>` would block the entire executor during 30+ second LLM calls. Instead, we use the **actor pattern**: a dedicated tokio task owns the `Agent` exclusively and receives commands via an mpsc channel.

**Why not `Agent::turn()`?** The `turn()` method on `Agent` does not support `on_delta` (streaming deltas back to clients) or `CancellationToken` (aborting mid-turn). Only `run_tool_call_loop()` in `loop_.rs` supports these — it takes 17 parameters including the delta sender and cancellation token. The Agent Actor must call `run_tool_call_loop()` directly, wiring up the parameters from the Agent's internal state and the per-turn request context.

```rust
/// Commands sent to the AgentActor.
enum AgentCommand {
    /// Execute a turn. Deltas are sent back via the response channel.
    Turn {
        message: String,
        files: Vec<FileAttachment>,
        delta_tx: mpsc::Sender<ChatOutput>,
        cancel_token: CancellationToken,
    },
    /// Cancel the current turn (if any).
    Cancel,
    /// Get current history (does not require &mut self on agent).
    GetHistory {
        reply: oneshot::Sender<Vec<ConversationMessage>>,
    },
    /// Clear history and reset agent.
    ClearHistory {
        reply: oneshot::Sender<u64>,
    },
    /// Restore history (on startup).
    RestoreHistory {
        history: Vec<ConversationMessage>,
        reply: oneshot::Sender<()>,
    },
    /// Shutdown the actor (graceful).
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

pub struct AgentActorHandle {
    cmd_tx: mpsc::Sender<AgentCommand>,
}

impl AgentActorHandle {
    /// Spawn the actor task. Returns the handle for sending commands.
    pub fn spawn(config: &Config) -> Result<Self> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<AgentCommand>(32);

        // Build everything run_tool_call_loop needs from config:
        let agent_config = config.agent.clone();
        let provider = providers::create_routed_provider(/* ... */)?;
        let tools = tools::all_tools_with_runtime(/* ... */);
        let observer = observability::create_observer(/* ... */);
        let memory = memory::create_memory_with_storage_and_routes(/* ... */)?;
        // ... etc.

        // Build the Agent for history management, prompt building, etc.
        let mut agent = Agent::from_config(config)?;
        let mut active_cancel: Option<CancellationToken> = None;

        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    AgentCommand::Turn { message, files, delta_tx, cancel_token } => {
                        active_cancel = Some(cancel_token.clone());

                        // 1. Build system prompt, enrich user message with memory context
                        // 2. Append user message to agent.history
                        // 3. Convert agent.history to ChatMessage vec for run_tool_call_loop

                        let mut history_msgs: Vec<ChatMessage> = /* convert from agent.history */;

                        // 4. Call run_tool_call_loop with all 17 parameters:
                        let result = run_tool_call_loop(
                            provider.as_ref(),
                            &mut history_msgs,
                            &tools,
                            observer.as_ref(),
                            &provider_name,
                            &model_name,
                            agent_config.temperature,
                            false,                          // silent
                            approval_manager.as_ref(),      // approval
                            "grpc",                         // channel_name
                            &multimodal_config,
                            agent_config.max_tool_iterations,
                            Some(cancel_token),
                            Some(delta_tx.clone()),         // on_delta for streaming
                            hooks.as_ref(),
                            &excluded_tools,
                            &dedup_exempt_tools,
                        ).await;

                        // 5. Send TurnComplete or TurnError via delta_tx
                        match result {
                            Ok(final_text) => {
                                let _ = delta_tx.send(ChatOutput::done(final_text)).await;
                            }
                            Err(e) => {
                                let _ = delta_tx.send(ChatOutput::error(e)).await;
                            }
                        }
                        active_cancel = None;
                    }
                    AgentCommand::Cancel => {
                        if let Some(token) = &active_cancel {
                            token.cancel();
                        }
                    }
                    AgentCommand::GetHistory { reply } => {
                        let _ = reply.send(agent.history().to_vec());
                    }
                    AgentCommand::ClearHistory { reply } => {
                        let count = agent.history().len() as u64;
                        agent.clear_history();
                        let _ = reply.send(count);
                    }
                    AgentCommand::RestoreHistory { history, reply } => {
                        agent.restore_history(history);
                        let _ = reply.send(());
                    }
                    AgentCommand::Shutdown { reply } => {
                        // If a turn is in progress, cancel it and wait
                        if let Some(token) = &active_cancel {
                            token.cancel();
                        }
                        let _ = reply.send(());
                        break; // Exit the actor loop
                    }
                }
            }
        });

        Ok(Self { cmd_tx })
    }
}
```

**Note on `run_tool_call_loop` wiring**: The function signature from `loop_.rs` is:

```rust
pub(crate) async fn run_tool_call_loop(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: f64,
    silent: bool,
    approval: Option<&ApprovalManager>,
    channel_name: &str,
    multimodal_config: &crate::config::MultimodalConfig,
    max_tool_iterations: usize,
    cancellation_token: Option<CancellationToken>,
    on_delta: Option<tokio::sync::mpsc::Sender<String>>,
    hooks: Option<&crate::hooks::HookRunner>,
    excluded_tools: &[String],
    dedup_exempt_tools: &[String],
) -> Result<String>
```

The `on_delta` sender receives `String` text deltas. The actor wraps these into `ChatOutput` messages with the `turn_id` before forwarding to the gRPC stream. The actor owns all 17 parameters as long-lived state, only the per-turn inputs (message, cancel token, delta sender) change per request.

### 5.4 New: SessionManager (Instance-Side)

Thin coordinator that ties the ActorHandle, HistoryStore, and metrics together.

```rust
pub struct SessionManager {
    actor: AgentActorHandle,
    history_store: Arc<HistoryStore>,
    event_tx: broadcast::Sender<AgentEvent>,
    turn_counter: AtomicU64,
    metrics: RwLock<SessionMetrics>,
    cost_tracker: RwLock<CostTracker>,
}

impl SessionManager {
    pub async fn send_message(
        &self,
        message: String,
        files: Vec<FileAttachment>,
    ) -> Result<(String, mpsc::Receiver<ChatOutput>)> {
        let turn_id = Uuid::new_v4().to_string();
        let (delta_tx, delta_rx) = mpsc::channel(64);
        let cancel_token = CancellationToken::new();

        // Store cancel token for this turn
        // ...

        self.actor.send(AgentCommand::Turn {
            message,
            files,
            delta_tx,
            cancel_token,
        }).await?;

        Ok((turn_id, delta_rx))
    }

    pub async fn cancel_turn(&self) -> CancelTurnResponse { /* ... */ }
}
```

### 5.5 New: gRPC Service Implementation

```rust
pub struct ClawAgentService {
    session: Arc<SessionManager>,
    config: Arc<RwLock<Config>>,
    cron_store: Arc<CronStore>,
    memory: Arc<dyn Memory>,
    observer: Arc<dyn Observer>,
    shared_secret: String,  // For gateway authentication
}

#[tonic::async_trait]
impl ClawAgent for ClawAgentService {
    type SendMessageStream = ReceiverStream<Result<ChatOutput, Status>>;

    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<Self::SendMessageStream>, Status> {
        // 1. Validate shared secret from metadata
        self.authenticate(&request)?;

        // 2. If agent is busy, send QueuePosition and wait
        // 3. Forward to SessionManager
        let (turn_id, mut delta_rx) = self.session
            .send_message(request.into_inner().message, vec![])
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        // 4. Create output channel with backpressure
        let (out_tx, out_rx) = mpsc::channel(32);

        // 5. Spawn task to relay deltas with turn_id tagging
        tokio::spawn(async move {
            while let Some(delta) = delta_rx.recv().await {
                let mut output = delta;
                output.turn_id = turn_id.clone();
                if out_tx.send(Ok(output)).await.is_err() {
                    break; // Client disconnected — backpressure drops remaining
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(out_rx)))
    }

    async fn cancel_turn(
        &self,
        request: Request<()>,
    ) -> Result<Response<CancelTurnResponse>, Status> {
        self.authenticate(&request)?;
        Ok(Response::new(self.session.cancel_turn().await))
    }

    async fn get_history(
        &self,
        req: Request<HistoryRequest>,
    ) -> Result<Response<HistoryResponse>, Status> {
        self.authenticate(&req)?;
        // Reads from HistoryStore.reader — does NOT block the agent actor
        // ...
    }

    // ... remaining RPCs delegate to appropriate subsystem

    fn authenticate<T>(&self, request: &Request<T>) -> Result<(), Status> {
        let token = request.metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        match token {
            Some(t) if t == self.shared_secret => Ok(()),
            _ => Err(Status::unauthenticated("invalid or missing shared secret")),
        }
    }
}
```

### 5.6 Modified: Agent (Minimal Changes)

Add two methods to the existing `Agent` struct:

```rust
impl Agent {
    pub fn restore_history(&mut self, history: Vec<ConversationMessage>) {
        self.history = history;
    }

    pub fn history_snapshot(&self) -> Vec<ConversationMessage> {
        self.history.clone()
    }
}
```

### 5.7 Kept As-Is

Everything else in ZeroClaw stays untouched:
- Provider system, tool system, memory, cron, security, observability
- The existing CLI commands (`zeroclaw agent`, `zeroclaw config`, etc.) still work
- The `serve` command is additive — doesn't break anything

### 5.8 New Dependencies

```toml
# In zc's Cargo.toml
tonic = "0.12"
prost = "0.13"
tokio-stream = "0.1"
tokio-util = "0.7"      # CancellationToken
uuid = "1"              # Turn IDs
```

### 5.9 Sandbox Strategy

The existing ZeroClaw uses firejail for filesystem sandboxing. **Firejail requires `CAP_SYS_ADMIN`** which is not available in default Docker containers and will silently fail. For containerized instances:

- **Phase 1**: Rely on Docker's own isolation (separate container per instance, read-only filesystem mounts, dropped capabilities). Firejail is not installed in the container image.
- **Phase 2 (future)**: Evaluate seccomp profiles or gVisor for additional in-container isolation if needed.

The `security.sandbox` config field should support `"docker"` as a sandbox mode that disables firejail but trusts container boundaries.

---

## 6. Gateway Architecture (zcgw)

### 6.1 What the Gateway Does

The gateway is a **thin routing and management layer**. It has **no AI logic, no agent, no memory, no tools**. It:

1. Authenticates clients (per-instance bearer tokens)
2. Maps clients to ZeroClaw instances
3. Proxies WebSocket <-> gRPC streaming
4. Serves REST endpoints (proxied to instances via gRPC)
5. Aggregates status/cost across all instances
6. Manages instance lifecycle (future: spawn/destroy containers)
7. Receives platform webhooks and forwards to correct instance (async, with idempotency)
8. Serves the Web UI
9. Rate-limits all incoming requests
10. Tracks metrics (request counts, latency, errors, connection counts)
11. Implements circuit breakers per instance (don't hammer a failing instance)

### 6.2 Instance Registry + gRPC Connection Pool

```rust
pub struct InstanceRegistry {
    instances: RwLock<HashMap<String, InstanceInfo>>,
    connection_pool: RwLock<HashMap<String, ClawAgentClient<tonic::transport::Channel>>>,
}

pub struct InstanceInfo {
    pub id: String,                          // e.g., "inst_abc123"
    pub display_name: String,                // e.g., "Daniel's Claw"
    pub grpc_address: String,                // e.g., "zc-1:50051" or "10.0.0.5:50051"
    pub api_key: Option<String>,             // Provider API key (if gateway-managed)
    pub auth_token: String,                  // Bearer token for clients accessing this instance
    pub status: InstanceStatus,              // Healthy, Unhealthy, Starting, Stopped
    pub last_health_check: DateTime<Utc>,
    pub circuit_breaker: CircuitBreaker,     // Per-instance circuit breaker state
    pub gateway_public_url: String,          // Public URL for webhook callbacks
}

pub enum InstanceStatus {
    Healthy,
    Unhealthy(String),  // reason
    Starting,
    Stopped,
    Unknown,
}

impl InstanceRegistry {
    /// Get or create a persistent gRPC connection to an instance.
    /// Connections are reused across requests.
    pub async fn get_client(&self, instance_id: &str) -> Result<ClawAgentClient<Channel>> {
        // Check pool first, create if missing, reconnect if broken
    }
}
```

**Phase 1 (now)**: Instances are statically configured in the gateway's config file or environment variables. Each instance has its own bearer token.

```toml
# zcgw.toml
[gateway]
# Shared secret for gateway-to-instance gRPC authentication
grpc_shared_secret = "${ZCGW_GRPC_SECRET}"

# Public URL where this gateway is reachable (for webhook callbacks)
public_url = "https://gateway.example.com"

[instances.daniel]
grpc_address = "zc-daniel:50051"
display_name = "Daniel's Claw"
auth_token = "${ZCGW_TOKEN_DANIEL}"   # Token clients use to access this instance

[instances.alice]
grpc_address = "zc-alice:50051"
display_name = "Alice's Claw"
auth_token = "${ZCGW_TOKEN_ALICE}"    # Different token per instance
```

**Phase 2 (future, K8s)**: Gateway calls Kubernetes API to create/destroy StatefulSets. Registry is populated dynamically from pod discovery.

### 6.3 Provider API Key Management

Two modes:

**Mode A — Shared Key**: Gateway holds the API key in `PROVIDER_API_KEY` env var. On instance startup (or via `UpdateConfig` gRPC call), the gateway pushes the key to each instance.

**Mode B — Per-Instance Key**: Each instance has its own key configured in its own config. Gateway doesn't manage keys.

**Recommendation**: Start with Mode A (shared key, gateway-distributed). The gateway calls `UpdateConfig` on each instance at startup to inject the key. Instances never persist the key to disk — it's held in memory only.

### 6.4 Authentication

**Per-instance bearer tokens** (not a single global token). Each instance has its own token configured in the gateway. This provides multi-tenant isolation: knowing one instance's token does not grant access to other instances.

```toml
# Each instance gets a unique token
[instances.daniel]
auth_token = "${ZCGW_TOKEN_DANIEL}"

[instances.alice]
auth_token = "${ZCGW_TOKEN_ALICE}"
```

All requests must include `Authorization: Bearer <token>`. The gateway validates the token and determines which instance(s) the client can access.

**Instance routing**: Clients specify the target instance via:
- Path prefix: `/api/instances/{instance_id}/status`
- WebSocket query: `/ws/chat?instance=daniel`
- Header: `X-Instance-Id: daniel`

The gateway validates that the bearer token is authorized for the requested instance.

**Gateway-to-instance authentication**: The gateway authenticates to instances using a shared secret (`ZCGW_GRPC_SECRET`) sent as a bearer token in gRPC metadata. This prevents unauthorized access to the gRPC port. In production, use TLS for the gRPC channel as well.

### 6.5 Rate Limiting

Token-bucket rate limiter per instance, plus global rate limit:

```rust
pub struct GatewayRateLimiter {
    global: RateBucket,                          // e.g., 1000 req/min total
    per_instance: RwLock<HashMap<String, RateBucket>>,  // e.g., 100 req/min per instance
    per_ip: RwLock<HashMap<IpAddr, RateBucket>>,        // e.g., 50 req/min per IP
}
```

Webhook endpoints get a separate, more generous limit (platforms retry aggressively on 429).

### 6.6 Gateway Endpoints

```
# Public
GET  /health                                    — Gateway health

# Instance-scoped (require auth + instance_id)
GET  /ws/chat?instance={id}                     — WebSocket streaming chat
GET  /api/instances/{id}/status                 — Instance status
GET  /api/instances/{id}/history                — Chat history (paginated)
GET  /api/instances/{id}/history/summary        — History summary
DELETE /api/instances/{id}/history              — Clear history
GET  /api/instances/{id}/config                 — Get config
PUT  /api/instances/{id}/config                 — Update config
POST /api/instances/{id}/restart                — Restart agent
GET  /api/instances/{id}/cost                   — Cost/spending
GET  /api/instances/{id}/cron                   — List cron jobs
POST /api/instances/{id}/cron                   — Create cron job
GET  /api/instances/{id}/cron/{job_id}          — Get cron job
PUT  /api/instances/{id}/cron/{job_id}          — Update cron job
DELETE /api/instances/{id}/cron/{job_id}        — Delete cron job
POST /api/instances/{id}/cron/{job_id}/run      — Trigger cron job
GET  /api/instances/{id}/cron/{job_id}/runs     — Cron run history
GET  /api/instances/{id}/memory                 — List memory entries
GET  /api/instances/{id}/memory/search          — Semantic search
POST /api/instances/{id}/memory                 — Store memory entry
DELETE /api/instances/{id}/memory/{key}         — Forget memory entry
GET  /api/instances/{id}/memory/stats           — Memory stats
GET  /api/instances/{id}/tools                  — List tools
GET  /api/instances/{id}/skills                 — List skills
GET  /api/instances/{id}/heartbeat              — Heartbeat status
GET  /api/instances/{id}/events                 — SSE event stream
POST /api/instances/{id}/chat                   — REST chat (non-streaming, returns full response)

# Gateway-level
GET  /api/instances                             — List all instances with status
GET  /api/dashboard                             — Aggregated stats across all instances
POST /api/instances/{id}/config/api-key         — Push API key to instance
GET  /api/metrics                               — Prometheus-format metrics

# Webhook ingress (platform-specific)
POST /webhook/{platform}/{instance_id}          — Forward webhook to instance
GET  /webhook/{platform}/{instance_id}          — Webhook verification (WhatsApp, etc.)

# Web UI
GET  /                                          — SPA index
GET  /_app/*                                    — Static assets
```

### 6.7 WebSocket -> gRPC Proxy

The gateway's WebSocket handler translates between the client-facing WebSocket protocol and the instance's gRPC `SendMessage` stream:

```rust
async fn ws_to_grpc_bridge(socket: WebSocket, state: GatewayState, instance_id: String) {
    let client = state.registry.get_client(&instance_id).await?;  // Pooled connection
    let (mut ws_tx, mut ws_rx) = socket.split();

    loop {
        match ws_rx.next().await {
            Some(Ok(Message::Text(text))) => {
                let msg: WsMessage = serde_json::from_str(&text)?;
                match msg {
                    WsMessage::Send { content, files } => {
                        // Start a new server-stream for this message
                        let request = SendMessageRequest {
                            message: content,
                            files: files.unwrap_or_default(),
                        };
                        // Add shared secret to gRPC metadata
                        let mut req = tonic::Request::new(request);
                        req.metadata_mut().insert(
                            "authorization",
                            format!("Bearer {}", state.grpc_secret).parse().unwrap(),
                        );
                        let mut stream = client.send_message(req).await?.into_inner();

                        // Relay deltas back to WebSocket with backpressure
                        while let Some(output) = stream.next().await {
                            match output {
                                Ok(chat_output) => {
                                    let ws_msg = chat_output_to_ws_message(chat_output);
                                    if ws_tx.send(ws_msg).await.is_err() {
                                        // Client disconnected — stream will be dropped,
                                        // agent continues but deltas are discarded
                                        return;
                                    }
                                }
                                Err(status) => {
                                    let _ = ws_tx.send(error_to_ws_message(status)).await;
                                    break;
                                }
                            }
                        }
                    }
                    WsMessage::Cancel => {
                        let _ = client.cancel_turn(()).await;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => break,
            _ => continue,
        }
    }
}
```

**Backpressure**: The `ws_tx.send()` call provides natural backpressure. If the WebSocket client is slow to read, `send` blocks. If the internal channel fills up (bounded at 32), the relay task waits rather than buffering unboundedly. If the client disconnects entirely, the stream is dropped.

### 6.8 Health Checking

Background task polls each instance every 30 seconds using **pooled connections**:

```rust
async fn health_check_loop(registry: Arc<InstanceRegistry>) {
    loop {
        for instance in registry.all() {
            // Skip if circuit breaker is open
            if instance.circuit_breaker.is_open() {
                continue;
            }
            match registry.get_client(&instance.id).await {
                Ok(mut client) => {
                    match client.health_check(()).await {
                        Ok(resp) => {
                            registry.set_status(&instance.id, Healthy);
                            instance.circuit_breaker.record_success();
                        }
                        Err(e) => {
                            registry.set_status(&instance.id, Unhealthy(e.to_string()));
                            instance.circuit_breaker.record_failure();
                        }
                    }
                }
                Err(e) => {
                    registry.set_status(&instance.id, Unhealthy(e.to_string()));
                    instance.circuit_breaker.record_failure();
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}
```

### 6.9 Circuit Breaker

Per-instance circuit breaker to avoid hammering a failing instance:

```rust
pub struct CircuitBreaker {
    state: AtomicU8,           // Closed=0, Open=1, HalfOpen=2
    failure_count: AtomicU32,
    last_failure: Mutex<Option<Instant>>,
    threshold: u32,            // Open after N consecutive failures (default: 3)
    recovery_timeout: Duration, // Try again after this duration (default: 30s)
}
```

When the circuit is open, REST/WS requests to that instance return `503 Service Unavailable` immediately instead of timing out.

### 6.10 Gateway Dependencies

```toml
# zcgw Cargo.toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
tonic = "0.12"
prost = "0.13"
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "timeout"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
rust-embed = "8"
tracing = "0.1"
tracing-subscriber = "0.3"
metrics = "0.24"
metrics-exporter-prometheus = "0.16"
anyhow = "1"
toml = "0.8"
uuid = "1"
```

Notably **no**: rusqlite, reqwest, provider SDKs, memory/embedding libraries. The gateway is thin.

---

## 7. Webhook Ingress

### How It Works

For push-based channels (WhatsApp, Slack Events API, generic webhooks), the platform sends HTTP requests to a public URL. The gateway receives these and forwards to the correct ZeroClaw instance via gRPC `ForwardWebhook`.

**Critical constraint**: Platforms like WhatsApp require a `200 OK` response within 5 seconds. Agent turns take 30+ seconds. Therefore, webhook processing is **asynchronous**: the gateway acks the webhook immediately, and the instance processes the message in the background. The instance sends replies to the platform via the platform's outbound API directly (not through the gateway response).

```
WhatsApp → POST https://gateway.example.com/webhook/whatsapp/inst_daniel
  → Gateway checks idempotency_key (X-Request-Id or message ID from body)
  → Gateway calls zc-daniel:50051.ForwardWebhook({
      platform: "whatsapp",
      body: <raw HTTP body>,
      headers: { "X-Hub-Signature-256": "sha256=..." },
      idempotency_key: "msg_abc123"
    })
  → ZeroClaw instance validates signature, returns WebhookAck { accepted: true }
  → Gateway returns 200 OK to WhatsApp within ~100ms
  → Meanwhile, instance processes message in background, calls WhatsApp API to send reply
```

### Idempotency

The gateway maintains an in-memory `IdempotencyStore` (TTL-based, same pattern as the existing gateway's `IdempotencyStore`). Webhook requests are deduplicated by:

1. Platform-specific message ID extracted from the body (e.g., WhatsApp `messages[0].id`)
2. Falling back to `X-Request-Id` header if present
3. Falling back to SHA-256 hash of the body

Duplicate webhooks within the TTL window (default: 5 minutes) are acked with 200 but not forwarded to the instance.

### Webhook URL Discovery

Instances need to know the gateway's public URL to register webhook callbacks with platforms (e.g., WhatsApp requires setting a webhook URL). This is provided via the gateway config and pushed to instances via `UpdateConfig`:

```toml
[gateway]
public_url = "https://gateway.example.com"

# Instance receives: gateway_webhook_base_url = "https://gateway.example.com/webhook"
# Instance can derive its own webhook URL: {base}/whatsapp/{instance_id}
```

### Instance-Side Webhook Handler

Inside the ZeroClaw instance, `ForwardWebhook` dispatches to the appropriate channel's webhook handler. Processing is async — the RPC returns immediately.

```rust
async fn forward_webhook(&self, req: WebhookRequest) -> Result<WebhookAck> {
    // Validate signature/platform
    let valid = match req.platform.as_str() {
        "whatsapp" => self.verify_whatsapp_signature(&req.body, &req.headers),
        "slack" => self.verify_slack_signature(&req.body, &req.headers),
        _ => Ok(()),
    };

    if let Err(e) = valid {
        return Ok(WebhookAck { accepted: false, error: e.to_string() });
    }

    // Spawn background task for processing (don't block the RPC)
    let session = self.session.clone();
    let platform = req.platform.clone();
    tokio::spawn(async move {
        match platform.as_str() {
            "whatsapp" => handle_whatsapp_message(session, req.body, req.headers).await,
            "slack" => handle_slack_message(session, req.body, req.headers).await,
            _ => handle_generic_webhook(session, req.body).await,
        }
    });

    Ok(WebhookAck { accepted: true, error: String::new() })
}
```

---

## 8. Web UI

### Design

Lightweight SPA for local development and testing. Served by the gateway. Minimal build step.

**Tech**: htmx + Preact (3KB) for interactive components + Tailwind CSS (self-hosted, not CDN). Embedded in the `zcgw` binary via `rust-embed`.

Why not vanilla JS? Managing a multi-instance dashboard with real-time streaming updates, state synchronization, and component lifecycle in vanilla JS becomes a maintenance burden quickly. htmx handles the server-rendered parts (dashboard, cron, memory, config) while Preact handles the chat component that needs client-side state. Total JS overhead: ~5KB gzipped.

Tailwind CSS is self-hosted (bundled into the embedded assets) to avoid the external CDN dependency and ensure the UI works in air-gapped environments.

### Views

#### Instance Selector (sidebar/header)
- Dropdown listing all instances from `/api/instances`
- Status indicator (green/red dot) per instance
- Selecting an instance scopes all views to that instance

#### Chat
- Full-height chat interface (Preact component)
- Connects to `/ws/chat?instance={id}` for streaming
- On load: fetches history from `/api/instances/{id}/history`
- Shows streaming deltas, tool calls (collapsible), final response
- Cancel button, clear history button

#### Dashboard
- Status cards per instance (model, uptime, turns, state)
- Aggregated cost summary across all instances
- Cost breakdown per instance
- Live event feed (SSE from `/api/instances/{id}/events`)

#### Cron
- Table of cron jobs for selected instance
- Create/edit/delete/run-now actions

#### Memory
- Search bar (semantic search)
- Browse by category
- Store/delete entries

#### Config
- JSON viewer/editor for selected instance
- Save + restart buttons

#### Tools & Skills
- Read-only lists

### Auth

Modal on first load: enter gateway token. Stored in `localStorage` (not `sessionStorage` — persists across tab close/reopen). Sent as `Authorization: Bearer` on all requests. A "logout" button clears the stored token.

### File Structure

```
zcgw/web-ui/
├── index.html
├── app.js              # htmx + routing
├── chat.jsx            # Preact chat component
├── components/
│   ├── dashboard.js    # htmx partials
│   ├── cron.js
│   ├── memory.js
│   ├── config.js
│   └── tools.js
├── tailwind.min.css    # Self-hosted, not CDN
└── vendor/
    ├── htmx.min.js
    └── preact.min.js
```

---

## 9. Repository Structure

```
zeroclaw-fork/
├── proto/
│   └── zeroclaw.proto              # Shared gRPC definition
├── zcgw/                           # Gateway binary (new crate)
│   ├── Cargo.toml
│   ├── build.rs                    # tonic-build for proto compilation
│   ├── src/
│   │   ├── main.rs                 # CLI entry, config loading
│   │   ├── config.rs               # Gateway config (instances, auth)
│   │   ├── server.rs               # Axum server setup, routes
│   │   ├── auth.rs                 # Per-instance bearer token middleware
│   │   ├── router.rs               # Instance routing logic
│   │   ├── ws_bridge.rs            # WebSocket ↔ gRPC bridge
│   │   ├── api.rs                  # REST handlers (proxy to gRPC)
│   │   ├── webhook.rs              # Webhook ingress + forwarding + idempotency
│   │   ├── registry.rs             # Instance registry + gRPC pool + health checks
│   │   ├── circuit_breaker.rs      # Per-instance circuit breaker
│   │   ├── rate_limiter.rs         # Token-bucket rate limiter
│   │   ├── metrics.rs              # Prometheus metrics export
│   │   ├── sse.rs                  # SSE event aggregation
│   │   └── static_files.rs         # Web UI serving
│   └── web-ui/                     # Embedded static assets
│       ├── index.html
│       ├── app.js
│       ├── chat.jsx
│       ├── tailwind.min.css
│       └── components/*.js
├── zc/                             # ZeroClaw instance (modified fork)
│   ├── Cargo.toml
│   ├── build.rs                    # tonic-build for proto compilation
│   ├── src/
│   │   ├── main.rs                 # Existing CLI + new `serve` command
│   │   ├── grpc_server.rs          # tonic service implementation
│   │   ├── agent_actor.rs          # Agent actor (dedicated tokio task)
│   │   ├── session.rs              # SessionManager (ties actor + history + metrics)
│   │   ├── history_store.rs        # SQLite conversation history (read/write split)
│   │   ├── cost_tracker.rs         # Per-instance cost tracking
│   │   ├── shutdown.rs             # Graceful shutdown coordinator
│   │   ├── agent/                  # (existing, minor modifications)
│   │   ├── providers/              # (existing, untouched)
│   │   ├── memory/                 # (existing, untouched)
│   │   ├── tools/                  # (existing, untouched)
│   │   ├── cron/                   # (existing, untouched)
│   │   ├── security/               # (existing, untouched)
│   │   ├── channels/               # (existing, untouched — channels stay!)
│   │   ├── observability/          # (existing, untouched)
│   │   ├── config/                 # (existing, untouched)
│   │   └── skills/                 # (existing, untouched)
│   └── ...
├── docker/
│   ├── Dockerfile.zcgw             # Gateway container (multi-stage, layer-cached)
│   ├── Dockerfile.zc               # ZeroClaw instance container
│   └── docker-compose.yml          # Dev deployment (with resource limits)
└── Cargo.workspace                 # Workspace root
```

### Workspace Cargo.toml

```toml
[workspace]
members = ["zcgw", "zc"]
resolver = "2"

[workspace.dependencies]
tonic = "0.12"
prost = "0.13"
tokio = { version = "1", features = ["full"] }
```

---

## 10. Docker Deployment (Dev)

### docker-compose.yml

```yaml
services:
  gateway:
    build:
      context: .
      dockerfile: docker/Dockerfile.zcgw
    ports:
      - "8080:8080"
    environment:
      ZCGW_GRPC_SECRET: "${ZCGW_GRPC_SECRET}"
      ZCGW_TOKEN_DANIEL: "${ZCGW_TOKEN_DANIEL}"
      ZCGW_CONFIG_PATH: /etc/zcgw/config.toml
      PROVIDER_API_KEY: "${PROVIDER_API_KEY}"
    volumes:
      - ./config/zcgw.toml:/etc/zcgw/config.toml:ro
    depends_on:
      zc-1:
        condition: service_healthy
    deploy:
      resources:
        limits:
          memory: 256M
          cpus: "0.5"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 10s
      timeout: 3s
      retries: 3

  zc-1:
    build:
      context: .
      dockerfile: docker/Dockerfile.zc
    command: ["zc", "serve", "--grpc-port", "50051", "--data-dir", "/data"]
    volumes:
      - zc-1-data:/data                # Persistent: memory DB, history DB, secrets
      - ./config/zc-1.toml:/etc/zc/config.toml:ro
    environment:
      ZC_CONFIG_PATH: /etc/zc/config.toml
      ZC_GRPC_SECRET: "${ZCGW_GRPC_SECRET}"
    deploy:
      resources:
        limits:
          memory: 1G
          cpus: "1.0"
        reservations:
          memory: 512M
    healthcheck:
      test: ["CMD", "/usr/local/bin/grpc-health-probe", "-addr=:50051"]
      interval: 10s
      timeout: 3s
      retries: 3
    # No CAP_SYS_ADMIN — firejail not used in containers
    security_opt:
      - no-new-privileges:true

  # Add more instances as needed:
  # zc-2:
  #   ...

volumes:
  zc-1-data:
```

### Dockerfile.zc (with layer caching)

```dockerfile
FROM rust:1.83-slim AS planner
WORKDIR /src
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:1.83-slim AS builder
WORKDIR /src
RUN cargo install cargo-chef
COPY --from=planner /src/recipe.json recipe.json
# Cache dependency build layer
RUN cargo chef cook --release --recipe-path recipe.json -p zc
COPY . .
RUN cargo build --release -p zc

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
# Install grpc-health-probe for container health checks
RUN curl -fsSL https://github.com/grpc-ecosystem/grpc-health-probe/releases/download/v0.4.25/grpc_health_probe-linux-amd64 \
    -o /usr/local/bin/grpc-health-probe && chmod +x /usr/local/bin/grpc-health-probe
COPY --from=builder /src/target/release/zc /usr/local/bin/zc
RUN useradd -r -s /bin/false zc && mkdir -p /data && chown zc:zc /data
USER zc
VOLUME /data
EXPOSE 50051
# Graceful shutdown: Docker sends SIGTERM, zc handles it
STOPSIGNAL SIGTERM
CMD ["zc", "serve", "--grpc-port", "50051", "--data-dir", "/data"]
```

### Dockerfile.zcgw (with layer caching)

```dockerfile
FROM rust:1.83-slim AS planner
WORKDIR /src
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust:1.83-slim AS builder
WORKDIR /src
RUN cargo install cargo-chef
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p zcgw
COPY . .
RUN cargo build --release -p zcgw

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/zcgw /usr/local/bin/zcgw
RUN useradd -r -s /bin/false zcgw
USER zcgw
EXPOSE 8080
STOPSIGNAL SIGTERM
CMD ["zcgw", "--port", "8080"]
```

---

## 11. Future: Kubernetes Deployment

Not implemented now, but the architecture is designed for it:

```
┌────────────────────────────────────────────────┐
│              Kubernetes Cluster                 │
│                                                 │
│  Deployment: zcgw (1 replica)                  │
│    - Service: zcgw-svc (LoadBalancer/Ingress)  │
│    - ConfigMap: zcgw-config                    │
│    - Secrets: zcgw-tokens, grpc-secret         │
│                                                 │
│  StatefulSet: zc (N replicas)                  │
│    - Service: zc-headless (ClusterIP: None)    │
│    - PVC template: 10Gi per pod                │
│    - Pod names: zc-0, zc-1, ..., zc-9         │
│    - gRPC address: zc-{n}.zc-headless:50051   │
│                                                 │
│  Future: Gateway discovers pods via K8s API    │
│  or headless service DNS resolution            │
└────────────────────────────────────────────────┘
```

Each `zc` pod has:
- PersistentVolumeClaim for `/data` (memory DB, history DB, secrets key)
- gRPC health check probe on port 50051
- Resource limits (CPU/memory based on model usage patterns)

**Gateway SPOF note**: In v1, the gateway is a single replica. This is acceptable for ~10 instances. For higher availability, run 2+ gateway replicas behind a load balancer. The gateway is stateless except for the in-memory idempotency store and rate limiter state, which are acceptable to lose on restart (idempotency has a short TTL, rate limiter resets are benign). Circuit breaker state is also in-memory and resets conservatively.

---

## 12. Graceful Shutdown

### Shutdown Sequence (zc instance)

When a `zc` container receives SIGTERM (e.g., `docker stop`, Kubernetes pod termination):

```
1. SIGTERM received
   │
2. Stop accepting new gRPC connections
   │  (tonic server graceful shutdown)
   │
3. Cancel any in-flight agent turn
   │  (via CancellationToken)
   │  Wait up to 10s for current LLM call to abort
   │
4. Drain active gRPC streams
   │  (send TurnError "shutting down" to connected clients)
   │
5. Stop cron scheduler
   │  (cancel pending cron triggers, let running cron finish up to 5s)
   │
6. Stop channel listeners
   │  (Telegram: stop polling, Discord: disconnect WebSocket)
   │
7. Flush HistoryStore WAL
   │  (PRAGMA wal_checkpoint(FULL))
   │
8. Flush memory database
   │  (checkpoint SQLite WAL for memory DB too)
   │
9. Exit cleanly
```

Implementation:

```rust
async fn run_serve(config: Config, grpc_port: u16, data_dir: PathBuf) -> Result<()> {
    let shutdown_token = CancellationToken::new();
    let shutdown_token_clone = shutdown_token.clone();

    // Register SIGTERM handler
    tokio::spawn(async move {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).unwrap();
        sigterm.recv().await;
        tracing::info!("SIGTERM received, initiating graceful shutdown");
        shutdown_token_clone.cancel();
    });

    // Start all subsystems with shutdown_token...
    // Wait for shutdown_token, then run shutdown sequence
}
```

Docker is configured with `stop_grace_period: 30s` (default) which gives ample time for the shutdown sequence.

### Shutdown Sequence (zcgw gateway)

```
1. SIGTERM received
2. Stop accepting new HTTP connections
3. Drain active WebSocket connections (send close frame)
4. Wait up to 5s for in-flight REST requests to complete
5. Close all gRPC connections in the pool
6. Exit cleanly
```

---

## 13. Security Hardening

### Gateway-to-Instance Authentication

All gRPC calls from the gateway to instances carry a shared secret in the `authorization` metadata header. The instance rejects any request without a valid secret. This prevents:
- Rogue processes on the same network from accessing instance gRPC ports
- Accidental cross-instance access

```
ZCGW_GRPC_SECRET="$(openssl rand -hex 32)"
```

In production (K8s), additionally use:
- mTLS between gateway and instances (via service mesh like Istio/Linkerd)
- Network policies restricting which pods can reach instance gRPC ports

### Per-Instance Client Tokens

Each instance has a unique bearer token for client access. This prevents one user from accessing another user's instance. Tokens are configured in the gateway config, not hardcoded.

### API Key Transit

Provider API keys are pushed from gateway to instance via gRPC `UpdateConfig`. In production:
- gRPC channel should use TLS
- Instance holds key in memory only (not persisted to disk)
- Key is masked in `GetConfig` responses

### Container Security

- Containers run as non-root user
- `no-new-privileges` security option
- No `CAP_SYS_ADMIN` (firejail not used in containers)
- Read-only config mounts
- Named volumes for data persistence (not bind mounts in production)

### Config Validation

Instance validates `UpdateConfig` requests before applying:
- JSON must parse successfully
- Known fields must have valid types/ranges
- Unknown fields are rejected (no silent pass-through)
- Security-sensitive fields (sandbox mode, autonomy level) cannot be weakened via API

---

## 14. Observability

### Gateway Metrics (Prometheus)

The gateway exports metrics at `GET /api/metrics` in Prometheus format:

```
# Request counts
zcgw_http_requests_total{method, path, status, instance}
zcgw_ws_connections_active{instance}
zcgw_ws_messages_total{direction, instance}

# Latency
zcgw_http_request_duration_seconds{method, path, instance}
zcgw_grpc_request_duration_seconds{method, instance}

# gRPC pool
zcgw_grpc_connections_active{instance}
zcgw_grpc_connection_errors_total{instance}

# Circuit breaker
zcgw_circuit_breaker_state{instance}  # 0=closed, 1=open, 2=half-open
zcgw_circuit_breaker_trips_total{instance}

# Rate limiting
zcgw_rate_limit_rejected_total{instance, scope}

# Webhooks
zcgw_webhook_received_total{platform, instance}
zcgw_webhook_deduplicated_total{platform, instance}
zcgw_webhook_forwarded_total{platform, instance}

# Errors
zcgw_errors_total{type, instance}
```

### Instance Metrics

Instances already have the existing ZeroClaw observability system (Observer trait, runtime trace). The gRPC `SubscribeEvents` stream surfaces these to the gateway. Additionally:

- `zc_grpc_requests_total{method}` — gRPC request count
- `zc_agent_turns_total` — total agent turns
- `zc_agent_turn_duration_seconds` — histogram of turn durations
- `zc_history_entries` — gauge of history size

### Log Aggregation

**Phase 1**: Structured JSON logs (`tracing-subscriber` with JSON formatter) written to stdout. Docker captures these. View with `docker compose logs`.

**Phase 2 (K8s)**: Cluster-level log aggregation (Loki, EFK, or CloudWatch). No application changes needed — structured JSON logs are already compatible.

### Pull-Based Channel Visibility

Pull-based channels (Telegram, Discord) bypass the gateway. Their activity is visible via:
1. Instance's `SubscribeEvents` gRPC stream (gateway subscribes and surfaces as SSE)
2. Instance metrics (turn counts, cost tracking include channel-originated turns)
3. Instance logs (structured, includes channel name in every log entry)

---

## 15. Multimodal / File Upload

File uploads (images for vision, documents) are supported via the `FileAttachment` field in `SendMessageRequest`. The flow:

1. Client sends file via WebSocket as a binary message (or base64 in JSON)
2. Gateway proxies to gRPC `SendMessage` with `FileAttachment` populated
3. Instance stores file temporarily and passes as image marker to the agent
4. Agent uses existing multimodal support (`multimodal::prepare_messages_for_provider`)

This preserves existing multimodal capabilities. Size limit: 10MB per file, enforced at the gateway.

**Deferred**: Streaming file upload for large files. For v1, files are sent inline in the gRPC message. This is acceptable for images (typically <5MB) but would need chunked upload for large documents.

---

## 16. Implementation Phases

```
Phase 0: Repository Setup
    │   Cargo workspace, proto file, basic crate scaffolding
    │
    ▼
Phase 1: zc — Agent Actor + gRPC Server + HistoryStore
    │   The core: agent actor with run_tool_call_loop,
    │   persistent history, gRPC interface
    │   (most complex phase — everything depends on this)
    │
    ├──► Phase 2: zcgw — Skeleton + Auth + Registry + Pool
    │    (can begin once proto is stable)
    │
    ▼
Phase 3: zcgw — WebSocket↔gRPC Bridge + REST Proxy
    │   (depends on Phase 1 + 2)
    │
    ├──► Phase 4: zc — Webhook Forwarding (async, with idempotency)
    │    (independent of gateway work)
    │
    ├──► Phase 5: zcgw — Webhook Ingress + Idempotency
    │    (depends on Phase 4)
    │
    ▼
Phase 6: Docker Compose + Graceful Shutdown
    │   Build both containers, test e2e, verify shutdown
    │
    ├──► Phase 7: Security Hardening
    │    (shared secret, per-instance tokens, config validation)
    │
    ▼
Phase 8: Web UI
    │   (depends on Phase 3 — needs stable gateway API)
    │
    ▼
Phase 9: Observability + Metrics
    │
    ▼
Phase 10: Testing & Polish
```

### Phase 0: Repository Setup (~2 days)

1. Create Cargo workspace with `zcgw` and `zc` crates
2. Write `proto/zeroclaw.proto`
3. Add `build.rs` to both crates for `tonic-build`
4. Verify proto compiles with `cargo build`
5. Set up `zc` crate by moving/forking ZeroClaw source
6. Set up CI for workspace builds

### Phase 1: zc — Core (~2-3 weeks)

This is the hardest phase. The Agent Actor + `run_tool_call_loop` wiring is the most complex integration point.

1. Implement `HistoryStore` (SQLite, WAL mode, read/write split) — 2 days
2. Implement `AgentActor` (dedicated tokio task, command channel, `run_tool_call_loop` wiring) — 4-5 days
   - This requires extracting all 17 parameters from Config and wiring them into the actor
   - Must handle streaming deltas (on_delta channel) and cancellation
   - Must handle concurrent access patterns (reads don't block the actor)
3. Implement `SessionManager` (ties actor + history + metrics) — 2 days
4. Implement `CostTracker` — 1 day
5. Implement `ClawAgentService` (gRPC service — all RPCs) — 3 days
6. Add `serve` CLI command with graceful shutdown coordinator — 1 day
7. Test: start instance, send gRPC chat, verify streaming + history persistence — 2 days

### Phase 2: zcgw — Skeleton (~1 week)

1. Axum server with per-instance auth middleware
2. Instance registry (static config) with gRPC connection pool
3. Health check background loop with circuit breaker
4. Rate limiter (token bucket)
5. `GET /health`, `GET /api/instances` endpoints
6. Shared secret authentication for gRPC calls

### Phase 3: zcgw — Full Proxy (~1.5 weeks)

1. WebSocket <-> gRPC bridge (`/ws/chat`) with backpressure
2. REST proxy handlers for all instance-scoped endpoints
3. SSE event aggregation
4. API key distribution on startup

### Phase 4: zc — Webhook Forwarding (~3 days)

1. Implement `ForwardWebhook` RPC (async processing)
2. Wire to existing channel webhook handlers
3. Configure webhook URL discovery from gateway
4. Test with mock webhook payloads

### Phase 5: zcgw — Webhook Ingress (~3 days)

1. `POST /webhook/{platform}/{instance_id}` routes
2. `GET /webhook/{platform}/{instance_id}` for verification
3. Idempotency store (TTL-based, in-memory)
4. Forward to instance via gRPC, return 200 immediately

### Phase 6: Docker + Shutdown (~1 week)

1. Write Dockerfiles with `cargo-chef` layer caching
2. Write `docker-compose.yml` with resource limits and health checks
3. Implement graceful shutdown in `zc` (SIGTERM handling, WAL flush)
4. Implement graceful shutdown in `zcgw` (drain connections)
5. Test: `docker compose up`, send messages, `docker compose stop`, verify clean shutdown

### Phase 7: Security Hardening (~3 days)

1. Per-instance bearer tokens in gateway config
2. Shared secret for gRPC authentication
3. Config validation on `UpdateConfig`
4. Container security (non-root, no-new-privileges)
5. Sandbox mode detection (skip firejail in containers)

### Phase 8: Web UI (~1.5 weeks)

1. HTML shell + htmx + Preact + self-hosted Tailwind
2. Instance selector
3. Chat view (Preact component, WebSocket streaming)
4. Dashboard view (status + cost)
5. Cron, Memory, Config, Tools views (htmx)
6. Auth modal (localStorage)

### Phase 9: Observability (~3 days)

1. Gateway Prometheus metrics endpoint
2. Instance gRPC metrics
3. Structured JSON logging in both binaries
4. Wire SubscribeEvents for pull-based channel visibility

### Phase 10: Testing & Polish (~1 week)

1. End-to-end: gateway -> gRPC -> agent actor -> streaming -> WebSocket -> client
2. History persistence: kill zc container, restart, verify history restored
3. Multi-instance: two zc containers, switch between them in UI
4. Webhook forwarding: simulate WhatsApp webhook through gateway (verify <5s ack)
5. Config update: change model via API, restart agent
6. Concurrent clients: two browser tabs chatting with same instance
7. Graceful shutdown: verify no data loss on `docker stop`
8. Security: verify cross-instance token isolation
9. Circuit breaker: kill an instance, verify gateway degrades gracefully
10. Backpressure: slow client, verify no unbounded buffering

### Total Estimate: ~8-10 weeks

| Phase | Estimate |
|-------|----------|
| Phase 0: Setup | 2 days |
| Phase 1: zc Core | 2-3 weeks |
| Phase 2: zcgw Skeleton | 1 week |
| Phase 3: zcgw Full Proxy | 1.5 weeks |
| Phase 4: zc Webhooks | 3 days |
| Phase 5: zcgw Webhooks | 3 days |
| Phase 6: Docker + Shutdown | 1 week |
| Phase 7: Security | 3 days |
| Phase 8: Web UI | 1.5 weeks |
| Phase 9: Observability | 3 days |
| Phase 10: Testing | 1 week |

Phases 2-3 can overlap with Phase 1 once the proto is stable. Phases 4-5 can overlap with Phase 3. Total calendar time with parallelism: ~8 weeks for a single developer.

---

## 17. Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| Agent Actor wiring (17 params) is complex | High | Spike this first in Phase 1. If too brittle, consider refactoring `run_tool_call_loop` to take a config struct. |
| gRPC adds latency to every interaction | Low | Same datacenter/node — sub-millisecond. HTTP/2 multiplexing keeps it fast. |
| Instance goes down, gateway doesn't notice | Medium | 30-second health check loop + circuit breaker. Clients get 503 on next request. |
| History DB in container volume lost | High | PersistentVolumeClaim in K8s. Docker named volumes in Compose. Document backup. |
| gRPC streaming disconnects mid-turn | Medium | Agent continues turn to completion. Gateway reconnects, client fetches history for missed content. |
| Proto schema evolution breaks compatibility | Low | Append-only field numbers. Deploy gateway + instances from same commit. |
| Provider API key in transit (gateway → instance) | Medium | gRPC over TLS in production. In dev (same Docker network), acceptable. |
| Instance container OOM from large agent context | Medium | Resource limits in docker-compose. Agent history compaction prevents unbounded growth. |
| Gateway is single point of failure | Medium | Acceptable for ~10 instances. Gateway is stateless and restarts in <1s. Future: multiple replicas behind LB. |
| SQLite performance on Docker volumes | Low | WAL mode + local volumes. Avoid NFS. Document this constraint. |
| Pull-based channels invisible to gateway | Low | Visible via SubscribeEvents stream and instance metrics. Document this tradeoff. |

---

## 18. Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | **Channels stay in ZeroClaw instance** | Stateful channels (Telegram, Discord) require persistent connections. Per-user credentials stay isolated. Gateway forwards webhooks via gRPC for push-based platforms. |
| 2 | **Chat history in zc, not gateway** | History is agent state. Co-located with memory and agent for consistency. Survives gateway restarts. |
| 3 | **gRPC between gateway and instances** | Native server streaming, typed schema, code generation, K8s-native. |
| 4 | **Per-instance auth tokens** | Multi-tenant isolation. One token per instance, not one token for all. |
| 5 | **Shared secret for gRPC auth** | Prevents unauthorized access to instance gRPC ports. |
| 6 | **Unary + server-stream for Chat** | Simpler than bidi streaming. Reconnection, cancellation, and error handling are all cleaner. |
| 7 | **Agent Actor pattern** | Agent takes `&mut self`. Cannot be wrapped in Mutex across async. Actor pattern with mpsc channel is the correct solution. |
| 8 | **`run_tool_call_loop` not `Agent::turn()`** | Only `run_tool_call_loop` supports `on_delta` streaming and `CancellationToken`. Required for gRPC streaming and cancellation. |
| 9 | **Async webhook processing** | Platform timeouts (WhatsApp: 5s) require immediate ack. Instance processes in background, replies via platform API. |
| 10 | **Shared provider API key by default** | Gateway distributes key to instances. Per-instance keys supported but not default. |
| 11 | **Static instance registry for now** | Config file lists instances. Dynamic K8s discovery is future work. |
| 12 | **No firejail in containers** | Requires CAP_SYS_ADMIN. Rely on container isolation instead. |
| 13 | **htmx + Preact for Web UI** | Not vanilla JS (maintenance burden for multi-instance dashboard). Not a full framework (unnecessary complexity). htmx for server-rendered views, Preact for chat component. |
| 14 | **localStorage for auth token** | Persists across tab close. sessionStorage loses token on every tab close. |
| 15 | **Cron runs inside zc** | Per your input. Gateway only reads cron state, doesn't execute. |
| 16 | **No dark mode yet** | Per your input. |

---

## 19. Intentionally Deferred

| # | Item | Reason |
|---|------|--------|
| D1 | **Proto-native config types** | Config is a complex evolving Rust struct. JSON bridge is pragmatic for v1. Extracting stable proto types is a v2 concern. (Issue #23) |
| D2 | **Streaming file upload** | Inline file attachment in gRPC message is fine for images (<10MB). Chunked upload is a v2 concern. (Issue #26 partial) |
| D3 | **Multi-replica gateway HA** | Single gateway is acceptable for ~10 instances. Stateless design makes multi-replica trivial when needed. (Issue #33) |
| D4 | **Log aggregation infrastructure** | Structured JSON to stdout is sufficient for Phase 1. Loki/EFK is infra, not application work. (Issue #24) |
| D5 | **Proto cross-version compatibility** | Deploy from same commit. Backward compat is a v2 concern when independent deployments are needed. (Issue #27) |

---

## 20. Open Questions

| # | Question | Context |
|---|----------|---------|
| 1 | **What should the project be called?** | Need names for: workspace, gateway binary (`zcgw`?), instance binary (`zc`?), env var prefix, Docker image names. The names above are placeholders. |
| 2 | **Should gRPC use TLS from day one?** | In Docker Compose (same network), plaintext + shared secret is acceptable. In K8s, mTLS via service mesh (Istio/Linkerd) is preferred over application-level TLS. Do you want TLS support built in, or defer to infra? |
| 3 | **Instance provisioning scope**: Should the gateway be able to **spawn new containers** (via Docker API or K8s API), or is instance lifecycle managed externally? | Affects whether `POST /api/instances` is a create-container action or just a registry update. |
| 4 | **Multi-session per instance?** | Currently: one continuous conversation per zc instance. Should there be multiple named conversations (e.g., "project-alpha", "debugging")? |
| 5 | **`run_tool_call_loop` refactor**: Should we refactor the 17-parameter function to take a config struct before starting the fork? | Would simplify Phase 1 significantly and reduce coupling. Risk: touching existing code pre-fork. |
