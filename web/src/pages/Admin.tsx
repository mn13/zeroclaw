import { useState, useEffect, useCallback } from "react";
import {
  getAdminStats,
  getAdminConfig,
  updateAdminConfig,
  listAdminInstances,
  createInstance,
  instanceAction,
  getAgentTemplate,
  getWorkspaceTemplates,
  batchUpdateIdentity,
} from "../api";
import type { AdminStats, DetailedInstance, InstanceInfo, IdentityFile } from "../api";
import { clipCorner } from "../theme";

interface Props {
  toast: (msg: string, isError?: boolean) => void;
  instances: InstanceInfo[];
  onInstancesChange: () => void;
}

type Tab = "dashboard" | "instances" | "config";

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

type TomlSections = Record<string, Record<string, string>>;

function parseSimpleToml(toml: string): TomlSections {
  const sections: TomlSections = {};
  let currentSection = "general";
  sections[currentSection] = {};

  const lines = toml.split("\n");
  let i = 0;
  while (i < lines.length) {
    const trimmed = lines[i]!.trim();
    if (!trimmed || trimmed.startsWith("#")) { i++; continue; }

    const sectionMatch = trimmed.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      currentSection = sectionMatch[1]!;
      if (!sections[currentSection]) sections[currentSection] = {};
      i++;
      continue;
    }

    const kvMatch = trimmed.match(/^(\S+)\s*=\s*(.+)$/);
    if (kvMatch) {
      const key = kvMatch[1]!;
      let val = kvMatch[2]!.trim();

      // Handle multi-line arrays: if value starts with [ but doesn't end with ]
      if (val.startsWith("[") && !val.endsWith("]")) {
        i++;
        while (i < lines.length) {
          const cont = lines[i]!.trim();
          val += "\n" + cont;
          if (cont.endsWith("]")) { i++; break; }
          i++;
        }
      } else {
        i++;
      }

      // Strip surrounding quotes from simple string values
      if (
        (val.startsWith('"') && val.endsWith('"') && !val.startsWith('"[')) ||
        (val.startsWith("'") && val.endsWith("'"))
      ) {
        val = val.slice(1, -1);
      }
      sections[currentSection]![key] = val;
    } else {
      i++;
    }
  }
  return sections;
}

function isTomlLiteral(val: string): boolean {
  const t = val.trim();
  return (
    t === "true" || t === "false" ||
    (!isNaN(Number(t)) && t !== "") ||
    t.startsWith("[") ||
    t.startsWith("{")
  );
}

function sectionsToToml(sections: TomlSections): string {
  const lines: string[] = [];
  for (const [section, fields] of Object.entries(sections)) {
    if (section !== "general") {
      lines.push(`[${section}]`);
    }
    for (const [key, val] of Object.entries(fields)) {
      if (isTomlLiteral(val)) {
        lines.push(`${key} = ${val}`);
      } else {
        lines.push(`${key} = "${val}"`);
      }
    }
    lines.push("");
  }
  return lines.join("\n");
}

function isSensitiveKey(key: string): boolean {
  const lower = key.toLowerCase();
  return (
    lower.includes("key") ||
    lower.includes("token") ||
    lower.includes("secret") ||
    lower.includes("password")
  );
}

/** Extract model names from the TOML model_routes inline array string. */
function extractModelsFromToml(routesStr: string): Array<{ model: string; hint: string; provider: string }> {
  const models: Array<{ model: string; hint: string; provider: string }> = [];
  const blockRe = /\{[^}]*\}/g;
  let blockMatch;
  while ((blockMatch = blockRe.exec(routesStr)) !== null) {
    const block = blockMatch[0];
    const modelMatch = block.match(/model\s*=\s*"([^"]+)"/);
    const hintMatch = block.match(/hint\s*=\s*"([^"]+)"/);
    const providerMatch = block.match(/provider\s*=\s*"([^"]+)"/);
    if (modelMatch) {
      models.push({
        model: modelMatch[1]!,
        hint: hintMatch ? hintMatch[1]! : "",
        provider: providerMatch ? providerMatch[1]! : "",
      });
    }
  }
  return models;
}

