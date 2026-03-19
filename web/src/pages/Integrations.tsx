import { useState, useEffect, useCallback } from "react";
import {
  listGatewayGoogleAccounts,
  gatewayGoogleAuthInit,
  gatewayGoogleAuthComplete,
  deleteGatewayGoogleAccount,
} from "../api";
import type { GatewayGoogleAccount } from "../api";
import { clipCorner } from "../theme";

interface Props {
  toast: (msg: string, isError?: boolean) => void;
}

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

// ── Main Integrations Page (Gateway-Level) ──

export default function Integrations({ toast }: Props) {
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
        <button
          style={{
            fontFamily: "Syne, sans-serif",
            fontSize: 12,
            fontWeight: 700,
            textTransform: "uppercase",
            letterSpacing: 2,
            padding: "8px 20px",
            background: "var(--amber-glow)",
            border: "none",
            borderBottom: "2px solid var(--amber)",
            color: "var(--amber)",
            cursor: "pointer",
            transition: "all 0.15s",
          }}
        >
          Google
        </button>
      </div>

      <div style={{ flex: 1, overflowY: "auto" }}>
        <GoogleGatewayTab toast={toast} />
      </div>
    </div>
  );
}
