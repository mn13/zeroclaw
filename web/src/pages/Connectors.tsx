import { useState, useEffect, useCallback } from "react";
import {
  getConnectors,
  updateConnectors,
  getMcpServers,
  updateMcpServers,
  instanceAction,
  getGoogle,
  updateGoogle,
  deleteGoogleAccount,
  getComposio,
  updateComposio,
} from "../api";
import type { ChannelSchema, ChannelField, McpServerConfig, GoogleConfig, GatewayGoogleAccount, ComposioConfig } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

type Tab = "integrations" | "channels" | "mcp";

const labelStyle: React.CSSProperties = {
  fontFamily: "JetBrains Mono, monospace",
  fontSize: 10,
  fontWeight: 600,
  textTransform: "uppercase",
  color: "var(--text-dim)",
  letterSpacing: 1,
};

const btnSecondary: React.CSSProperties = {
  fontFamily: "JetBrains Mono, monospace",
  fontSize: 11,
  fontWeight: 600,
  textTransform: "uppercase",
  letterSpacing: 1,
  padding: "8px 18px",
  background: "transparent",
  border: "1px solid var(--border)",
  color: "var(--text-primary)",
  cursor: "pointer",
  clipPath: clipCorner(6),
};

const btnPrimary: React.CSSProperties = {
  ...btnSecondary,
  borderColor: "var(--amber)",
  color: "var(--amber)",
};

const btnDanger: React.CSSProperties = {
  ...btnSecondary,
  borderColor: "var(--error-text)",
  color: "var(--error-text)",
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "8px 12px",
  background: "var(--bg-input)",
  border: "1px solid var(--border)",
  color: "var(--text-primary)",
  fontFamily: "JetBrains Mono, monospace",
  fontSize: 13,
  clipPath: clipCorner(6),
  outline: "none",
  boxSizing: "border-box" as const,
};

const tabBtnStyle = (active: boolean): React.CSSProperties => ({
  fontFamily: "Syne, sans-serif",
  fontSize: 12,
  fontWeight: 700,
  textTransform: "uppercase",
  letterSpacing: 2,
  padding: "8px 20px",
  background: active ? "var(--amber-glow)" : "transparent",
  border: "none",
  borderBottom: active ? "2px solid var(--amber)" : "2px solid transparent",
  color: active ? "var(--amber)" : "var(--text-dim)",
  cursor: "pointer",
  transition: "all 0.15s",
});

/** Render a single channel config field based on its field_type. */
function FieldRenderer({
  field,
  value,
  onChange,
}: {
  field: ChannelField;
  value: unknown;
  onChange: (val: unknown) => void;
}) {
  // select:opt1,opt2,...
  if (field.field_type.startsWith("select:")) {
    const options = field.field_type.slice(7).split(",");
    const strVal = (value as string) || options[0] || "";
    return (
      <div>
        <div style={{ ...labelStyle, marginBottom: 4 }}>
          {field.label}
          {field.required && <span style={{ color: "var(--amber)" }}> *</span>}
        </div>
        <select
          value={strVal}
          onChange={(e) => onChange(e.target.value)}
          style={{ ...inputStyle, appearance: "auto" as never }}
        >
          {options.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
        <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 11, color: "var(--text-dim)", marginTop: 2 }}>
          {field.help}
        </div>
      </div>
    );
  }

  if (field.field_type === "bool") {
    const boolVal = value === true || value === "true";
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ fontFamily: "Outfit, sans-serif", fontSize: 13, color: "var(--text-dim)", minWidth: 160 }}>
          {field.label}
          {field.required && <span style={{ color: "var(--amber)" }}> *</span>}
        </span>
        <div
          onClick={() => onChange(!boolVal)}
          style={{
            width: 40, height: 22, borderRadius: 11,
            background: boolVal ? "var(--amber)" : "var(--toggle-off)",
            position: "relative", cursor: "pointer", transition: "background 0.2s",
          }}
        >
          <div
            style={{
              width: 16, height: 16, borderRadius: "50%", background: "#fff",
              position: "absolute", top: 3, left: boolVal ? 21 : 3, transition: "left 0.2s",
            }}
          />
        </div>
        <span style={{ fontFamily: "Outfit, sans-serif", fontSize: 11, color: "var(--text-dim)" }}>
          {field.help}
        </span>
      </div>
    );
  }

  if (field.field_type === "string_list") {
    const listVal = Array.isArray(value) ? (value as string[]).join(", ") : (value as string) || "";
    return (
      <div>
        <div style={{ ...labelStyle, marginBottom: 4 }}>
          {field.label}
          {field.required && <span style={{ color: "var(--amber)" }}> *</span>}
        </div>
        <input
          type="text"
          value={listVal}
          onChange={(e) => {
            const arr = e.target.value.split(",").map((s) => s.trim()).filter(Boolean);
            onChange(arr);
          }}
          placeholder={field.help}
          style={inputStyle}
        />
      </div>
    );
  }

  if (field.field_type === "u64") {
    const numVal = typeof value === "number" ? value : parseInt(value as string, 10) || 0;
    return (
      <div>
        <div style={{ ...labelStyle, marginBottom: 4 }}>
          {field.label}
          {field.required && <span style={{ color: "var(--amber)" }}> *</span>}
        </div>
        <input
          type="number"
          value={numVal || ""}
          onChange={(e) => onChange(parseInt(e.target.value, 10) || 0)}
          placeholder={field.help}
          style={{ ...inputStyle, maxWidth: 200 }}
        />
      </div>
    );
  }

  // Default: string input
  return (
    <div>
      <div style={{ ...labelStyle, marginBottom: 4 }}>
        {field.label}
        {field.required && <span style={{ color: "var(--amber)" }}> *</span>}
      </div>
      <input
        type={field.sensitive ? "password" : "text"}
        value={(value as string) || ""}
        onChange={(e) => onChange(e.target.value)}
        placeholder={field.help}
        style={inputStyle}
      />
    </div>
  );
}

