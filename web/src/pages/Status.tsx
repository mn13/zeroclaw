import { useState, useEffect, useCallback } from "react";
import { getStatus, clearHistory } from "../api";
import type { StatusInfo } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
  onLogout: () => void;
}

interface TokenUsage {
  input: number;
  output: number;
  turns: number;
  lastReset: string;
}

const TOKEN_STORAGE_KEY = "zcgw-token-usage";

function readTokenUsage(): TokenUsage {
  try {
    const raw = localStorage.getItem(TOKEN_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      return {
        input: parsed.input ?? 0,
        output: parsed.output ?? 0,
        turns: parsed.turns ?? 0,
        lastReset: parsed.lastReset ?? new Date().toISOString(),
      };
    }
  } catch {
    // ignore parse errors
  }
  return { input: 0, output: 0, turns: 0, lastReset: new Date().toISOString() };
}

function resetTokenUsage(): TokenUsage {
  const fresh: TokenUsage = {
    input: 0,
    output: 0,
    turns: 0,
    lastReset: new Date().toISOString(),
  };
  localStorage.setItem(TOKEN_STORAGE_KEY, JSON.stringify(fresh));
  return fresh;
}

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}d`);
  if (h > 0) parts.push(`${h}h`);
  if (m > 0) parts.push(`${m}m`);
  parts.push(`${s}s`);
  return parts.join(" ");
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}

export default function Status({ instanceId, toast, onLogout }: Props) {
  const [status, setStatus] = useState<StatusInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [tokenUsage, setTokenUsage] = useState<TokenUsage>(readTokenUsage);

  const load = useCallback(() => {
    setLoading(true);
    getStatus(instanceId)
      .then(setStatus)
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
    setTokenUsage(readTokenUsage());
  }, [instanceId, toast]);

  useEffect(() => {
    load();
  }, [load]);

  const handleClear = useCallback(async () => {
    try {
      const result = await clearHistory(instanceId);
      toast(`Cleared ${result.messages_cleared} message(s)`);
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Clear failed", true);
    }
  }, [instanceId, toast, load]);

  const handleResetTokens = useCallback(() => {
    const fresh = resetTokenUsage();
    setTokenUsage(fresh);
    toast("Token counters reset");
  }, [toast]);

  const sectionHeading: React.CSSProperties = {
    fontFamily: "Syne, sans-serif",
    fontSize: 13,
    fontWeight: 700,
    color: "var(--text-dim)",
    marginBottom: 12,
    marginTop: 28,
    textTransform: "uppercase",
    letterSpacing: 2,
  };

  const cardStyle: React.CSSProperties = {
    background: "var(--bg-card)",
    border: "1px solid var(--border)",
    clipPath: clipCorner(10),
    padding: "14px 18px",
    minWidth: 140,
  };

  const wideCardStyle: React.CSSProperties = {
    ...cardStyle,
    flex: 1,
    minWidth: 260,
  };

  const labelStyle: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 10,
    fontWeight: 600,
    textTransform: "uppercase",
    color: "var(--text-dim)",
    letterSpacing: 1,
    marginBottom: 6,
  };

  const valueStyle: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 22,
    fontWeight: 700,
    color: "var(--amber)",
  };

  const valueSm: React.CSSProperties = {
    ...valueStyle,
    fontSize: 14,
  };

  const tokenRowStyle: React.CSSProperties = {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    padding: "6px 0",
    borderBottom: "1px solid var(--border)",
  };

  const tokenLabelStyle: React.CSSProperties = {
    fontFamily: "Outfit, sans-serif",
    fontSize: 13,
    color: "var(--text-secondary)",
  };

  const tokenValueStyle: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 14,
    fontWeight: 600,
    color: "var(--text-primary)",
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

  const btnDanger: React.CSSProperties = {
    ...btnSecondary,
    borderColor: "var(--error-text)",
    color: "var(--error-text)",
  };

  if (loading || !status) {
    return (
      <div
        style={{
          padding: 32,
          color: "var(--text-dim)",
          fontFamily: "Outfit, sans-serif",
          fontSize: 14,
        }}
      >
        Loading status...
      </div>
    );
  }

  const totalTokens = tokenUsage.input + tokenUsage.output;
  const avgPerTurn =
    tokenUsage.turns > 0 ? Math.round(totalTokens / tokenUsage.turns) : 0;

  return (
    <div style={{ padding: 24, maxWidth: 900, flex: 1, overflowY: "auto" }}>
      <h2
        style={{
          fontFamily: "Syne, sans-serif",
          fontSize: 18,
          fontWeight: 700,
          color: "var(--amber)",
          marginBottom: 20,
          textTransform: "uppercase",
          letterSpacing: 1,
        }}
      >
        Status
      </h2>

      {/* Stat cards */}
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 12,
          marginBottom: 12,
        }}
      >
        <div style={cardStyle}>
          <div style={labelStyle}>State</div>
          <div style={valueStyle}>{status.state}</div>
        </div>
        <div style={cardStyle}>
          <div style={labelStyle}>Uptime</div>
          <div style={valueStyle}>{formatUptime(status.uptime_secs)}</div>
        </div>
        <div style={cardStyle}>
          <div style={labelStyle}>Total Turns</div>
          <div style={valueStyle}>{status.total_turns}</div>
        </div>
        <div style={cardStyle}>
          <div style={labelStyle}>History Length</div>
          <div style={valueStyle}>{status.history_length}</div>
        </div>
      </div>

      {/* Token Consumption */}
      <div style={sectionHeading}>Token Consumption</div>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 12,
          marginBottom: 12,
        }}
      >
        <div style={wideCardStyle}>
          <div style={labelStyle}>Session Tokens</div>
          <div style={{ marginTop: 4 }}>
            <div style={tokenRowStyle}>
              <span style={tokenLabelStyle}>Input</span>
              <span style={tokenValueStyle}>
                {formatNumber(tokenUsage.input)}
              </span>
            </div>
            <div style={tokenRowStyle}>
              <span style={tokenLabelStyle}>Output</span>
              <span style={tokenValueStyle}>
                {formatNumber(tokenUsage.output)}
              </span>
            </div>
            <div style={tokenRowStyle}>
              <span style={tokenLabelStyle}>Combined</span>
              <span style={{ ...tokenValueStyle, color: "var(--amber)" }}>
                {formatNumber(totalTokens)}
              </span>
            </div>
            <div style={tokenRowStyle}>
              <span style={tokenLabelStyle}>Turns</span>
              <span style={tokenValueStyle}>{tokenUsage.turns}</span>
            </div>
            <div style={{ ...tokenRowStyle, borderBottom: "none" }}>
              <span style={tokenLabelStyle}>Avg / Turn</span>
              <span style={tokenValueStyle}>{formatNumber(avgPerTurn)}</span>
            </div>
          </div>
          <div style={{ marginTop: 12 }}>
            <button onClick={handleResetTokens} style={btnSecondary}>
              Reset Counters
            </button>
          </div>
          {tokenUsage.lastReset && (
            <div
              style={{
                fontFamily: "JetBrains Mono, monospace",
                fontSize: 10,
                color: "var(--text-dim)",
                marginTop: 8,
              }}
            >
              Last reset:{" "}
              {new Date(tokenUsage.lastReset).toLocaleString()}
            </div>
          )}
        </div>
      </div>

      {/* Agent Info */}
      <div style={sectionHeading}>Agent Info</div>
      <div
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 12,
          marginBottom: 24,
        }}
      >
        <div style={wideCardStyle}>
          <div
            style={{
              display: "flex",
              gap: 32,
              flexWrap: "wrap",
            }}
          >
            <div>
              <div style={labelStyle}>Model</div>
              <div style={valueSm}>{status.model}</div>
            </div>
            <div>
              <div style={labelStyle}>Provider</div>
              <div style={valueSm}>{status.provider}</div>
            </div>
          </div>
        </div>
      </div>

      {/* Action bar */}
      <div style={{ display: "flex", gap: 10 }}>
        <button onClick={load} style={btnSecondary}>
          Refresh
        </button>
        <button onClick={handleClear} style={btnDanger}>
          Clear History
        </button>
        <button onClick={onLogout} style={btnDanger}>
          Logout
        </button>
      </div>
    </div>
  );
}
