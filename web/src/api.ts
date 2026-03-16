/** Lightweight fetch wrapper for the zcgw REST + WebSocket API. */

let _token = "";

function saveCookie(token: string) {
  const maxAge = token ? 86400 * 30 : 0; // 30 days, or expire immediately
  document.cookie = `zc_token=${encodeURIComponent(token)}; path=/; max-age=${maxAge}; SameSite=Strict`;
}

function loadCookie(): string {
  const match = document.cookie.match(/(?:^|;\s*)zc_token=([^;]*)/);
  return match?.[1] ? decodeURIComponent(match[1]) : "";
}

export function setToken(t: string) {
  _token = t;
  saveCookie(t);
}

export function getToken() {
  return _token;
}

export function restoreToken(): string {
  const saved = loadCookie();
  if (saved) _token = saved;
  return saved;
}

function headers(): Record<string, string> {
  return {
    Authorization: `Bearer ${_token}`,
    "Content-Type": "application/json",
  };
}

async function api<T>(path: string, opts?: RequestInit): Promise<T> {
  const resp = await fetch(path, { ...opts, headers: { ...headers(), ...opts?.headers } });
  if (resp.status === 401) throw new Error("Unauthorized");
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  return resp.json() as Promise<T>;
}

// ── Instances ──
export interface InstanceInfo {
  id: string;
  display_name: string;
  grpc_address: string;
  health: "healthy" | "unhealthy" | "unknown";
}

export const listInstances = () => api<InstanceInfo[]>("/api/instances");

// ── Status ──
export interface StatusInfo {
  state: string;
  uptime_secs: number;
  total_turns: number;
  history_length: number;
  model: string;
  provider: string;
}

export const getStatus = (id: string) =>
  api<StatusInfo>(`/api/instances/${encodeURIComponent(id)}/status`);

// ── History ──
export interface HistoryMessage {
  turn_index: number;
  role: string;
  content: string;
  created_at: string;
}

export interface HistoryResponse {
  total: number;
  offset: number;
  limit: number;
  messages: HistoryMessage[];
}

export const getHistory = (id: string, limit = 200) =>
  api<HistoryResponse>(`/api/instances/${encodeURIComponent(id)}/history?limit=${limit}`);

export const clearHistory = (id: string) =>
  api<{ messages_cleared: number }>(`/api/instances/${encodeURIComponent(id)}/history`, {
    method: "DELETE",
  });

// ── Config ──
export const getConfig = (id: string) =>
  api<Record<string, unknown>>(`/api/instances/${encodeURIComponent(id)}/config`);

export const updateConfig = (id: string, data: Record<string, unknown>) =>
  api<{ updated_fields: string[]; requires_restart: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/config`,
    { method: "PUT", body: JSON.stringify(data) },
  );

// ── Tools ──
export interface ToolInfo {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

export const listTools = (id: string) =>
  api<{ tools: ToolInfo[] }>(`/api/instances/${encodeURIComponent(id)}/tools`);

// ── Memory ──
export interface MemoryEntry {
  key: string;
  content: string;
  category: string;
  timestamp: string;
  score?: number;
}

export interface MemoryListResponse {
  entries: MemoryEntry[];
  total: number;
}

export const listMemory = (id: string, limit = 200) =>
  api<MemoryListResponse>(`/api/instances/${encodeURIComponent(id)}/memory?limit=${limit}`);

export const searchMemory = (id: string, query: string, limit = 50) =>
  api<MemoryListResponse>(
    `/api/instances/${encodeURIComponent(id)}/memory/search?query=${encodeURIComponent(query)}&limit=${limit}`,
  );

export const storeMemory = (id: string, key: string, content: string, category: string) =>
  api<{ ok: boolean }>(`/api/instances/${encodeURIComponent(id)}/memory`, {
    method: "POST",
    body: JSON.stringify({ key, content, category }),
  });

export const forgetMemory = (id: string, key: string) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/memory/${encodeURIComponent(key)}`,
    { method: "DELETE" },
  );

// ── Admin ──
export interface AdminStats {
  uptime_secs: number;
  started_at: string;
  total_instances: number;
  healthy_count: number;
  unhealthy_count: number;
}

export interface DetailedInstance extends InstanceInfo {
  container_status?: string;
}

export const getAdminStats = () => api<AdminStats>("/api/admin/stats");

export const getAdminConfig = () => api<{ raw: string }>("/api/admin/config");

export const updateAdminConfig = (raw: string) =>
  api<{ requires_restart: boolean }>("/api/admin/config", {
    method: "PUT",
    body: JSON.stringify({ raw }),
  });

export const listAdminInstances = () =>
  api<DetailedInstance[]>("/api/admin/instances");

export const createInstance = (id: string, display_name: string, config_toml: string) =>
  api<DetailedInstance>("/api/admin/instances", {
    method: "POST",
    body: JSON.stringify({ id, display_name, config_toml }),
  });

export const instanceAction = (
  id: string,
  action: "start" | "stop" | "destroy" | "reconnect" | "restart",
  confirm?: boolean,
) =>
  api<{ ok: boolean }>(`/api/admin/instances/${encodeURIComponent(id)}/action`, {
    method: "POST",
    body: JSON.stringify({ action, confirm }),
  });

export const getAgentTemplate = () => api<{ raw: string }>("/api/admin/template");

// ── Identity ──
export interface IdentityFile {
  filename: string;
  content: string;
}