// ── Integrations Assignment Tab ──

function IntegrationsTab({ instanceId, toast }: Props) {
  const [googleConfig, setGoogleConfig] = useState<GoogleConfig | null>(null);
  const [googleEnabled, setGoogleEnabled] = useState(false);
  const [googleLoading, setGoogleLoading] = useState(true);
  const [assignLoading, setAssignLoading] = useState(false);

  const [composioConfig, setComposioConfig] = useState<ComposioConfig | null>(null);
  const [composioEnabled, setComposioEnabled] = useState(false);
  const [composioLoading, setComposioLoading] = useState(true);

  const loadGoogle = useCallback(() => {
    setGoogleLoading(true);
    getGoogle(instanceId)
      .then((c) => {
        setGoogleConfig(c);
        setGoogleEnabled(c.enabled);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Google config", true))
      .finally(() => setGoogleLoading(false));
  }, [instanceId, toast]);

  const loadComposio = useCallback(() => {
    setComposioLoading(true);
    getComposio(instanceId)
      .then((c) => {
        setComposioConfig(c);
        setComposioEnabled(c.enabled);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Composio config", true))
      .finally(() => setComposioLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    loadGoogle();
    loadComposio();
  }, [instanceId, loadGoogle, loadComposio]);

  const handleGoogleToggle = useCallback(async () => {
    const next = !googleEnabled;
    setGoogleEnabled(next);
    try {
      await updateGoogle(instanceId, { enabled: next });
      toast(next ? "Google enabled" : "Google disabled");
      loadGoogle();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
      setGoogleEnabled(!next);
    }
  }, [instanceId, googleEnabled, toast, loadGoogle]);

  const handleComposioToggle = useCallback(async () => {
    const next = !composioEnabled;
    setComposioEnabled(next);
    try {
      await updateComposio(instanceId, { enabled: next });
      toast(next ? "Composio enabled" : "Composio disabled");
      loadComposio();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
      setComposioEnabled(!next);
    }
  }, [instanceId, composioEnabled, toast, loadComposio]);

  const handleUnassign = useCallback(async (email: string) => {
    if (!confirm(`Unassign ${email} from this agent?`)) return;
    try {
      await deleteGoogleAccount(instanceId, email);
      toast(`Unassigned ${email}`);
      loadGoogle();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Unassign failed", true);
    }
  }, [instanceId, toast, loadGoogle]);

  const handleAssign = useCallback(async (email: string) => {
    setAssignLoading(true);
    try {
      await updateGoogle(instanceId, { enabled: true, assign_accounts: [email] });
      toast(`Assigned ${email} to this agent`);
      loadGoogle();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Assign failed", true);
    } finally {
      setAssignLoading(false);
    }
  }, [instanceId, toast, loadGoogle]);

  const loading = googleLoading || composioLoading;
  if (loading && !googleConfig && !composioConfig) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading integrations...
      </div>
    );
  }

  const gatewayAccounts: GatewayGoogleAccount[] = googleConfig?.gateway_accounts ?? [];
  const assignedEmails = googleConfig?.accounts ?? [];
  const unassignedGateway = gatewayAccounts.filter((a) => !assignedEmails.includes(a.email));

  return (
    <div style={{ padding: 24, maxWidth: 600, flex: 1, overflowY: "auto" }}>
      {/* Google Workspace Section */}
      <div style={{ marginBottom: 32 }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
          <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
            Google Workspace
          </span>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ ...labelStyle, marginBottom: 0 }}>{googleEnabled ? "Enabled" : "Disabled"}</span>
            <div
              onClick={handleGoogleToggle}
              style={{
                width: 40, height: 22, borderRadius: 11,
                background: googleEnabled ? "var(--amber)" : "var(--toggle-off)",
                position: "relative", cursor: "pointer", transition: "background 0.2s",
              }}
            >
              <div
                style={{
                  width: 16, height: 16, borderRadius: "50%", background: "#fff",
                  position: "absolute", top: 3, left: googleEnabled ? 21 : 3, transition: "left 0.2s",
                }}
              />
            </div>
          </div>
        </div>

        {/* Assigned to this Agent */}
        <div>
          <div style={{ ...labelStyle, marginBottom: 10 }}>Assigned to this Agent</div>
          {assignedEmails.length > 0 ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 16 }}>
              {assignedEmails.map((email) => (
                <div
                  key={email}
                  style={{
                    display: "flex", alignItems: "center", justifyContent: "space-between",
                    padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
                    clipPath: clipCorner(6),
                  }}
                >
                  <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, color: "var(--text-primary)" }}>
                    {email}
                  </span>
                  <button onClick={() => handleUnassign(email)} style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}>
                    Unassign
                  </button>
                </div>
              ))}
            </div>
          ) : (
            <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13, marginBottom: 16 }}>
              No accounts assigned to this agent.
            </div>
          )}
        </div>

        {/* Available Gateway Accounts (unassigned) */}
        {unassignedGateway.length > 0 && (
          <div style={{ marginTop: 8 }}>
            <div style={{ ...labelStyle, marginBottom: 10 }}>Available Gateway Accounts</div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {unassignedGateway.map((account) => (
                <div
                  key={account.email}
                  style={{
                    display: "flex", alignItems: "center", justifyContent: "space-between",
                    padding: "8px 12px", background: "var(--bg-input)", border: "1px dashed var(--border)",
                    clipPath: clipCorner(6),
                  }}
                >
                  <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, color: "var(--text-dim)" }}>
                    {account.email}
                  </span>
                  <button
                    onClick={() => handleAssign(account.email)}
                    disabled={assignLoading}
                    style={{
                      ...btnPrimary,
                      padding: "4px 10px",
                      fontSize: 10,
                      opacity: assignLoading ? 0.4 : 1,
                      cursor: assignLoading ? "default" : "pointer",
                    }}
                  >
                    {assignLoading ? "..." : "Assign"}
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}

        {gatewayAccounts.length === 0 && assignedEmails.length === 0 && (
          <div style={{
            padding: 12, background: "var(--bg-input)",
            border: "1px solid var(--border)", clipPath: clipCorner(6),
            color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 12,
          }}>
            No gateway accounts available. Connect accounts on the gateway INTEGRATIONS page first.
          </div>
        )}
      </div>

      {/* Composio Section */}
      <div>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
          <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
            Composio
          </span>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ ...labelStyle, marginBottom: 0 }}>{composioEnabled ? "Enabled" : "Disabled"}</span>
            <div
              onClick={handleComposioToggle}
              style={{
                width: 40, height: 22, borderRadius: 11,
                background: composioEnabled ? "var(--amber)" : "var(--toggle-off)",
                position: "relative", cursor: "pointer", transition: "background 0.2s",
              }}
            >
              <div
                style={{
                  width: 16, height: 16, borderRadius: "50%", background: "#fff",
                  position: "absolute", top: 3, left: composioEnabled ? 21 : 3, transition: "left 0.2s",
                }}
              />
            </div>
          </div>
        </div>

        {composioConfig && (
          <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13 }}>
            {composioConfig.has_api_key ? (
              <span>API key configured. Entity ID: <code style={{ fontFamily: "JetBrains Mono, monospace", color: "var(--text-primary)" }}>{composioConfig.entity_id || "default"}</code></span>
            ) : (
              <span>API key not configured. Set it up on the gateway INTEGRATIONS page.</span>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Channels Tab ──

function ChannelsTab({ instanceId, toast }: Props) {
  const [schema, setSchema] = useState<ChannelSchema[]>([]);
  const [channelsConfig, setChannelsConfig] = useState<Record<string, Record<string, unknown>>>({});
  const [selectedChannel, setSelectedChannel] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [dirty, setDirty] = useState(false);
  const [needsRestart, setNeedsRestart] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    getConnectors(instanceId)
      .then((resp) => {
        setSchema(resp.channel_schema);
        setChannelsConfig((resp.channels_config || {}) as Record<string, Record<string, unknown>>);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load connectors", true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    setSelectedChannel(null);
    setDirty(false);
    setNeedsRestart(false);
    load();
  }, [instanceId, load]);

  const handleSave = useCallback(async () => {
    try {
      await updateConnectors(instanceId, channelsConfig);
      toast("Connectors saved");
      setDirty(false);
      setNeedsRestart(true);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
    }
  }, [instanceId, channelsConfig, toast]);

  const handleRestart = useCallback(async () => {
    try {
      await instanceAction(instanceId, "restart");
      toast("Container restarting...");
      setNeedsRestart(false);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Restart failed", true);
    }
  }, [instanceId, toast]);

  const updateField = useCallback(
    (channelType: string, fieldName: string, value: unknown) => {
      setChannelsConfig((prev) => ({
        ...prev,
        [channelType]: {
          ...prev[channelType],
          [fieldName]: value,
        },
      }));
      setDirty(true);
    },
    [],
  );

  const toggleChannel = useCallback(
    (channelType: string, enabled: boolean) => {
      setChannelsConfig((prev) => ({
        ...prev,
        [channelType]: {
          ...(prev[channelType] || {}),
          enabled,
        },
      }));
      setDirty(true);
    },
    [],
  );

  if (loading && schema.length === 0) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading connectors...
      </div>
    );
  }

  const selectedSchema = schema.find((s) => s.channel_type === selectedChannel);
  const selectedConfig = selectedChannel ? channelsConfig[selectedChannel] || {} : {};
  const isEnabled = !!(selectedConfig as Record<string, unknown>)?.enabled;

  return (
    <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
      {/* Channel list sidebar */}
      <div
        style={{
          width: 240,
          minWidth: 240,
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <div style={{ padding: "14px 16px 10px", ...labelStyle }}>Channels</div>
        <div style={{ flex: 1, overflowY: "auto" }}>
          {schema.map((ch) => {
            const active = selectedChannel === ch.channel_type;
            const configured = !!(channelsConfig[ch.channel_type]?.enabled);
            return (
              <button
                key={ch.channel_type}
                onClick={() => setSelectedChannel(ch.channel_type)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  width: "100%",
                  padding: "10px 16px",
                  background: active ? "var(--amber-glow)" : "transparent",
                  border: "none",
                  borderLeft: `2px solid ${active ? "var(--amber)" : "transparent"}`,
                  cursor: "pointer",
                  transition: "background 0.1s",
                  textAlign: "left",
                }}
                onMouseEnter={(e) => {
                  if (!active) e.currentTarget.style.background = "var(--row-hover-bg)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = active ? "var(--amber-glow)" : "transparent";
                }}
              >
                <span
                  style={{
                    width: 8,
                    height: 8,
                    minWidth: 8,
                    borderRadius: "50%",
                    background: configured ? "#22c55e" : "var(--toggle-off)",
                    boxShadow: configured ? "0 0 4px rgba(34,197,94,0.5)" : "none",
                  }}
                />
                <span
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 12,
                    fontWeight: active ? 600 : 400,
                    color: active ? "var(--amber)" : configured ? "var(--text-primary)" : "var(--text-dim)",
                  }}
                >
                  {ch.label}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Config form */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* Restart banner */}
        {needsRestart && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "10px 20px",
              background: "var(--amber-glow)",
              borderBottom: "1px solid var(--border-amber)",
            }}
          >
            <span style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--amber)" }}>
              Config saved. Restart container to apply.
            </span>
            <button onClick={handleRestart} style={{ ...btnPrimary, padding: "5px 12px", fontSize: 10 }}>
              Restart Now
            </button>
          </div>
        )}

        {selectedSchema ? (
          <div style={{ flex: 1, overflowY: "auto", padding: "20px" }}>
            {/* Header + enable toggle */}
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 20 }}>
              <span
                style={{
                  fontFamily: "Syne, sans-serif",
                  fontSize: 16,
                  fontWeight: 700,
                  color: "var(--amber)",
                }}
              >
                {selectedSchema.label}
              </span>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ ...labelStyle, marginBottom: 0 }}>{isEnabled ? "Enabled" : "Disabled"}</span>
                <div
                  onClick={() => toggleChannel(selectedChannel!, !isEnabled)}
                  style={{
                    width: 40,
                    height: 22,
                    borderRadius: 11,
                    background: isEnabled ? "var(--amber)" : "var(--toggle-off)",
                    position: "relative",
                    cursor: "pointer",
                    transition: "background 0.2s",
                  }}
                >
                  <div
                    style={{
                      width: 16,
                      height: 16,
                      borderRadius: "50%",
                      background: "#fff",
                      position: "absolute",
                      top: 3,
                      left: isEnabled ? 21 : 3,
                      transition: "left 0.2s",
                    }}
                  />
                </div>
              </div>
            </div>

            {/* Fields */}
            {isEnabled && (
              <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                {selectedSchema.fields.map((field) => {
                  const val = (selectedConfig as Record<string, unknown>)[field.name];
                  return (
                    <FieldRenderer
                      key={field.name}
                      field={field}
                      value={val}
                      onChange={(v) => updateField(selectedChannel!, field.name, v)}
                    />
                  );
                })}
              </div>
            )}

            {/* Save button */}
            <div style={{ marginTop: 24 }}>
              <button
                onClick={handleSave}
                disabled={!dirty}
                style={{
                  ...btnPrimary,
                  opacity: dirty ? 1 : 0.4,
                  cursor: dirty ? "pointer" : "default",
                }}
              >
                Save
              </button>
              {dirty && (
                <span style={{ ...labelStyle, marginLeft: 12, color: "var(--amber)" }}>UNSAVED</span>
              )}
            </div>
          </div>
        ) : (
          <div
            style={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              color: "var(--text-dim)",
              fontFamily: "Outfit, sans-serif",
              gap: 8,
            }}
          >
            <span style={{ fontSize: 28, opacity: 0.4 }}>{"\u2261"}</span>
            <span style={{ fontSize: 13, letterSpacing: 1 }}>Select a channel to configure</span>
            <span style={{ fontSize: 11, maxWidth: 400, textAlign: "center", lineHeight: 1.5 }}>
              Enable channels to connect this agent to Telegram, Discord, Slack, and other messaging platforms.
              Channel changes require a container restart.
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

