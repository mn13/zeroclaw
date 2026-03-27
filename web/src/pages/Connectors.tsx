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
  updateComposio,
  getComposioInstance,
  composioInstanceConnect,
  unassignComposioConnection,
  assignComposioConnection,
  listComposioConnections,
  syncComposioConnections,
  composioMcpSync,
  getSignal,
  assignSignal,
  unassignSignal,
} from "../api";
import type { ChannelSchema, ChannelField, McpServerConfig, GoogleConfig, GatewayGoogleAccount, ComposioInstanceConfig, ComposioConnectionInfo, SignalConfig, SignalConnection } from "../api";
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

  const [composioConfig, setComposioConfig] = useState<ComposioInstanceConfig | null>(null);
  const [composioEnabled, setComposioEnabled] = useState(false);
  const [composioLoading, setComposioLoading] = useState(true);
  const [composioConnectApp, setComposioConnectApp] = useState("");
  const [composioConnectLoading, setComposioConnectLoading] = useState(false);
  const [composioSyncLoading, setComposioSyncLoading] = useState(false);
  const [composioGatewayConnections, setComposioGatewayConnections] = useState<ComposioConnectionInfo[]>([]);
  const [composioAssignLoading, setComposioAssignLoading] = useState(false);

  const [signalConfig, setSignalConfig] = useState<SignalConfig | null>(null);
  const [signalLoading, setSignalLoading] = useState(true);
  const [signalAssignLoading, setSignalAssignLoading] = useState(false);

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
    Promise.all([getComposioInstance(instanceId), listComposioConnections()])
      .then(async ([c, gw]) => {
        setComposioConfig(c);
        setComposioEnabled(c.enabled);
        // If the local store has no connections but the gateway API key is
        // configured, auto-sync from Composio to pick up any connections
        // created via OAuth whose callbacks may not have persisted locally.
        if (gw.connections.length === 0 && c.has_gateway_api_key) {
          try {
            const synced = await syncComposioConnections();
            if (synced.connections && synced.connections.length > 0) {
              setComposioGatewayConnections(synced.connections);
              return;
            }
          } catch { /* ignore sync failures */ }
        }
        setComposioGatewayConnections(gw.connections);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Composio config", true))
      .finally(() => setComposioLoading(false));
  }, [instanceId, toast]);

  const loadSignal = useCallback(() => {
    setSignalLoading(true);
    getSignal(instanceId)
      .then((c) => setSignalConfig(c))
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Signal config", true))
      .finally(() => setSignalLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    loadGoogle();
    loadComposio();
    loadSignal();
  }, [instanceId, loadGoogle, loadComposio, loadSignal]);

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

  const handleComposioSyncGateway = useCallback(async () => {
    try {
      await updateComposio(instanceId, { enabled: true, sync_gateway: true } as any);
      toast("Synced gateway credentials to instance");
      loadComposio();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Sync failed", true);
    }
  }, [instanceId, toast, loadComposio]);

  const handleComposioConnect = useCallback(async () => {
    if (!composioConnectApp) return;
    setComposioConnectLoading(true);
    try {
      const resp = await composioInstanceConnect(instanceId, composioConnectApp);
      if (resp.redirect_url) {
        window.open(resp.redirect_url, "_blank");
        toast("OAuth window opened. Complete authorization and refresh.");
      }
      setComposioConnectApp("");
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Connect failed", true);
    } finally {
      setComposioConnectLoading(false);
    }
  }, [instanceId, composioConnectApp, toast]);

  const handleComposioMcpSync = useCallback(async () => {
    setComposioSyncLoading(true);
    try {
      const resp = await composioMcpSync(instanceId);
      toast(`MCP servers synced for: ${resp.synced_toolkits.join(", ")}`);
      if (resp.gap_warning) {
        toast(resp.gap_warning, true);
      }
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "MCP sync failed", true);
    } finally {
      setComposioSyncLoading(false);
    }
  }, [instanceId, toast]);

  const handleComposioUnassign = useCallback(async (connectionId: string) => {
    if (!confirm("Unassign this Composio connection from this agent?")) return;
    try {
      await unassignComposioConnection(instanceId, connectionId);
      toast("Connection unassigned");
      loadComposio();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Unassign failed", true);
    }
  }, [instanceId, toast, loadComposio]);

  const handleComposioAssign = useCallback(async (connectionId: string) => {
    setComposioAssignLoading(true);
    try {
      await assignComposioConnection(instanceId, connectionId);
      toast("Connection assigned");
      loadComposio();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Assign failed", true);
    } finally {
      setComposioAssignLoading(false);
    }
  }, [instanceId, toast, loadComposio]);

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

  // Signal assign config form state
  const [signalAssignTarget, setSignalAssignTarget] = useState<string | null>(null);
  const [signalAllowedFrom, setSignalAllowedFrom] = useState("*");
  const [signalGroupId, setSignalGroupId] = useState("");

  const handleSignalAssign = useCallback(async (connectionName: string) => {
    const allowedFrom = signalAllowedFrom.trim()
      ? signalAllowedFrom.split(",").map((s) => s.trim()).filter(Boolean)
      : ["*"];
    setSignalAssignLoading(true);
    try {
      await assignSignal(instanceId, {
        connection: connectionName,
        allowed_from: allowedFrom,
        group_id: signalGroupId.trim() || undefined,
      });
      toast(`Assigned Signal connection "${connectionName}" to this agent`);
      setSignalAssignTarget(null);
      setSignalAllowedFrom("*");
      setSignalGroupId("");
      loadSignal();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Assign failed", true);
    } finally {
      setSignalAssignLoading(false);
    }
  }, [instanceId, signalAllowedFrom, signalGroupId, toast, loadSignal]);

  const handleSignalUnassign = useCallback(async (connectionName: string) => {
    if (!confirm(`Unassign Signal connection "${connectionName}" from this agent?`)) return;
    try {
      await unassignSignal(instanceId, connectionName);
      toast(`Unassigned Signal connection "${connectionName}"`);
      loadSignal();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Unassign failed", true);
    }
  }, [instanceId, toast, loadSignal]);

  const loading = googleLoading || composioLoading || signalLoading;
  if (loading && !googleConfig && !composioConfig && !signalConfig) {
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
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            {/* Status info */}
            <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13 }}>
              {composioConfig.has_api_key ? (
                <span>API key configured. Entity ID: <code style={{ fontFamily: "JetBrains Mono, monospace", color: "var(--text-primary)" }}>{composioConfig.entity_id || "default"}</code></span>
              ) : (
                <span>Instance API key not configured.</span>
              )}
              {composioConfig.has_gateway_api_key && (
                <span style={{ marginLeft: 8, fontSize: 11, color: "#22c55e" }}>(Gateway key available)</span>
              )}
            </div>

            {/* Sync from Gateway button */}
            {composioConfig.has_gateway_api_key && (
              <button onClick={handleComposioSyncGateway} style={{ ...btnPrimary, alignSelf: "flex-start" }}>
                Sync from Gateway
              </button>
            )}

            {/* Connections for this instance */}
            {composioConfig.connections && composioConfig.connections.length > 0 && (
              <div>
                <div style={{ ...labelStyle, marginBottom: 8 }}>Assigned Connections</div>
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  {composioConfig.connections.map((conn: ComposioConnectionInfo) => (
                    <div
                      key={conn.id}
                      style={{
                        display: "flex", alignItems: "center", justifyContent: "space-between",
                        padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
                        clipPath: clipCorner(6),
                      }}
                    >
                      <div>
                        <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                          {conn.name || conn.display_name}
                        </span>
                        <span style={{ marginLeft: 8, fontSize: 11, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                          {conn.toolkit_slug}
                        </span>
                        <span style={{
                          marginLeft: 8, fontSize: 9, fontWeight: 600, padding: "1px 6px", borderRadius: 3,
                          background: conn.status === "ACTIVE" ? "rgba(34,197,94,0.15)" : "rgba(239,68,68,0.15)",
                          color: conn.status === "ACTIVE" ? "#22c55e" : "#ef4444",
                          fontFamily: "JetBrains Mono, monospace",
                        }}>
                          {conn.status}
                        </span>
                      </div>
                      <button onClick={() => handleComposioUnassign(conn.id)} style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}>
                        Unassign
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Available Gateway Connections */}
            {composioConfig.has_gateway_api_key && (() => {
              const assignedIds = new Set((composioConfig.connections ?? []).map((c: ComposioConnectionInfo) => c.id));
              const unassigned = composioGatewayConnections.filter((c) => !assignedIds.has(c.id));
              if (unassigned.length === 0) return null;
              return (
                <div>
                  <div style={{ ...labelStyle, marginBottom: 8 }}>Available Gateway Connections</div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                    {unassigned.map((conn) => (
                      <div
                        key={conn.id}
                        style={{
                          display: "flex", alignItems: "center", justifyContent: "space-between",
                          padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
                          clipPath: clipCorner(6),
                        }}
                      >
                        <div>
                          <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                            {conn.name || conn.display_name}
                          </span>
                          <span style={{ marginLeft: 8, fontSize: 11, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                            {conn.toolkit_slug}
                          </span>
                          <span style={{
                            marginLeft: 8, fontSize: 9, fontWeight: 600, padding: "1px 6px", borderRadius: 3,
                            background: conn.status === "ACTIVE" ? "rgba(34,197,94,0.15)" : "rgba(239,68,68,0.15)",
                            color: conn.status === "ACTIVE" ? "#22c55e" : "#ef4444",
                            fontFamily: "JetBrains Mono, monospace",
                          }}>
                            {conn.status}
                          </span>
                        </div>
                        <button
                          onClick={() => handleComposioAssign(conn.id)}
                          disabled={composioAssignLoading}
                          style={{
                            ...btnPrimary, padding: "4px 10px", fontSize: 10,
                            opacity: composioAssignLoading ? 0.4 : 1,
                            cursor: composioAssignLoading ? "default" : "pointer",
                          }}
                        >
                          Assign
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              );
            })()}

            {/* Connect App */}
            {composioConfig.has_gateway_api_key && (
              <div style={{ padding: 12, border: "1px solid var(--border)", clipPath: clipCorner(6) }}>
                <div style={{ ...labelStyle, marginBottom: 6 }}>Connect New App</div>
                <div style={{ display: "flex", gap: 8 }}>
                  <input
                    value={composioConnectApp}
                    onChange={(e) => setComposioConnectApp(e.target.value)}
                    placeholder="App name (e.g. gmail, slack)"
                    style={{ ...inputStyle, flex: 1 }}
                  />
                  <button
                    onClick={handleComposioConnect}
                    disabled={!composioConnectApp || composioConnectLoading}
                    style={{
                      ...btnPrimary,
                      opacity: composioConnectApp && !composioConnectLoading ? 1 : 0.4,
                      cursor: composioConnectApp && !composioConnectLoading ? "pointer" : "default",
                    }}
                  >
                    {composioConnectLoading ? "..." : "Connect"}
                  </button>
                </div>
              </div>
            )}

            {/* MCP Sync */}
            {composioConfig.connections && composioConfig.connections.length > 0 && composioConfig.has_gateway_api_key && (
              <div>
                <button
                  onClick={handleComposioMcpSync}
                  disabled={composioSyncLoading}
                  style={{
                    ...btnSecondary,
                    opacity: composioSyncLoading ? 0.4 : 1,
                    cursor: composioSyncLoading ? "default" : "pointer",
                  }}
                >
                  {composioSyncLoading ? "Syncing..." : "Sync MCP Servers"}
                </button>
                <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 11, color: "var(--text-dim)", marginTop: 4 }}>
                  Creates Composio-hosted MCP server entries in this instance's config. Note: MCP client runtime is not yet implemented.
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Signal Section */}
      <div style={{ marginTop: 32 }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
          <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
            Signal
          </span>
          {signalConfig?.enabled && (
            <span style={{ ...labelStyle, marginBottom: 0, color: "#22c55e" }}>Connected</span>
          )}
        </div>

        {/* Currently assigned connection */}
        {signalConfig?.enabled && signalConfig.connection_name ? (
          <div style={{ marginBottom: 16 }}>
            <div style={{ ...labelStyle, marginBottom: 10 }}>Assigned to this Agent</div>
            <div
              style={{
                display: "flex", alignItems: "center", justifyContent: "space-between",
                padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
                clipPath: clipCorner(6),
              }}
            >
              <div>
                <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, color: "var(--text-primary)" }}>
                  {signalConfig.connection_name}
                </span>
                <span style={{ marginLeft: 8, fontSize: 11, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                  {signalConfig.account}
                </span>
              </div>
              <button
                onClick={() => handleSignalUnassign(signalConfig.connection_name!)}
                style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}
              >
                Unassign
              </button>
            </div>
          </div>
        ) : (
          <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13, marginBottom: 16 }}>
            No Signal connection assigned to this agent.
          </div>
        )}

        {/* Available gateway connections (not assigned to this agent) */}
        {(() => {
          const gwConns: SignalConnection[] = signalConfig?.gateway_connections ?? [];
          const currentName = signalConfig?.connection_name;
          const available = gwConns.filter((c) => c.name !== currentName);

          if (available.length === 0 && !signalConfig?.enabled) {
            return (
              <div style={{
                padding: 12, background: "var(--bg-input)",
                border: "1px solid var(--border)", clipPath: clipCorner(6),
                color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 12,
              }}>
                No gateway connections available. Link Signal accounts on the gateway INTEGRATIONS page first.
              </div>
            );
          }

          if (available.length === 0) return null;

          return (
            <div>
              <div style={{ ...labelStyle, marginBottom: 10 }}>Available Gateway Connections</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {available.map((conn) => (
                  <div key={conn.name}>
                    <div
                      style={{
                        display: "flex", alignItems: "center", justifyContent: "space-between",
                        padding: "8px 12px", background: "var(--bg-input)", border: "1px dashed var(--border)",
                        clipPath: clipCorner(6),
                      }}
                    >
                      <div>
                        <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, color: "var(--text-dim)" }}>
                          {conn.name}
                        </span>
                        <span style={{ marginLeft: 8, fontSize: 11, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                          {conn.account}
                        </span>
                      </div>
                      <button
                        onClick={() => {
                          if (signalAssignTarget === conn.name) {
                            setSignalAssignTarget(null);
                          } else {
                            setSignalAssignTarget(conn.name);
                            setSignalAllowedFrom("*");
                            setSignalGroupId("");
                          }
                        }}
                        style={{
                          ...btnPrimary,
                          padding: "4px 10px",
                          fontSize: 10,
                        }}
                      >
                        {signalAssignTarget === conn.name ? "Cancel" : "Assign"}
                      </button>
                    </div>

                    {signalAssignTarget === conn.name && (
                      <div style={{
                        marginTop: 6, padding: 12, background: "var(--bg-card)",
                        border: "1px solid var(--border)", clipPath: clipCorner(6),
                        display: "flex", flexDirection: "column", gap: 10,
                      }}>
                        <div>
                          <div style={{ ...labelStyle, marginBottom: 4 }}>Allowed Senders</div>
                          <input
                            value={signalAllowedFrom}
                            onChange={(e) => setSignalAllowedFrom(e.target.value)}
                            placeholder="* (all) or +1234567890, +0987654321"
                            style={inputStyle}
                          />
                          <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 11, color: "var(--text-dim)", marginTop: 3 }}>
                            Comma-separated E.164 phone numbers. Use * to allow all senders.
                          </div>
                        </div>
                        <div>
                          <div style={{ ...labelStyle, marginBottom: 4 }}>Group / DM Filter</div>
                          <input
                            value={signalGroupId}
                            onChange={(e) => setSignalGroupId(e.target.value)}
                            placeholder='Leave empty for all, or "dm" for DMs only'
                            style={inputStyle}
                          />
                          <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 11, color: "var(--text-dim)", marginTop: 3 }}>
                            Empty = all messages. "dm" = direct messages only. Or enter a specific group ID.
                          </div>
                        </div>
                        <button
                          onClick={() => handleSignalAssign(conn.name)}
                          disabled={signalAssignLoading}
                          style={{
                            ...btnPrimary,
                            alignSelf: "flex-start",
                            opacity: signalAssignLoading ? 0.4 : 1,
                            cursor: signalAssignLoading ? "default" : "pointer",
                          }}
                        >
                          {signalAssignLoading ? "Assigning..." : "Confirm Assign"}
                        </button>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          );
        })()}
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