export default function Admin({ toast, instances: _instances, onInstancesChange }: Props) {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [stats, setStats] = useState<AdminStats | null>(null);
  const [detailedInstances, setDetailedInstances] = useState<DetailedInstance[]>([]);
  const [configRaw, setConfigRaw] = useState("");
  const [configDirty, setConfigDirty] = useState(false);
  const [requiresRestart, setRequiresRestart] = useState(false);
  const [loading, setLoading] = useState(true);

  // Create agent form
  const [newId, setNewId] = useState("");
  const [newName, setNewName] = useState("");
  const [newConfig, setNewConfig] = useState<TomlSections>({});
  const [newTomlRaw, setNewTomlRaw] = useState("");
  const [showRawToml, setShowRawToml] = useState(false);
  const [templateLoaded, setTemplateLoaded] = useState(false);
  const [collapsedSections, setCollapsedSections] = useState<Record<string, boolean>>({});

  // Identity files for new agent (loaded from workspace templates)
  const [identityFiles, setIdentityFiles] = useState<IdentityFile[]>([
    { filename: "SOUL.md", content: "" },
    { filename: "IDENTITY.md", content: "" },
  ]);
  const [activeIdentityFile, setActiveIdentityFile] = useState<string>("SOUL.md");
  const [workspaceTemplates, setWorkspaceTemplates] = useState<IdentityFile[]>([]);

  // Confirm destroy
  const [destroyConfirm, setDestroyConfirm] = useState<string | null>(null);

  const loadDashboard = useCallback(() => {
    setLoading(true);
    Promise.all([getAdminStats(), listAdminInstances()])
      .then(([s, insts]) => {
        setStats(s);
        setDetailedInstances(insts);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load admin data", true))
      .finally(() => setLoading(false));
  }, [toast]);

  const loadConfig = useCallback(() => {
    getAdminConfig()
      .then((c) => {
        setConfigRaw(c.raw);
        setConfigDirty(false);
        setRequiresRestart(false);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load config", true));
  }, [toast]);

  const loadTemplate = useCallback(() => {
    if (templateLoaded) return;
    getAgentTemplate()
      .then((t) => {
        setNewTomlRaw(t.raw);
        setNewConfig(parseSimpleToml(t.raw));
        setTemplateLoaded(true);
      })
      .catch(() => {
        // template endpoint may not exist, that's ok
      });
    getWorkspaceTemplates()
      .then((res) => {
        if (res.files && res.files.length > 0) {
          setWorkspaceTemplates(res.files);
          setIdentityFiles(res.files.map((f) => ({ ...f })));
          setActiveIdentityFile(res.files[0]?.filename ?? "SOUL.md");
        }
      })
      .catch(() => {
        // workspace templates may not be configured
      });
  }, [templateLoaded]);

  useEffect(() => {
    loadDashboard();
  }, [loadDashboard]);

  useEffect(() => {
    if (tab === "config") loadConfig();
    if (tab === "instances") {
      loadDashboard();
      loadTemplate();
    }
  }, [tab, loadConfig, loadDashboard, loadTemplate]);

  const handleSaveConfig = useCallback(async () => {
    try {
      const result = await updateAdminConfig(configRaw);
      setConfigDirty(false);
      setRequiresRestart(result.requires_restart);
      toast("Configuration saved");
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
    }
  }, [configRaw, toast]);

  const handleAction = useCallback(
    async (id: string, action: "start" | "stop" | "destroy" | "reconnect") => {
      if (action === "destroy" && destroyConfirm !== id) {
        setDestroyConfirm(id);
        return;
      }
      try {
        await instanceAction(id, action, action === "destroy" ? true : undefined);
        toast(`Action '${action}' executed on ${id}`);
        setDestroyConfirm(null);
        loadDashboard();
        onInstancesChange();
        // Refresh again after health check completes for start/reconnect
        if (action === "start" || action === "reconnect") {
          setTimeout(() => { loadDashboard(); onInstancesChange(); }, 5000);
        }
      } catch (err: unknown) {
        toast(err instanceof Error ? err.message : `Action '${action}' failed`, true);
      }
    },
    [destroyConfirm, toast, loadDashboard, onInstancesChange],
  );

  const updateConfigField = useCallback(
    (section: string, key: string, value: string) => {
      setNewConfig((prev) => ({
        ...prev,
        [section]: {
          ...prev[section],
          [key]: value,
        },
      }));
    },
    [],
  );

  const toggleCreateSection = useCallback((section: string) => {
    setCollapsedSections((prev) => ({ ...prev, [section]: !prev[section] }));
  }, []);

  const handleCreate = useCallback(async () => {
    if (!newId.trim() || !newName.trim()) {
      toast("ID and Display Name are required", true);
      return;
    }
    const toml = showRawToml ? newTomlRaw : sectionsToToml(newConfig);
    try {
      await createInstance(newId.trim(), newName.trim(), toml);
      // Save identity files (non-empty ones)
      const filesToSave = identityFiles.filter((f) => f.content.trim());
      if (filesToSave.length > 0) {
        await batchUpdateIdentity(newId.trim(), filesToSave);
      }
      toast(`Instance '${newId.trim()}' created`);
      setNewId("");
      setNewName("");
      setTemplateLoaded(false);
      setNewConfig({});
      setNewTomlRaw("");
      setIdentityFiles(
        workspaceTemplates.length > 0
          ? workspaceTemplates.map((f) => ({ ...f }))
          : [{ filename: "SOUL.md", content: "" }, { filename: "IDENTITY.md", content: "" }],
      );
      loadDashboard();
      onInstancesChange();
      // Refresh again after health check completes (background check takes ~4-10s)
      setTimeout(() => { loadDashboard(); onInstancesChange(); }, 6000);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Create failed", true);
    }
  }, [newId, newName, newConfig, newTomlRaw, showRawToml, identityFiles, workspaceTemplates, toast, loadDashboard, onInstancesChange]);

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

  const btnPrimary: React.CSSProperties = {
    ...btnSecondary,
    borderColor: "var(--amber)",
    color: "var(--amber)",
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

  const healthDot = (health: string): React.CSSProperties => ({
    width: 8,
    height: 8,
    minWidth: 8,
    borderRadius: "50%",
    display: "inline-block",
    background:
      health === "healthy" ? "#22c55e" : health === "unhealthy" ? "#ef4444" : "#737373",
    boxShadow: health === "healthy" ? "0 0 4px rgba(34,197,94,0.5)" : "none",
  });

  const thStyle: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 10,
    fontWeight: 600,
    textTransform: "uppercase",
    color: "var(--text-dim)",
    letterSpacing: 1,
    padding: "8px 12px",
    textAlign: "left",
    borderBottom: "1px solid var(--border)",
  };

  const tdStyle: React.CSSProperties = {
    fontFamily: "Outfit, sans-serif",
    fontSize: 13,
    color: "var(--text-primary)",
    padding: "10px 12px",
    borderBottom: "1px solid var(--border)",
  };

  const tdMono: React.CSSProperties = {
    ...tdStyle,
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 12,
  };

  const renderDashboard = () => {
    if (loading || !stats) {
      return (
        <div
          style={{
            padding: 32,
            color: "var(--text-dim)",
            fontFamily: "Outfit, sans-serif",
            fontSize: 14,
          }}
        >
          Loading admin data...
        </div>
      );
    }

    return (
      <>
        {/* Stat cards */}
        <div style={{ display: "flex", flexWrap: "wrap", gap: 12, marginBottom: 12 }}>
          <div style={cardStyle}>
            <div style={labelStyle}>Gateway Uptime</div>
            <div style={valueStyle}>{formatUptime(stats.uptime_secs)}</div>
          </div>
          <div style={cardStyle}>
            <div style={labelStyle}>Total Instances</div>
            <div style={valueStyle}>{stats.total_instances}</div>
          </div>
          <div style={cardStyle}>
            <div style={labelStyle}>Healthy</div>
            <div style={{ ...valueStyle, color: "#22c55e" }}>{stats.healthy_count}</div>
          </div>
          <div style={cardStyle}>
            <div style={labelStyle}>Unhealthy</div>
            <div style={{ ...valueStyle, color: stats.unhealthy_count > 0 ? "#ef4444" : "var(--amber)" }}>
              {stats.unhealthy_count}
            </div>
          </div>
        </div>

        {/* Instance overview table */}
        <div style={sectionHeading}>Instance Overview</div>
        <div
          style={{
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            overflow: "hidden",
          }}
        >
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                <th style={thStyle}>Health</th>
                <th style={thStyle}>Name</th>
                <th style={thStyle}>ID</th>
                <th style={thStyle}>gRPC Address</th>
                <th style={thStyle}>Container Status</th>
              </tr>
            </thead>
            <tbody>
              {detailedInstances.map((inst) => (
                <tr key={inst.id} style={{ transition: "background 0.1s" }}>
                  <td style={tdStyle}>
                    <span style={healthDot(inst.health)} />
                  </td>
                  <td style={tdStyle}>{inst.display_name || inst.id}</td>
                  <td style={tdMono}>{inst.id}</td>
                  <td style={tdMono}>{inst.grpc_address}</td>
                  <td style={tdMono}>{inst.container_status || "—"}</td>
                </tr>
              ))}
              {detailedInstances.length === 0 && (
                <tr>
                  <td colSpan={5} style={{ ...tdStyle, textAlign: "center", color: "var(--text-dim)" }}>
                    No instances found
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div style={{ marginTop: 20 }}>
          <button onClick={loadDashboard} style={btnSecondary}>
            Refresh
          </button>
        </div>
      </>
    );
  };

  const renderInstances = () => (
    <>
      {/* Instance table with actions */}
      <div
        style={{
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          clipPath: clipCorner(10),
          overflow: "hidden",
        }}
      >
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th style={thStyle}>Health</th>
              <th style={thStyle}>Name</th>
              <th style={thStyle}>ID</th>
              <th style={thStyle}>gRPC Address</th>
              <th style={thStyle}>Container Status</th>
              <th style={thStyle}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {detailedInstances.map((inst) => {
              const status = (inst.container_status || "").toLowerCase();
              const isRunning = status === "running" || inst.health === "healthy";
              const isStopped = status === "stopped" || status === "exited";
              return (
                <tr key={inst.id}>
                  <td style={tdStyle}>
                    <span style={healthDot(inst.health)} />
                  </td>
                  <td style={tdStyle}>{inst.display_name || inst.id}</td>
                  <td style={tdMono}>{inst.id}</td>
                  <td style={tdMono}>{inst.grpc_address}</td>
                  <td style={tdMono}>{inst.container_status || "—"}</td>
                  <td style={tdStyle}>
                    <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                      {(isStopped || !isRunning) && (
                        <button
                          onClick={() => handleAction(inst.id, "start")}
                          style={{ ...btnSecondary, padding: "4px 10px", fontSize: 10 }}
                        >
                          Start
                        </button>
                      )}
                      {isRunning && (
                        <button
                          onClick={() => handleAction(inst.id, "stop")}
                          style={{ ...btnSecondary, padding: "4px 10px", fontSize: 10 }}
                        >
                          Stop
                        </button>
                      )}
                      <button
                        onClick={() => handleAction(inst.id, "reconnect")}
                        style={{ ...btnSecondary, padding: "4px 10px", fontSize: 10 }}
                      >
                        Reconnect
                      </button>
                      {destroyConfirm === inst.id ? (
                        <button
                          onClick={() => handleAction(inst.id, "destroy")}
                          style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}
                        >
                          Confirm Destroy
                        </button>
                      ) : (
                        <button
                          onClick={() => handleAction(inst.id, "destroy")}
                          style={{ ...btnDanger, padding: "4px 10px", fontSize: 10 }}
                        >
                          Destroy
                        </button>
                      )}
                    </div>
                  </td>
                </tr>
              );
            })}
            {detailedInstances.length === 0 && (
              <tr>
                <td colSpan={6} style={{ ...tdStyle, textAlign: "center", color: "var(--text-dim)" }}>
                  No instances found
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {/* Create Agent */}
      <div style={sectionHeading}>Create Agent</div>
      <div
        style={{
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          clipPath: clipCorner(10),
          padding: "18px 20px",
        }}
      >
        <div style={{ display: "flex", gap: 12, marginBottom: 14, flexWrap: "wrap" }}>
          <div style={{ flex: 1, minWidth: 180 }}>
            <div style={labelStyle}>ID</div>
            <input
              value={newId}
              onChange={(e) => setNewId(e.target.value)}
              placeholder="agent-id"
              style={{
                width: "100%",
                padding: "8px 12px",
                background: "var(--bg-input)",
                border: "1px solid var(--border)",
                color: "var(--text-primary)",
                fontFamily: "JetBrains Mono, monospace",
                fontSize: 13,
                clipPath: clipCorner(6),
                outline: "none",
                boxSizing: "border-box",
              }}
            />
          </div>
          <div style={{ flex: 1, minWidth: 180 }}>
            <div style={labelStyle}>Display Name</div>
            <input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder="My Agent"
              style={{
                width: "100%",
                padding: "8px 12px",
                background: "var(--bg-input)",
                border: "1px solid var(--border)",
                color: "var(--text-primary)",
                fontFamily: "JetBrains Mono, monospace",
                fontSize: 13,
                clipPath: clipCorner(6),
                outline: "none",
                boxSizing: "border-box",
              }}
            />
          </div>
        </div>

        {/* Toggle between structured and raw TOML */}
        <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 14 }}>
          <div style={labelStyle}>Configuration</div>
          <button
            onClick={() => {
              if (!showRawToml) {
                // Switching to raw: serialize current structured config
                setNewTomlRaw(sectionsToToml(newConfig));
              } else {
                // Switching to structured: parse raw TOML
                setNewConfig(parseSimpleToml(newTomlRaw));
              }
              setShowRawToml(!showRawToml);
            }}
            style={{
              ...btnSecondary,
              padding: "3px 10px",
              fontSize: 10,
            }}
          >
            {showRawToml ? "Structured Editor" : "Raw TOML"}
          </button>
        </div>

        {showRawToml ? (
          <textarea
            value={newTomlRaw}
            onChange={(e) => setNewTomlRaw(e.target.value)}
            rows={16}
            style={{
              width: "100%",
              padding: "10px 12px",
              background: "var(--bg-input)",
              border: "1px solid var(--border)",
              color: "var(--text-primary)",
              fontFamily: "JetBrains Mono, monospace",
              fontSize: 12,
              lineHeight: 1.5,
              resize: "vertical",
              clipPath: clipCorner(8),
              outline: "none",
              boxSizing: "border-box",
            }}
          />
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            {Object.keys(newConfig).length === 0 && (
              <div
                style={{
                  padding: 16,
                  color: "var(--text-dim)",
                  fontFamily: "Outfit, sans-serif",
                  fontSize: 13,
                }}
              >
                No template loaded. Switch to Raw TOML to enter configuration manually.
              </div>
            )}
            {Object.entries(newConfig).map(([section, fields]) => {
              const isCollapsed = collapsedSections[section] ?? false;
              return (
                <div
                  key={section}
                  style={{
                    background: "var(--bg-surface, rgba(0,0,0,0.15))",
                    border: "1px solid var(--border)",
                    clipPath: clipCorner(8),
                    overflow: "hidden",
                  }}
                >
                  <div
                    onClick={() => toggleCreateSection(section)}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "8px 14px",
                      cursor: "pointer",
                      userSelect: "none",
                    }}
                  >
                    <span
                      style={{
                        fontFamily: "Syne, sans-serif",
                        fontSize: 12,
                        fontWeight: 600,
                        textTransform: "uppercase",
                        color: "var(--text-primary)",
                        letterSpacing: 0.8,
                      }}
                    >
                      {section}
                    </span>
                    <span
                      style={{
                        color: "var(--text-dim)",
                        fontSize: 11,
                        transition: "transform 0.2s",
                        transform: isCollapsed ? "rotate(-90deg)" : "rotate(0deg)",
                      }}
                    >
                      {"\u25BC"}
                    </span>
                  </div>

                  {!isCollapsed && (
                    <div style={{ padding: "4px 14px 12px" }}>
                      {Object.entries(fields).map(([key, val]) => {
                        const inputStyle: React.CSSProperties = {
                          fontFamily: "JetBrains Mono, monospace",
                          fontSize: 13,
                          background: "var(--bg-input)",
                          border: "1px solid var(--border)",
                          color: "var(--text-primary)",
                          padding: "5px 8px",
                          borderRadius: 0,
                          clipPath: clipCorner(6),
                          outline: "none",
                          flex: 1,
                        };
                        const fieldLabelStyle: React.CSSProperties = {
                          fontFamily: "Outfit, sans-serif",
                          fontSize: 13,
                          color: "var(--text-dim)",
                          minWidth: 180,
                          flexShrink: 0,
                        };

                        // default_model dropdown (extract models from model_routes)
                        if (key === "default_model" && section === "general") {
                          const routesStr = newConfig["general"]?.["model_routes"] || "";
                          const models = extractModelsFromToml(routesStr);
                          if (models.length > 0) {
                            const modelNames = models.map((m) => m.model);
                            if (val && !modelNames.includes(val)) modelNames.unshift(val);
                            return (
                              <div
                                key={key}
                                style={{
                                  display: "flex",
                                  alignItems: "center",
                                  gap: 12,
                                  padding: "6px 0",
                                }}
                              >
                                <span style={fieldLabelStyle}>{key}</span>
                                <select
                                  value={val}
                                  onChange={(e) => {
                                    const newModel = e.target.value;
                                    updateConfigField(section, key, newModel);
                                    const route = models.find((m) => m.model === newModel);
                                    if (route && route.provider) {
                                      updateConfigField(section, "default_provider", route.provider);
                                    }
                                  }}
                                  style={{ ...inputStyle, cursor: "pointer" }}
                                >
                                  {modelNames.map((m) => {
                                    const route = models.find((r) => r.model === m);
                                    return (
                                      <option key={m} value={m}>
                                        {m}{route ? ` (${route.hint})` : ""}
                                      </option>
                                    );
                                  })}
                                </select>
                              </div>
                            );
                          }
                        }

                        // Boolean toggle
                        if (val === "true" || val === "false") {
                          const boolVal = val === "true";
                          return (
                            <div
                              key={key}
                              style={{
                                display: "flex",
                                alignItems: "center",
                                gap: 12,
                                padding: "6px 0",
                              }}
                            >
                              <span style={fieldLabelStyle}>{key}</span>
                              <div
                                onClick={() =>
                                  updateConfigField(section, key, boolVal ? "false" : "true")
                                }
                                style={{
                                  width: 40,
                                  height: 22,
                                  borderRadius: 11,
                                  background: boolVal ? "var(--amber)" : "var(--toggle-off, #555)",
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
                                    left: boolVal ? 21 : 3,
                                    transition: "left 0.2s",
                                  }}
                                />
                              </div>
                            </div>
                          );
                        }

                        // Number input
                        if (!isNaN(Number(val)) && val.trim() !== "") {
                          return (
                            <div
                              key={key}
                              style={{
                                display: "flex",
                                alignItems: "center",
                                gap: 12,
                                padding: "6px 0",
                              }}
                            >
                              <span style={fieldLabelStyle}>{key}</span>
                              <input
                                type="number"
                                value={val}
                                onChange={(e) =>
                                  updateConfigField(section, key, e.target.value)
                                }
                                style={{ ...inputStyle, maxWidth: 120 }}
                              />
                            </div>
                          );
                        }

                        // String / sensitive input
                        return (
                          <div
                            key={key}
                            style={{
                              display: "flex",
                              alignItems: "center",
                              gap: 12,
                              padding: "6px 0",
                            }}
                          >
                            <span style={fieldLabelStyle}>{key}</span>
                            <input
                              type={isSensitiveKey(key) ? "password" : "text"}
                              value={val}
                              onChange={(e) =>
                                updateConfigField(section, key, e.target.value)
                              }
                              style={inputStyle}
                            />
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        {/* Identity Files */}
        <div style={{ ...labelStyle, marginTop: 18, marginBottom: 8 }}>Identity Files (Optional)</div>
        <div
          style={{
            display: "flex",
            gap: 6,
            marginBottom: 8,
            flexWrap: "wrap",
            alignItems: "center",
          }}
        >
          {identityFiles.map((f) => (
            <button
              key={f.filename}
              onClick={() => setActiveIdentityFile(f.filename)}
              style={{
                ...btnSecondary,
                padding: "4px 10px",
                fontSize: 10,
                borderColor: activeIdentityFile === f.filename ? "var(--amber)" : "var(--border)",
                color: activeIdentityFile === f.filename ? "var(--amber)" : f.content ? "var(--text-primary)" : "var(--text-dim)",
              }}
            >
              {f.filename}
              {f.content ? " *" : ""}
            </button>
          ))}
          <button
            onClick={() => {
              const name = prompt("Filename (e.g. CUSTOM.md):");
              if (!name) return;
              const fn = name.endsWith(".md") ? name : name + ".md";
              if (identityFiles.find((f) => f.filename === fn)) {
                setActiveIdentityFile(fn);
                return;
              }
              setIdentityFiles((prev) => [...prev, { filename: fn, content: "" }]);
              setActiveIdentityFile(fn);
            }}
            style={{ ...btnSecondary, padding: "4px 10px", fontSize: 10 }}
          >
            + Add
          </button>
        </div>
        {activeIdentityFile && (
          <textarea
            value={identityFiles.find((f) => f.filename === activeIdentityFile)?.content || ""}
            onChange={(e) => {
              const val = e.target.value;
              setIdentityFiles((prev) =>
                prev.map((f) => (f.filename === activeIdentityFile ? { ...f, content: val } : f)),
              );
            }}
            placeholder={`Write ${activeIdentityFile} content here...\nThis defines the agent's personality and behavior.`}
            rows={8}
            style={{
              width: "100%",
              padding: "10px 12px",
              background: "var(--bg-input)",
              border: "1px solid var(--border)",
              color: "var(--text-primary)",
              fontFamily: "JetBrains Mono, monospace",
              fontSize: 12,
              lineHeight: 1.5,
              resize: "vertical",
              clipPath: clipCorner(8),
              outline: "none",
              boxSizing: "border-box",
            }}
          />
        )}

        <div style={{ marginTop: 14 }}>
          <button onClick={handleCreate} style={btnPrimary}>
            Create Instance
          </button>
        </div>
      </div>
    </>
  );

  const renderConfig = () => (
    <>
      {requiresRestart && (
        <div
          style={{
            background: "var(--error-bg)",
            border: "1px solid var(--error-text)",
            clipPath: clipCorner(8),
            padding: "12px 16px",
            marginBottom: 16,
            fontFamily: "Outfit, sans-serif",
            fontSize: 13,
            color: "var(--error-text)",
          }}
        >
          Configuration saved. A gateway restart is required for changes to take effect.
        </div>
      )}
      <div style={labelStyle}>Gateway Configuration (TOML)</div>
      <textarea
        value={configRaw}
        onChange={(e) => {
          setConfigRaw(e.target.value);
          setConfigDirty(true);
        }}
        rows={24}
        style={{
          width: "100%",
          padding: "12px 14px",
          background: "var(--bg-input)",
          border: "1px solid var(--border)",
          color: "var(--text-primary)",
          fontFamily: "JetBrains Mono, monospace",
          fontSize: 12,
          lineHeight: 1.5,
          resize: "vertical",
          clipPath: clipCorner(10),
          outline: "none",
          boxSizing: "border-box",
        }}
      />
      <div style={{ marginTop: 14, display: "flex", gap: 10, alignItems: "center" }}>
        <button onClick={handleSaveConfig} disabled={!configDirty} style={{
          ...btnPrimary,
          opacity: configDirty ? 1 : 0.5,
          cursor: configDirty ? "pointer" : "default",
        }}>
          Save Config
        </button>
        <button onClick={loadConfig} style={btnSecondary}>
          Reload
        </button>
        {configDirty && (
          <span
            style={{
              fontFamily: "JetBrains Mono, monospace",
              fontSize: 10,
              color: "var(--amber)",
              letterSpacing: 1,
            }}
          >
            UNSAVED CHANGES
          </span>
        )}
      </div>
    </>
  );

  return (
    <div style={{ padding: 24, maxWidth: 1100, flex: 1, overflowY: "auto" }}>
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
        Admin Panel
      </h2>

      {/* Tabs */}
      <div
        style={{
          display: "flex",
          gap: 0,
          borderBottom: "1px solid var(--border)",
          marginBottom: 20,
        }}
      >
        <button style={tabBtnStyle(tab === "dashboard")} onClick={() => setTab("dashboard")}>
          Dashboard
        </button>
        <button style={tabBtnStyle(tab === "instances")} onClick={() => setTab("instances")}>
          Instances
        </button>
        <button style={tabBtnStyle(tab === "config")} onClick={() => setTab("config")}>
          Gateway Config
        </button>
      </div>

      {tab === "dashboard" && renderDashboard()}
      {tab === "instances" && renderInstances()}
      {tab === "config" && renderConfig()}
    </div>
  );
}