// ── MCP Servers Tab ──

const emptyServer: McpServerConfig = {
  name: "",
  transport: "sse",
  url: "",
  command: "",
  args: [],
  env: {},
  enabled: true,
};

function McpServersTab({ instanceId, toast }: Props) {
  const [servers, setServers] = useState<McpServerConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [dirty, setDirty] = useState(false);
  const [needsRestart, setNeedsRestart] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);
  const [editForm, setEditForm] = useState<McpServerConfig>({ ...emptyServer });
  const [showAdd, setShowAdd] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    getMcpServers(instanceId)
      .then((resp) => setServers(resp.mcp_servers || []))
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load MCP servers", true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    setDirty(false);
    setNeedsRestart(false);
    setEditing(null);
    setShowAdd(false);
    load();
  }, [instanceId, load]);

  const handleSave = useCallback(async () => {
    try {
      await updateMcpServers(instanceId, servers);
      toast("MCP servers saved");
      setDirty(false);
      setNeedsRestart(true);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
    }
  }, [instanceId, servers, toast]);

  const handleRestart = useCallback(async () => {
    try {
      await instanceAction(instanceId, "restart");
      toast("Container restarting...");
      setNeedsRestart(false);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Restart failed", true);
    }
  }, [instanceId, toast]);

  const addOrUpdateServer = useCallback(() => {
    if (!editForm.name.trim()) {
      toast("Name is required", true);
      return;
    }
    if (editing !== null) {
      setServers((prev) => prev.map((s, i) => (i === editing ? { ...editForm } : s)));
    } else {
      setServers((prev) => [...prev, { ...editForm }]);
    }
    setDirty(true);
    setEditing(null);
    setShowAdd(false);
    setEditForm({ ...emptyServer });
  }, [editForm, editing, toast]);

  const removeServer = useCallback((idx: number) => {
    setServers((prev) => prev.filter((_, i) => i !== idx));
    setDirty(true);
  }, []);

  const toggleServer = useCallback((idx: number) => {
    setServers((prev) =>
      prev.map((s, i) => (i === idx ? { ...s, enabled: !s.enabled } : s)),
    );
    setDirty(true);
  }, []);

  if (loading) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading MCP servers...
      </div>
    );
  }

  const renderForm = () => (
    <div
      style={{
        background: "var(--bg-card)",
        border: "1px solid var(--border)",
        clipPath: clipCorner(10),
        padding: "18px 20px",
        marginBottom: 16,
      }}
    >
      <div style={{ display: "flex", gap: 12, marginBottom: 12, flexWrap: "wrap" }}>
        <div style={{ flex: 1, minWidth: 160 }}>
          <div style={{ ...labelStyle, marginBottom: 4 }}>Name</div>
          <input
            value={editForm.name}
            onChange={(e) => setEditForm((f) => ({ ...f, name: e.target.value }))}
            placeholder="my-mcp-server"
            style={inputStyle}
          />
        </div>
        <div style={{ minWidth: 140 }}>
          <div style={{ ...labelStyle, marginBottom: 4 }}>Transport</div>
          <select
            value={editForm.transport}
            onChange={(e) => setEditForm((f) => ({ ...f, transport: e.target.value as "sse" | "stdio" }))}
            style={{ ...inputStyle, appearance: "auto" as never }}
          >
            <option value="sse">SSE</option>
            <option value="stdio">stdio</option>
          </select>
        </div>
      </div>

      {editForm.transport === "sse" ? (
        <div style={{ marginBottom: 12 }}>
          <div style={{ ...labelStyle, marginBottom: 4 }}>URL</div>
          <input
            value={editForm.url || ""}
            onChange={(e) => setEditForm((f) => ({ ...f, url: e.target.value }))}
            placeholder="http://localhost:3000/sse"
            style={inputStyle}
          />
        </div>
      ) : (
        <>
          <div style={{ marginBottom: 12 }}>
            <div style={{ ...labelStyle, marginBottom: 4 }}>Command</div>
            <input
              value={editForm.command || ""}
              onChange={(e) => setEditForm((f) => ({ ...f, command: e.target.value }))}
              placeholder="npx"
              style={inputStyle}
            />
          </div>
          <div style={{ marginBottom: 12 }}>
            <div style={{ ...labelStyle, marginBottom: 4 }}>Args (comma-separated)</div>
            <input
              value={(editForm.args || []).join(", ")}
              onChange={(e) =>
                setEditForm((f) => ({
                  ...f,
                  args: e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean),
                }))
              }
              placeholder="-y, @modelcontextprotocol/server-name"
              style={inputStyle}
            />
          </div>
        </>
      )}

      <div style={{ marginBottom: 12 }}>
        <div style={{ ...labelStyle, marginBottom: 4 }}>Env Vars (KEY=VALUE, one per line)</div>
        <textarea
          value={Object.entries(editForm.env || {})
            .map(([k, v]) => `${k}=${v}`)
            .join("\n")}
          onChange={(e) => {
            const env: Record<string, string> = {};
            e.target.value.split("\n").forEach((line) => {
              const idx = line.indexOf("=");
              if (idx > 0) {
                env[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
              }
            });
            setEditForm((f) => ({ ...f, env }));
          }}
          rows={3}
          placeholder="API_KEY=sk-..."
          style={{
            ...inputStyle,
            resize: "vertical",
            lineHeight: 1.5,
          }}
        />
      </div>

      <div style={{ display: "flex", gap: 10 }}>
        <button onClick={addOrUpdateServer} style={btnPrimary}>
          {editing !== null ? "Update" : "Add"}
        </button>
        <button
          onClick={() => {
            setShowAdd(false);
            setEditing(null);
            setEditForm({ ...emptyServer });
          }}
          style={btnSecondary}
        >
          Cancel
        </button>
      </div>
    </div>
  );

  return (
    <div style={{ padding: 24, maxWidth: 900, flex: 1, overflowY: "auto" }}>
      {needsRestart && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "10px 16px",
            background: "var(--amber-glow)",
            border: "1px solid var(--border-amber)",
            clipPath: clipCorner(8),
            marginBottom: 16,
          }}
        >
          <span style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--amber)" }}>
            Config saved. Restart container to apply.
          </span>
          <button onClick={handleRestart} style={{ ...btnPrimary, padding: "5px 12px", fontSize: 10 }}>
            Restart Now
          </button>
        </div>
      )}

      {(showAdd || editing !== null) && renderForm()}

      {/* Server list */}
      {servers.length > 0 && (
        <div
          style={{
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            overflow: "hidden",
            marginBottom: 16,
          }}
        >
          {servers.map((srv, idx) => (
            <div
              key={idx}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "12px 16px",
                borderBottom: idx < servers.length - 1 ? "1px solid var(--border)" : "none",
              }}
            >
              <div
                onClick={() => toggleServer(idx)}
                style={{
                  width: 40,
                  height: 22,
                  borderRadius: 11,
                  background: srv.enabled ? "var(--amber)" : "var(--toggle-off)",
                  position: "relative",
                  cursor: "pointer",
                  transition: "background 0.2s",
                  flexShrink: 0,
                }}
              >
                <div
                  style={{
                    width: 16,
                    height: 16,
                    borderRadius: "50%",
                    background: "#fff",
                    position: "absolute",
                    top: 3,
                    left: srv.enabled ? 21 : 3,
                    transition: "left 0.2s",
                  }}
                />
              </div>

              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                  {srv.name}
                </div>
                <div style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 11, color: "var(--text-dim)" }}>
                  {srv.transport.toUpperCase()}
                  {srv.transport === "sse" && srv.url ? ` — ${srv.url}` : ""}
                  {srv.transport === "stdio" && srv.command ? ` — ${srv.command} ${(srv.args || []).join(" ")}` : ""}
                </div>
              </div>

              <button
                onClick={() => {
                  setEditForm({ ...srv });
                  setEditing(idx);
                  setShowAdd(true);
                }}
                style={{ ...btnSecondary, padding: "4px 10px", fontSize: 10 }}
              >
                Edit
              </button>
              <button
                onClick={() => removeServer(idx)}
                style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}
              >
                Remove
              </button>
            </div>
          ))}
        </div>
      )}

      {servers.length === 0 && !showAdd && (
        <div
          style={{
            padding: 32,
            textAlign: "center",
            color: "var(--text-dim)",
            fontFamily: "Outfit, sans-serif",
            fontSize: 13,
          }}
        >
          No MCP servers configured. Add one to extend the agent's tool capabilities.
        </div>
      )}

      <div style={{ display: "flex", gap: 10 }}>
        {!showAdd && editing === null && (
          <button onClick={() => setShowAdd(true)} style={btnPrimary}>
            + Add MCP Server
          </button>
        )}
        <button
          onClick={handleSave}
          disabled={!dirty}
          style={{
            ...btnPrimary,
            opacity: dirty ? 1 : 0.4,
            cursor: dirty ? "pointer" : "default",
          }}
        >
          Save
        </button>
        {dirty && (
          <span style={{ ...labelStyle, color: "var(--amber)", alignSelf: "center" }}>UNSAVED</span>
        )}
      </div>
    </div>
  );
}

// ── Main Connectors Page ──

export default function Connectors({ instanceId, toast }: Props) {
  const [tab, setTab] = useState<Tab>("integrations");

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, overflow: "hidden" }}>
      {/* Tab bar */}
      <div
        style={{
          display: "flex",
          gap: 0,
          borderBottom: "1px solid var(--border)",
          background: "var(--bg-card)",
          minHeight: 40,
        }}
      >
        <button style={tabBtnStyle(tab === "integrations")} onClick={() => setTab("integrations")}>
          Integrations
        </button>
        <button style={tabBtnStyle(tab === "channels")} onClick={() => setTab("channels")}>
          Channels
        </button>
        <button style={tabBtnStyle(tab === "mcp")} onClick={() => setTab("mcp")}>
          MCP Servers
        </button>
      </div>

      {/* Use key={instanceId} to force full remount on agent switch */}
      {tab === "integrations" && <IntegrationsTab key={instanceId} instanceId={instanceId} toast={toast} />}
      {tab === "channels" && <ChannelsTab key={instanceId} instanceId={instanceId} toast={toast} />}
      {tab === "mcp" && <McpServersTab key={instanceId} instanceId={instanceId} toast={toast} />}
    </div>
  );
}
