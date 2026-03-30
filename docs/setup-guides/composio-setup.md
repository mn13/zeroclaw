# Composio Integration Setup

Composio provides managed OAuth connections to 1000+ apps (Gmail, Slack, Notion, GitHub, etc.) without storing raw tokens locally. ZeroClaw integrates Composio at the gateway level — connections are created centrally and assigned to individual agent instances.

## Prerequisites

1. A [Composio](https://composio.dev) account and API key.
2. A running ZeroClaw gateway (`zcgw`).
3. A publicly reachable hostname for OAuth callbacks (or a tunnel).

## Environment Variables

Set these on the gateway before starting `zcgw`:

| Variable | Required | Description |
|----------|----------|-------------|
| `COMPOSIO_API_KEY` | Yes | Your Composio API key. |
| `COMPOSIO_REDIRECT_HOST` | Yes | Public hostname for OAuth callbacks (e.g. `https://gw.example.com`). Falls back to `ZEROCLAW_GOOGLE_REDIRECT_HOST` if unset. |
| `ZCGW_TENANT_ID` | No | Tenant identifier when multiple gateways share one Composio API key. Must be lowercase alphanumeric with hyphens. |

## Connection Lifecycle

Setting up a Composio integration follows five steps. Each step builds on the previous one.

```
┌─────────┐    ┌──────┐    ┌────────┐    ┌──────────┐    ┌──────────────┐
│ Connect │───▶│ Sync │───▶│ Assign │───▶│ MCP Sync │───▶│ Sync Gateway │
└─────────┘    └──────┘    └────────┘    └──────────┘    └──────────────┘
```

### Step 1 — Connect (OAuth)

Create a new OAuth connection via the gateway. This opens a browser-based auth flow with the target service (e.g. Gmail, GitHub).

**Web UI:** Go to **Integrations > Composio**, select an app from the dropdown, enter a name, and click **Connect**. A browser window opens for OAuth authorization.

**API:**
```bash
curl -X POST https://gw.example.com/api/admin/composio/connect \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"app": "gmail", "name": "Work Gmail"}'
```

The response contains a `redirect_url` — open it in a browser to complete authorization. After the OAuth flow completes, Composio redirects to the gateway callback (`/composio/callback`) which stores the connection automatically.

You can optionally pass `"instance_id": "my-agent"` to auto-assign the connection to an instance in the same call.

### Step 2 — Sync Connections

After the OAuth callback completes, the connection is usually stored automatically. Run a sync to ensure the local store is up to date with Composio's backend (especially useful if connections were created outside the gateway, or if the callback was missed).

**Web UI:** Click **Sync from Composio** in the Composio tab.

**API:**
```bash
curl -X POST https://gw.example.com/api/admin/composio/sync \
  -H "Authorization: Bearer <token>"
```

Only connections whose `user_id` matches the gateway prefix (`zcgw-` or `zcgw-{tenant}-`) are imported.

### Step 3 — Assign to Instance

Assign the connection to the agent instance that should use it.

**Web UI:** On the instance's Composio integration panel, select a connection and click **Assign**.

**API:**
```bash
curl -X POST https://gw.example.com/api/instances/my-agent/integrations/composio/assign \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"connection_id": "conn-abc123"}'
```

A single connection can be assigned to multiple instances. Use the `DELETE /api/instances/{id}/integrations/composio/connections/{connection_id}` endpoint to unassign.

### Step 4 — MCP Sync

Create Composio-hosted MCP server entries for all toolkits assigned to the instance. This registers MCP servers on Composio's backend and writes `mcp_servers` entries (using `streamable-http` transport) to the instance config.

**Web UI:** Click **Sync MCP Servers** on the instance's Composio panel.

**API:**
```bash
curl -X POST https://gw.example.com/api/instances/my-agent/integrations/composio/mcp-sync \
  -H "Authorization: Bearer <token>"
```

The gateway reuses existing MCP servers if they already exist on Composio's backend (matched by name). Server names follow the pattern `zcgw-{slug}` (or `zcgw-{tenant}-{slug}` when `ZCGW_TENANT_ID` is set).

### Step 5 — Sync Gateway Credentials

Push the gateway's API key, entity ID, and `connected_accounts` mappings to the instance config. This enables the agent's built-in `composio` tool to resolve connections directly without querying by `user_id`.

**Web UI:** Toggle **Sync from Gateway** when updating the instance's Composio settings.

**API:**
```bash
curl -X PUT https://gw.example.com/api/instances/my-agent/integrations/composio \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"enabled": true, "sync_gateway": true}'
```

This writes the following to the instance's `config.toml`:

```toml
[composio]
enabled = true
api_key = "<gateway's API key>"
entity_id = "zcgw-gateway"
connected_accounts = { gmail = "conn-abc123" }
```

## Verifying the Setup

After completing all five steps, verify the integration is working:

1. **Check instance status:** `GET /api/instances/{id}/integrations/composio` — should show `enabled: true`, `has_api_key: true`, and the assigned connections.
2. **Check MCP servers:** The instance config should contain `[[mcp_servers]]` entries named `composio-{slug}`.
3. **Test from the agent:** The agent can now use the `composio` tool to list and execute actions:
   - `composio list` — list available actions
   - `composio list_accounts` — verify connected accounts are visible
   - `composio execute` — run an action (e.g. send an email via Gmail)

## Standalone (Non-Gateway) Setup

For agents running without the gateway, configure Composio directly in `config.toml`:

```toml
[composio]
enabled = true
api_key = "your-composio-api-key"
entity_id = "default"
```

The agent can then use the `composio connect` action to initiate OAuth flows directly and `composio execute` to run actions. Connected accounts are resolved via the Composio API using the configured `entity_id`.

## Multi-Tenant Deployments

When multiple gateways share a single Composio API key, set `ZCGW_TENANT_ID` on each gateway to prevent identity collisions:

- Gateway identity becomes `zcgw-{tenant}-gateway` instead of `zcgw-gateway`
- MCP server names become `zcgw-{tenant}-{slug}` instead of `zcgw-{slug}`
- Only connections matching the tenant prefix are imported during sync

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `COMPOSIO_API_KEY not configured` | Env var not set on gateway | Set `COMPOSIO_API_KEY` and restart `zcgw` |
| `COMPOSIO_REDIRECT_HOST not configured` | No public callback URL | Set `COMPOSIO_REDIRECT_HOST` to your gateway's public URL |
| OAuth callback shows "completed" but no connection appears | Callback URL unreachable or API key mismatch | Verify `COMPOSIO_REDIRECT_HOST` resolves to the gateway; run **Sync from Composio** |
| `No Composio connections found for this instance` on MCP sync | No connections assigned to this instance | Assign connections first (Step 3) |
| Agent can't find connected accounts | Gateway credentials not synced | Run Step 5 (Sync Gateway) to push `connected_accounts` to instance config |
| Stale tool versions | Composio caches tool definitions | ZeroClaw requests `toolkit_versions=latest` by default; if still stale, recreate the MCP server |

## Related Documentation

- [Configuration Reference — `[composio]`](../reference/api/config-reference.md) — all config keys and defaults
- [API Reference — Composio Endpoints](../architecture/api-reference.md) — full endpoint documentation
- [Gateway Architecture](../architecture/gateway.md) — environment variables and gateway setup
