import { useState, useEffect, useCallback } from "react";
import {
  listGatewayGoogleAccounts,
  gatewayGoogleAuthInit,
  gatewayGoogleAuthComplete,
  deleteGatewayGoogleAccount,
  listSignalConnections,
  signalLinkStart,
  signalLinkFinish,
  deleteSignalConnection,
  getComposioGatewayConfig,
  listComposioConnections,
  listComposioApps,
  composioConnectInit,
  deleteComposioConnectionGlobal,
  syncComposioConnections,
} from "../api";
import type { GatewayGoogleAccount, SignalConnection, ComposioGatewayConfig, ComposioConnectionInfo, ComposioApp } from "../api";
import { clipCorner } from "../theme";
import { QRCodeSVG } from "qrcode.react";

interface Props {
  toast: (msg: string, isError?: boolean) => void;
}

type Tab = "google" | "signal" | "composio";

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

// ── Google Gateway Tab ──

function GoogleGatewayTab({ toast }: Props) {
  const [accounts, setAccounts] = useState<GatewayGoogleAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [authEmail, setAuthEmail] = useState("");
  const [authLoading, setAuthLoading] = useState(false);
  const [authUrl, setAuthUrl] = useState("");
  const [callbackUrl, setCallbackUrl] = useState("");

  const load = useCallback(() => {
    setLoading(true);
    listGatewayGoogleAccounts()
      .then((resp) => setAccounts(resp.accounts))
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load gateway accounts", true))
      .finally(() => setLoading(false));
  }, [toast]);

  useEffect(() => {
    load();
  }, [load]);

  // Check URL params for OAuth callback result on mount
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const googleResult = params.get("google");
    if (googleResult === "success") {
      const email = params.get("email");
      toast(email ? `Account ${email} linked successfully` : "Google account linked successfully");
      window.history.replaceState({}, "", window.location.pathname);
      load();
    } else if (googleResult === "error") {
      const message = params.get("message") || "OAuth flow failed";
      toast(message, true);
      window.history.replaceState({}, "", window.location.pathname);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleConnect = useCallback(async () => {
    if (!authEmail) return;
    setAuthLoading(true);
    try {
      const resp = await gatewayGoogleAuthInit(authEmail);
      setAuthUrl(resp.auth_url);
      window.open(resp.auth_url, "_blank");
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to start OAuth flow", true);
    } finally {
      setAuthLoading(false);
    }
  }, [authEmail, toast]);

  const handlePasteCallback = useCallback(async () => {
    if (!callbackUrl) return;
    setAuthLoading(true);
    try {
      const resp = await gatewayGoogleAuthComplete(callbackUrl, authEmail);
      toast(`Account ${resp.email} linked successfully`);
      setAuthUrl("");
      setCallbackUrl("");
      setAuthEmail("");
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to complete OAuth", true);
    } finally {
      setAuthLoading(false);
    }
  }, [callbackUrl, authEmail, toast, load]);

  const handleRemoveAccount = useCallback(async (email: string) => {
    const account = accounts.find((a) => a.email === email);
    const assignedTo = account?.assigned_to ?? [];
    const msg = assignedTo.length > 0
      ? `Remove ${email} from gateway and unassign from ${assignedTo.join(", ")}?`
      : `Remove ${email} from gateway?`;
    if (!confirm(msg)) return;
    try {
      await deleteGatewayGoogleAccount(email);
      toast(`Removed ${email}`);
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Remove failed", true);
    }
  }, [accounts, toast, load]);

  if (loading && accounts.length === 0) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading Google accounts...
      </div>
    );
  }

  return (
    <div style={{ padding: 24, maxWidth: 600 }}>
      <div style={{ marginBottom: 24 }}>
        <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
          Google Workspace
        </span>
        <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--text-dim)", marginTop: 4 }}>
          Manage gateway-level Google accounts. Assign them to agents on the agent CONNECT page.
        </div>
      </div>

      {/* Gateway Accounts */}
      <div>
        <div style={{ ...labelStyle, marginBottom: 10 }}>Gateway Accounts</div>
        {accounts.length > 0 ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 16 }}>
            {accounts.map((account) => (
              <div
                key={account.email}
                style={{
                  display: "flex", alignItems: "center", justifyContent: "space-between",
                  padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
                  clipPath: clipCorner(6),
                }}
              >
                <div>
                  <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, color: "var(--text-primary)" }}>
                    {account.email}
                  </span>
                  {account.assigned_to.length > 0 && (
                    <span style={{ marginLeft: 8, fontSize: 10, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                      [{account.assigned_to.join(", ")}]
                    </span>
                  )}
                </div>
                <button onClick={() => handleRemoveAccount(account.email)} style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}>
                  Remove
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13, marginBottom: 16 }}>
            No Google accounts connected to the gateway.
          </div>
        )}
      </div>

      {/* Connect New Account */}
      <div style={{ marginTop: 24, padding: 16, border: "1px solid var(--border)", clipPath: clipCorner(8) }}>
        <div style={{ ...labelStyle, marginBottom: 8 }}>Connect New Account</div>
        <div style={{ display: "flex", gap: 8, marginBottom: authUrl ? 12 : 0 }}>
          <input
            value={authEmail}
            onChange={(e) => setAuthEmail(e.target.value)}
            placeholder="user@gmail.com"
            style={{ ...inputStyle, flex: 1 }}
            disabled={!!authUrl}
          />
          <button
            onClick={handleConnect}
            disabled={!authEmail || authLoading || !!authUrl}
            style={{
              ...btnPrimary,
              opacity: authEmail && !authLoading && !authUrl ? 1 : 0.4,
              cursor: authEmail && !authLoading && !authUrl ? "pointer" : "default",
            }}
          >
            {authLoading ? "..." : "Connect"}
          </button>
        </div>

        {authUrl && (
          <div>
            <div style={{ ...labelStyle, marginBottom: 4 }}>
              After approving in Google, paste the redirect URL here:
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              <input
                value={callbackUrl}
                onChange={(e) => setCallbackUrl(e.target.value)}
                placeholder="Paste the full callback URL from your browser"
                style={{ ...inputStyle, flex: 1 }}
              />
              <button
                onClick={handlePasteCallback}
                disabled={!callbackUrl || authLoading}
                style={{
                  ...btnPrimary,
                  opacity: callbackUrl && !authLoading ? 1 : 0.4,
                  cursor: callbackUrl && !authLoading ? "pointer" : "default",
                }}
              >
                {authLoading ? "..." : "Complete"}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Signal Gateway Tab ──

function SignalGatewayTab({ toast }: Props) {
  const [connections, setConnections] = useState<SignalConnection[]>([]);
  const [daemonRunning, setDaemonRunning] = useState(false);
  const [loading, setLoading] = useState(true);

  // Link flow state
  const [linkLoading, setLinkLoading] = useState(false);
  const [linkUri, setLinkUri] = useState("");
  const [linkId, setLinkId] = useState("");
  const [linkName, setLinkName] = useState("");
  const [linkAccount, setLinkAccount] = useState("");
  const [finishLoading, setFinishLoading] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    listSignalConnections()
      .then((resp) => {
        setConnections(resp.connections);
        setDaemonRunning(resp.daemon_running);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Signal connections", true))
      .finally(() => setLoading(false));
  }, [toast]);

  useEffect(() => {
    load();
  }, [load]);

  const handleStartLink = useCallback(async () => {
    setLinkLoading(true);
    try {
      const resp = await signalLinkStart("ZeroClaw");
      setLinkUri(resp.device_link_uri);
      setLinkId(resp.link_id);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to start Signal linking", true);
    } finally {
      setLinkLoading(false);
    }
  }, [toast]);

  const handleFinishLink = useCallback(async () => {
    if (!linkName || !linkAccount || !linkId) return;
    setFinishLoading(true);
    try {
      await signalLinkFinish(linkId, linkName, linkAccount);
      toast(`Signal connection "${linkName}" linked successfully`);
      setLinkUri("");
      setLinkId("");
      setLinkName("");
      setLinkAccount("");
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to complete linking", true);
    } finally {
      setFinishLoading(false);
    }
  }, [linkId, linkName, linkAccount, toast, load]);

  const handleCancelLink = useCallback(() => {
    setLinkUri("");
    setLinkId("");
    setLinkName("");
    setLinkAccount("");
  }, []);

  const handleRemoveConnection = useCallback(async (name: string) => {
    const conn = connections.find((c) => c.name === name);
    const assignedTo = conn?.assigned_to ?? [];
    const msg = assignedTo.length > 0
      ? `Remove "${name}" and unassign from ${assignedTo.join(", ")}?`
      : `Remove Signal connection "${name}"?`;
    if (!confirm(msg)) return;
    try {
      await deleteSignalConnection(name);
      toast(`Removed "${name}"`);
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Remove failed", true);
    }
  }, [connections, toast, load]);

  if (loading && connections.length === 0) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading Signal connections...
      </div>
    );
  }

  return (
    <div style={{ padding: 24, maxWidth: 600 }}>
      <div style={{ marginBottom: 24 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
            Signal
          </span>
          <span
            style={{
              fontFamily: "JetBrains Mono, monospace", fontSize: 10, fontWeight: 600,
              padding: "2px 8px", borderRadius: 4,
              background: daemonRunning ? "rgba(34,197,94,0.15)" : "rgba(239,68,68,0.15)",
              color: daemonRunning ? "#22c55e" : "#ef4444",
            }}
          >
            {daemonRunning ? "DAEMON RUNNING" : "DAEMON STOPPED"}
          </span>
        </div>
        <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--text-dim)", marginTop: 4 }}>
          Manage gateway-level Signal connections. Link your phone via QR code, then assign to agents on the agent CONNECT page.
        </div>
      </div>

      {/* Existing Connections */}
      <div>
        <div style={{ ...labelStyle, marginBottom: 10 }}>Gateway Connections</div>
        {connections.length > 0 ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 16 }}>
            {connections.map((conn) => (
              <div
                key={conn.name}
                style={{
                  display: "flex", alignItems: "center", justifyContent: "space-between",
                  padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
                  clipPath: clipCorner(6),
                }}
              >
                <div>
                  <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 13, color: "var(--text-primary)" }}>
                    {conn.name}
                  </span>
                  <span style={{ marginLeft: 8, fontSize: 11, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                    {conn.account}
                  </span>
                  {conn.assigned_to.length > 0 && (
                    <span style={{ marginLeft: 8, fontSize: 10, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                      [{conn.assigned_to.join(", ")}]
                    </span>
                  )}
                </div>
                <button onClick={() => handleRemoveConnection(conn.name)} style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}>
                  Remove
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13, marginBottom: 16 }}>
            No Signal connections. Link an account below.
          </div>
        )}
      </div>

      {/* Link New Account */}
      <div style={{ marginTop: 24, padding: 16, border: "1px solid var(--border)", clipPath: clipCorner(8) }}>
        <div style={{ ...labelStyle, marginBottom: 8 }}>Link New Account</div>

        {!linkUri ? (
          <div>
            <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--text-dim)", marginBottom: 12 }}>
              This will generate a QR code URI. Open it in a QR code viewer or scan it with your Signal app
              to link your phone number as a secondary device.
            </div>
            <button
              onClick={handleStartLink}
              disabled={linkLoading}
              style={{
                ...btnPrimary,
                opacity: linkLoading ? 0.4 : 1,
                cursor: linkLoading ? "default" : "pointer",
              }}
            >
              {linkLoading ? "Starting..." : "Start Link"}
            </button>
          </div>
        ) : (
          <div>
            {/* QR Code display */}
            <div style={{ ...labelStyle, marginBottom: 4 }}>Scan QR Code</div>
            <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--text-dim)", marginBottom: 12 }}>
              Open Signal on your phone &rarr; Settings &rarr; Linked Devices &rarr; Link New Device, then scan:
            </div>
            <div
              style={{
                display: "flex", justifyContent: "center", padding: 20,
                background: "#ffffff", border: "1px solid var(--border)",
                clipPath: clipCorner(8), marginBottom: 16,
              }}
            >
              <QRCodeSVG value={linkUri} size={220} level="M" />
            </div>
            <details style={{ marginBottom: 16 }}>
              <summary style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 10, color: "var(--text-dim)", cursor: "pointer" }}>
                Show raw URI
              </summary>
              <div
                style={{
                  marginTop: 6, padding: "8px 12px", background: "var(--bg-input)",
                  border: "1px solid var(--border)", clipPath: clipCorner(6),
                  fontFamily: "JetBrains Mono, monospace", fontSize: 10,
                  color: "var(--text-primary)", wordBreak: "break-all", userSelect: "all",
                }}
              >
                {linkUri}
              </div>
            </details>

            {/* Finish form */}
            <div style={{ ...labelStyle, marginBottom: 4 }}>After scanning, complete the link:</div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <input
                value={linkName}
                onChange={(e) => setLinkName(e.target.value)}
                placeholder='Connection name (e.g. "support-line")'
                style={inputStyle}
              />
              <input
                value={linkAccount}
                onChange={(e) => setLinkAccount(e.target.value)}
                placeholder="Phone number (e.g. +1234567890)"
                style={inputStyle}
              />
              <div style={{ display: "flex", gap: 8 }}>
                <button
                  onClick={handleFinishLink}
                  disabled={!linkName || !linkAccount || finishLoading}
                  style={{
                    ...btnPrimary,
                    opacity: linkName && linkAccount && !finishLoading ? 1 : 0.4,
                    cursor: linkName && linkAccount && !finishLoading ? "pointer" : "default",
                  }}
                >
                  {finishLoading ? "Completing..." : "Complete Link"}
                </button>
                <button onClick={handleCancelLink} style={btnSecondary}>
                  Cancel
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Composio Gateway Tab ──

