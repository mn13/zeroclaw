import { useState, useEffect, useCallback } from "react";
import { getStatus, clearHistory, getModels, setDefaultModel } from "../api";
import type { StatusInfo, ModelsInfo } from "../api";
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
  const [modelsInfo, setModelsInfo] = useState<ModelsInfo | null>(null);
  const [selectedModel, setSelectedModel] = useState("");
  const [modelSaving, setModelSaving] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    getStatus(instanceId)
      .then(setStatus)
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
    getModels(instanceId)
      .then((m) => {
        setModelsInfo(m);
        setSelectedModel(m.default_model);
      })
      .catch(() => {/* models info is optional */});
    setTokenUsage(readTokenUsage());
  }, [instanceId, toast]);

  const handleModelChange = useCallback(async (value: string) => {
    setSelectedModel(value);
    setModelSaving(true);
    try {
      // Find the provider for the selected model from model_routes
      const route = modelsInfo?.model_routes.find((r) => r.model === value);
      const provider = route?.provider;
      const result = await setDefaultModel(instanceId, value, provider);
      const msg = `Model set to ${value}` + (result.requires_restart ? " — restart required" : "");
      toast(msg);
      // Update local state immediately so provider display reflects the change
      if (modelsInfo && provider) {
        setModelsInfo({ ...modelsInfo, default_model: value, default_provider: provider });
      }
      // Reload status to reflect the change
      getStatus(instanceId).then(setStatus).catch(() => {});
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to set model", true);
      // Revert selection
      if (modelsInfo) setSelectedModel(modelsInfo.default_model);
    } finally {
      setModelSaving(false);
    }
  }, [instanceId, modelsInfo, toast]);

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
              alignItems: "flex-end",
            }}
          >
            <div style={{ flex: 1, minWidth: 220 }}>
              <div style={labelStyle}>Default Model</div>
              {modelsInfo && modelsInfo.model_routes.length > 0 ? (
                <select
                  value={selectedModel}
                  onChange={(e) => handleModelChange(e.target.value)}
                  disabled={modelSaving}
                  style={{
                    width: "100%",
                    padding: "8px 12px",
                    background: "var(--bg-input)",
                    border: "1px solid var(--border)",
                    color: "var(--amber)",
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 13,
                    fontWeight: 600,
                    clipPath: clipCorner(6),
                    outline: "none",
                    cursor: modelSaving ? "wait" : "pointer",
                    opacity: modelSaving ? 0.5 : 1,
                    appearance: "none",
                    WebkitAppearance: "none",
                    backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23f59e0b' d='M2 4l4 4 4-4'/%3E%3C/svg%3E")`,
                    backgroundRepeat: "no-repeat",
                    backgroundPosition: "right 10px center",
                    paddingRight: 30,
                  }}
                >
                  {/* Deduplicate: include all route models + current default if not in routes */}
                  {(() => {
                    const models = new Map<string, string>();
                    if (modelsInfo.default_model && !modelsInfo.model_routes.find((r) => r.model === modelsInfo.default_model)) {
                      models.set(modelsInfo.default_model, modelsInfo.default_provider);
                    }
                    for (const r of modelsInfo.model_routes) {
                      models.set(r.model, r.provider);
                    }
                    return Array.from(models.entries()).map(([model, provider]) => (
                      <option key={model} value={model}>
                        {model} ({provider})
                      </option>
                    ));
                  })()}
                </select>
              ) : (
                <div style={valueSm}>{status.model}</div>
              )}
            </div>
            <div>
              <div style={labelStyle}>Provider</div>
              <div style={valueSm}>
                {modelsInfo ? modelsInfo.default_provider || status.provider : status.provider}
              </div>
            </div>
          </div>
          {modelsInfo && modelsInfo.model_routes.length > 0 && (
            <div style={{ marginTop: 12, display: "flex", flexWrap: "wrap", gap: 6 }}>
              {modelsInfo.model_routes.map((r) => (
                <span
                  key={r.hint}
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 10,
                    padding: "3px 8px",
                    background: r.model === selectedModel ? "var(--amber-glow)" : "var(--bg-input)",
                    border: `1px solid ${r.model === selectedModel ? "var(--amber)" : "var(--border)"}`,
                    color: r.model === selectedModel ? "var(--amber)" : "var(--text-dim)",
                    clipPath: clipCorner(4),
                  }}
                >
                  {r.hint}: {r.model}
                </span>
              ))}
            </div>
          )}
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