export interface IdentityListResponse {
  files: IdentityFile[];
  known_files: string[];
}

export const listIdentity = (id: string) =>
  api<IdentityListResponse>(`/api/instances/${encodeURIComponent(id)}/identity`);

export const updateIdentityFile = (id: string, filename: string, content: string) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/identity/${encodeURIComponent(filename)}`,
    { method: "PUT", body: JSON.stringify({ content }) },
  );

export const deleteIdentityFile = (id: string, filename: string) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/identity/${encodeURIComponent(filename)}`,
    { method: "DELETE" },
  );

export const batchUpdateIdentity = (id: string, files: IdentityFile[]) =>
  api<{ ok: boolean; saved: number }>(
    `/api/instances/${encodeURIComponent(id)}/identity`,
    { method: "PUT", body: JSON.stringify({ files }) },
  );

// ── Connectors ──
export interface ChannelField {
  name: string;
  label: string;
  field_type: string; // "string" | "bool" | "string_list" | "u64" | "select:opt1,opt2,..."
  required: boolean;
  sensitive: boolean;
  help: string;
}

export interface ChannelSchema {
  channel_type: string;
  label: string;
  fields: ChannelField[];
}

export interface ConnectorsResponse {
  channels_config: Record<string, unknown>;
  channel_schema: ChannelSchema[];
}

export const getConnectors = (id: string) =>
  api<ConnectorsResponse>(`/api/instances/${encodeURIComponent(id)}/connectors`);

export const updateConnectors = (id: string, channels_config: Record<string, unknown>) =>
  api<{ ok: boolean; requires_restart: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/connectors`,
    { method: "PUT", body: JSON.stringify({ channels_config }) },
  );

// ── MCP Servers ──
export interface McpServerConfig {
  name: string;
  transport: "sse" | "stdio";
  url?: string;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  enabled: boolean;
}

export const getMcpServers = (id: string) =>
  api<{ mcp_servers: McpServerConfig[] }>(`/api/instances/${encodeURIComponent(id)}/mcp-servers`);

export const updateMcpServers = (id: string, mcp_servers: McpServerConfig[]) =>
  api<{ ok: boolean; requires_restart: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/mcp-servers`,
    { method: "PUT", body: JSON.stringify({ mcp_servers }) },
  );

// ── Integrations: Composio ──
export interface ComposioConfig {
  enabled: boolean;
  entity_id: string;
  has_api_key: boolean;
}

export const getComposio = (id: string) =>
  api<ComposioConfig>(`/api/instances/${encodeURIComponent(id)}/integrations/composio`);

export const updateComposio = (
  id: string,
  data: { enabled?: boolean; api_key?: string; entity_id?: string },
) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/integrations/composio`,
    { method: "PUT", body: JSON.stringify(data) },
  );

// ── Skills ──
export interface SkillFile {
  name: string;
  content: string;
}

export const listSkills = (id: string) =>
  api<{ skills: SkillFile[] }>(`/api/instances/${encodeURIComponent(id)}/skills`);

export const updateSkill = (id: string, name: string, content: string) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/skills/${encodeURIComponent(name)}`,
    { method: "PUT", body: JSON.stringify({ content }) },
  );

export const deleteSkill = (id: string, name: string) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/skills/${encodeURIComponent(name)}`,
    { method: "DELETE" },
  );

// ── Cron Jobs ──
export interface CronJob {
  id: string;
  name: string;
  expression: string;
  schedule: string;
  command: string;
  prompt: string;
  job_type: string; // "shell" | "agent"
  enabled: boolean;
  next_run: string;
  last_run: string;
  last_status: string;
  last_output: string;
  created_at: string;
  session_target: string;
  model: string;
  delivery: string;
  delete_after_run: boolean;
}

export interface CronRun {
  id: number;
  job_id: string;
  started_at: string;
  finished_at: string;
  status: string;
  output: string;
  duration_ms: number;
}

export const listCronJobs = (id: string) =>
  api<{ jobs: CronJob[] }>(`/api/instances/${encodeURIComponent(id)}/cron`);

export const getCronRuns = (id: string, jobId: string, limit = 20) =>
  api<{ runs: CronRun[] }>(
    `/api/instances/${encodeURIComponent(id)}/cron/${encodeURIComponent(jobId)}/runs?limit=${limit}`,
  );

export const createCronJob = (
  id: string,
  job: { name: string; expression: string; job_type: string; command?: string; prompt?: string },
) =>
  api<{ ok: boolean; id: string }>(`/api/instances/${encodeURIComponent(id)}/cron`, {
    method: "POST",
    body: JSON.stringify(job),
  });

export const updateCronJob = (id: string, jobId: string, patch: Record<string, unknown>) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/cron/${encodeURIComponent(jobId)}`,
    { method: "PUT", body: JSON.stringify(patch) },
  );

export const deleteCronJob = (id: string, jobId: string) =>
  api<{ ok: boolean }>(
    `/api/instances/${encodeURIComponent(id)}/cron/${encodeURIComponent(jobId)}`,
    { method: "DELETE" },
  );

// ── WebSocket ──
export function connectChat(instanceId: string): WebSocket {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const url = `${proto}//${location.host}/ws/chat?instance=${encodeURIComponent(instanceId)}&token=${encodeURIComponent(_token)}`;
  return new WebSocket(url);
}