function ComposioGatewayTab({ toast }: Props) {
  const [config, setConfig] = useState<ComposioGatewayConfig | null>(null);
  const [connections, setConnections] = useState<ComposioConnectionInfo[]>([]);
  const [apps, setApps] = useState<ComposioApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [connectName, setConnectName] = useState("");
  const [connectLoading, setConnectLoading] = useState<string | null>(null);
  const [syncLoading, setSyncLoading] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    Promise.all([getComposioGatewayConfig(), listComposioConnections()])
      .then(([cfg, conns]) => {
        setConfig(cfg);
        setConnections(conns.connections);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Composio config", true))
      .finally(() => setLoading(false));
  }, [toast]);

  // Load available apps when API key is configured
  useEffect(() => {
    if (config?.has_api_key) {
      listComposioApps()
        .then((resp) => setApps(resp.apps))
        .catch(() => {}); // silently fail — apps list is best-effort
    }
  }, [config?.has_api_key]);

  useEffect(() => {
    load();
  }, [load]);

  const handleConnect = useCallback(async (app: ComposioApp) => {
    if (!connectName.trim()) {
      toast("Enter a connection name first (e.g. \"ava gmail\")", true);
      return;
    }
    setConnectLoading(app.id);
    try {
      const resp = await composioConnectInit({ name: connectName.trim(), app: app.toolkit_slug, auth_config_id: app.id });
      if (resp.redirect_url) {
        window.open(resp.redirect_url, "_blank");
        toast("OAuth window opened. Complete authorization and refresh this page.");
      } else {
        toast("Connection initiated but no redirect URL returned", true);
      }
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to initiate connection", true);
    } finally {
      setConnectLoading(null);
    }
  }, [connectName, toast]);

  const handleRemove = useCallback(async (connectionId: string) => {
    if (!confirm("Remove this Composio connection from all instances?")) return;
    try {
      await deleteComposioConnectionGlobal(connectionId);
      toast("Connection removed");
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Remove failed", true);
    }
  }, [toast, load]);

  const handleSync = useCallback(async () => {
    setSyncLoading(true);
    try {
      const resp = await syncComposioConnections();
      setConnections(resp.connections);
      toast(resp.synced > 0 ? `Synced ${resp.synced} new connection(s) from Composio` : "Already up to date");
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Sync failed", true);
    } finally {
      setSyncLoading(false);
    }
  }, [toast]);

  if (loading && !config) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading Composio config...
      </div>
    );
  }

  return (
    <div style={{ padding: 24, maxWidth: 600 }}>
      <div style={{ marginBottom: 24 }}>
        <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
          Composio
        </span>
        <div style={{ fontFamily: "Outfit, sans-serif", fontSize: 12, color: "var(--text-dim)", marginTop: 4 }}>
          Manage gateway-level Composio connections. Connect OAuth apps and assign them to agents on the agent CONNECT page.
        </div>
      </div>

      {/* API Key Status */}
      <div style={{ marginBottom: 20 }}>
        <div style={{ ...labelStyle, marginBottom: 6 }}>API Key</div>
        <div style={{
          padding: "8px 12px", background: "var(--bg-input)", border: "1px solid var(--border)",
          clipPath: clipCorner(6), fontFamily: "JetBrains Mono, monospace", fontSize: 12,
          color: config?.has_api_key ? "#22c55e" : "var(--text-dim)",
        }}>
          {config?.has_api_key ? "Configured (via COMPOSIO_API_KEY env var)" : "Not configured. Set COMPOSIO_API_KEY env var and restart gateway."}
        </div>
      </div>

      {/* Connections */}
      <div>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 10 }}>
          <span style={labelStyle}>Connections ({connections.length})</span>
          {config?.has_api_key && (
            <button
              onClick={handleSync}
              disabled={syncLoading}
              style={{
                ...btnSecondary,
                padding: "4px 12px",
                fontSize: 10,
                opacity: syncLoading ? 0.4 : 1,
                cursor: syncLoading ? "default" : "pointer",
              }}
            >
              {syncLoading ? "Syncing..." : "Sync from Composio"}
            </button>
          )}
        </div>
        {connections.length > 0 ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 16 }}>
            {connections.map((conn) => (
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
                  {conn.assigned_to.length > 0 && (
                    <span style={{ marginLeft: 8, fontSize: 10, color: "var(--text-dim)", fontFamily: "JetBrains Mono, monospace" }}>
                      [{conn.assigned_to.join(", ")}]
                    </span>
                  )}
                  <span style={{
                    marginLeft: 8, fontSize: 9, fontWeight: 600, padding: "1px 6px", borderRadius: 3,
                    background: conn.status === "ACTIVE" ? "rgba(34,197,94,0.15)" : "rgba(239,68,68,0.15)",
                    color: conn.status === "ACTIVE" ? "#22c55e" : "#ef4444",
                    fontFamily: "JetBrains Mono, monospace",
                  }}>
                    {conn.status}
                  </span>
                </div>
                <button onClick={() => handleRemove(conn.id)} style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}>
                  Remove
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 13, marginBottom: 16 }}>
            No Composio connections. Connect an app below.
          </div>
        )}
      </div>

      {/* Connect New App */}
      {config?.has_api_key && (
        <div style={{ marginTop: 24, padding: 16, border: "1px solid var(--border)", clipPath: clipCorner(8) }}>
          <div style={{ ...labelStyle, marginBottom: 8 }}>Connect New App</div>
          <input
            value={connectName}
            onChange={(e) => setConnectName(e.target.value)}
            placeholder='Connection name (e.g. "ava gmail")'
            style={{ ...inputStyle, marginBottom: 12 }}
          />
          {apps.length > 0 ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {apps.map((app) => (
                <div
                  key={app.id}
                  style={{
                    display: "flex", alignItems: "center", justifyContent: "space-between",
                    padding: "6px 10px", background: "var(--bg-input)", border: "1px solid var(--border)",
                    clipPath: clipCorner(4),
                  }}
                >
                  <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 12, color: "var(--text-primary)" }}>
                    {app.toolkit_slug || app.name}
                  </span>
                  <button
                    onClick={() => handleConnect(app)}
                    disabled={connectLoading === app.id}
                    style={{
                      ...btnPrimary, padding: "3px 12px", fontSize: 10,
                      opacity: connectLoading === app.id ? 0.4 : 1,
                      cursor: connectLoading === app.id ? "default" : "pointer",
                    }}
                  >
                    {connectLoading === app.id ? "..." : "Connect"}
                  </button>
                </div>
              ))}
            </div>
          ) : (
            <div style={{ color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 12 }}>
              No apps found. Configure auth integrations in your Composio dashboard first.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ── Main Integrations Page (Gateway-Level) ──

export default function Integrations({ toast }: Props) {
  const [tab, setTab] = useState<Tab>("google");

  return (
    <div style={{ display: "flex", flexDirection: "column", flex: 1, overflow: "hidden" }}>
      <div
        style={{
          display: "flex",
          gap: 0,
          borderBottom: "1px solid var(--border)",
          background: "var(--bg-card)",
          minHeight: 40,
        }}
      >
        <button onClick={() => setTab("google")} style={tabBtnStyle(tab === "google")}>
          Google
        </button>
        <button onClick={() => setTab("signal")} style={tabBtnStyle(tab === "signal")}>
          Signal
        </button>
        <button onClick={() => setTab("composio")} style={tabBtnStyle(tab === "composio")}>
          Composio
        </button>
      </div>

      <div style={{ flex: 1, overflowY: "auto" }}>
        {tab === "google" && <GoogleGatewayTab toast={toast} />}
        {tab === "signal" && <SignalGatewayTab toast={toast} />}
        {tab === "composio" && <ComposioGatewayTab toast={toast} />}
      </div>
    </div>
  );
}
