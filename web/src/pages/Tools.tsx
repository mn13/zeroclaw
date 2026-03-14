import { useState, useEffect, useCallback, useMemo } from "react";
import { listTools, getConfig, updateConfig } from "../api";
import type { ToolInfo } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

interface AutonomyFields {
  allowed_commands: string[];
  auto_approve: string[];
  always_ask: string[];
  forbidden_paths: string[];
  non_cli_excluded_tools: string[];
}

const FIELD_META: {
  key: keyof AutonomyFields;
  label: string;
  description: string;
}[] = [
  {
    key: "allowed_commands",
    label: "Shell Whitelist",
    description: "Commands the agent can run",
  },
  {
    key: "auto_approve",
    label: "Auto-Approve",
    description: "Tools that run without confirmation",
  },
  {
    key: "always_ask",
    label: "Always Ask",
    description: "Tools that always require confirmation",
  },
  {
    key: "forbidden_paths",
    label: "Forbidden Paths",
    description: "Paths the agent cannot access",
  },
  {
    key: "non_cli_excluded_tools",
    label: "Non-CLI Excluded",
    description: "Tools excluded from non-CLI channels",
  },
];

const EMPTY_AUTONOMY: AutonomyFields = {
  allowed_commands: [],
  auto_approve: [],
  always_ask: [],
  forbidden_paths: [],
  non_cli_excluded_tools: [],
};

function deepClone<T>(obj: T): T {
  return JSON.parse(JSON.stringify(obj));
}

function extractAutonomy(config: Record<string, unknown>): AutonomyFields {
  const autonomy = (config.autonomy ?? {}) as Record<string, unknown>;
  const result = deepClone(EMPTY_AUTONOMY);
  for (const field of FIELD_META) {
    const val = autonomy[field.key];
    if (Array.isArray(val)) {
      result[field.key] = val.map(String);
    }
  }
  return result;
}

type SaveStatus = null | "saving" | "saved" | "error";

export default function Tools({ instanceId, toast }: Props) {
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(true);

  const [savedAutonomy, setSavedAutonomy] = useState<AutonomyFields>(
    deepClone(EMPTY_AUTONOMY),
  );
  const [draft, setDraft] = useState<AutonomyFields>(
    deepClone(EMPTY_AUTONOMY),
  );
  const [inputValues, setInputValues] = useState<
    Record<keyof AutonomyFields, string>
  >({
    allowed_commands: "",
    auto_approve: "",
    always_ask: "",
    forbidden_paths: "",
    non_cli_excluded_tools: "",
  });
  const [saveStatus, setSaveStatus] = useState<SaveStatus>(null);

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(savedAutonomy),
    [draft, savedAutonomy],
  );

  const load = useCallback(() => {
    setLoading(true);
    Promise.all([
      listTools(instanceId).catch(() => ({ tools: [] as ToolInfo[] })),
      getConfig(instanceId).catch(() => ({} as Record<string, unknown>)),
    ])
      .then(([toolsRes, configRes]) => {
        setTools(toolsRes.tools);
        const autonomy = extractAutonomy(configRes);
        setSavedAutonomy(deepClone(autonomy));
        setDraft(deepClone(autonomy));
      })
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    load();
  }, [load]);

  const toggleParams = useCallback((name: string) => {
    setExpanded((prev) => ({ ...prev, [name]: !prev[name] }));
  }, []);

  const addTag = useCallback(
    (field: keyof AutonomyFields, value: string) => {
      const trimmed = value.trim();
      if (!trimmed) return;
      if (draft[field].includes(trimmed)) return;
      setDraft((prev) => ({
        ...prev,
        [field]: [...prev[field], trimmed],
      }));
    },
    [draft],
  );

  const removeTag = useCallback(
    (field: keyof AutonomyFields, index: number) => {
      setDraft((prev) => ({
        ...prev,
        [field]: prev[field].filter((_, i) => i !== index),
      }));
    },
    [],
  );

  const handleSave = useCallback(async () => {
    setSaveStatus("saving");
    try {
      const payload: Record<string, unknown> = {
        autonomy: { ...draft },
      };
      const result = await updateConfig(instanceId, payload);
      setSavedAutonomy(deepClone(draft));
      setSaveStatus("saved");
      toast(
        `Updated ${result.updated_fields.length} field(s)` +
          (result.requires_restart ? " — restart required" : ""),
      );
      setTimeout(() => setSaveStatus(null), 3000);
    } catch (err: unknown) {
      setSaveStatus("error");
      toast(err instanceof Error ? err.message : "Save failed", true);
      setTimeout(() => setSaveStatus(null), 3000);
    }
  }, [instanceId, draft, toast]);

  if (loading) {
    return (
      <div
        style={{
          padding: 32,
          color: "var(--text-dim)",
          fontFamily: "Outfit, sans-serif",
          fontSize: 14,
        }}
      >
        Loading tools...
      </div>
    );
  }

  return (
    <div style={{ padding: 24, maxWidth: 900, flex: 1, overflowY: "auto" }}>
      {/* ── REGISTERED TOOLS ── */}
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
        Registered Tools
      </h2>

      {tools.length === 0 ? (
        <div
          style={{
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            padding: 24,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 10,
            marginBottom: 32,
          }}
        >
          <div
            style={{
              fontSize: 28,
              color: "var(--text-dim)",
              fontFamily: "JetBrains Mono, monospace",
            }}
          >
            {"{ }"}
          </div>
          <div
            style={{
              fontFamily: "Outfit, sans-serif",
              fontSize: 13,
              color: "var(--text-dim)",
              textAlign: "center",
            }}
          >
            No tools reported by agent — tools are dynamically loaded at runtime
          </div>
        </div>
      ) : (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 10,
            marginBottom: 32,
          }}
        >
          {tools.map((tool) => {
            const isExpanded = expanded[tool.name] ?? false;
            return (
              <div
                key={tool.name}
                style={{
                  background: "var(--bg-card)",
                  border: "1px solid var(--border)",
                  clipPath: clipCorner(10),
                  padding: 16,
                  transition: "border-color 0.2s",
                }}
                onMouseEnter={(e) =>
                  (e.currentTarget.style.borderColor = "var(--border-amber)")
                }
                onMouseLeave={(e) =>
                  (e.currentTarget.style.borderColor = "var(--border)")
                }
              >
                <div
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 14,
                    fontWeight: 600,
                    color: "var(--amber)",
                    marginBottom: 4,
                  }}
                >
                  {tool.name}
                </div>
                <div
                  style={{
                    fontFamily: "Outfit, sans-serif",
                    fontSize: 13,
                    color: "var(--text-dim)",
                    marginBottom: 8,
                  }}
                >
                  {tool.description}
                </div>
                {tool.parameters &&
                  Object.keys(tool.parameters).length > 0 && (
                    <>
                      <div
                        onClick={() => toggleParams(tool.name)}
                        style={{
                          fontFamily: "JetBrains Mono, monospace",
                          fontSize: 11,
                          color: "var(--text-dim)",
                          cursor: "pointer",
                          userSelect: "none",
                          marginBottom: isExpanded ? 8 : 0,
                        }}
                      >
                        <span
                          style={{
                            display: "inline-block",
                            transition: "transform 0.2s",
                            transform: isExpanded
                              ? "rotate(90deg)"
                              : "rotate(0deg)",
                            marginRight: 6,
                          }}
                        >
                          ▸
                        </span>
                        parameters
                      </div>
                      {isExpanded && (
                        <div
                          style={{
                            background: "var(--bg-input)",
                            fontFamily: "JetBrains Mono, monospace",
                            fontSize: 10,
                            color: "var(--text-primary)",
                            padding: 12,
                            whiteSpace: "pre-wrap",
                            wordBreak: "break-word",
                            clipPath: clipCorner(6),
                          }}
                        >
                          {JSON.stringify(tool.parameters, null, 2)}
                        </div>
                      )}
                    </>
                  )}
              </div>
            );
          })}
        </div>
      )}

      {/* ── TOOL CONFIGURATION ── */}
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
        Tool Configuration
      </h2>

      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {FIELD_META.map(({ key, label, description }) => (
          <div
            key={key}
            style={{
              background: "var(--bg-card)",
              border: "1px solid var(--border)",
              clipPath: clipCorner(10),
              padding: 16,
            }}
          >
            <div
              style={{
                fontFamily: "Syne, sans-serif",
                fontSize: 13,
                fontWeight: 600,
                textTransform: "uppercase",
                color: "var(--text-primary)",
                letterSpacing: 0.8,
                marginBottom: 2,
              }}
            >
              {label}
            </div>
            <div
              style={{
                fontFamily: "Outfit, sans-serif",
                fontSize: 12,
                color: "var(--text-dim)",
                marginBottom: 10,
              }}
            >
              {description}
            </div>

            {/* Tags */}
            <div
              style={{
                display: "flex",
                flexWrap: "wrap",
                gap: 6,
                marginBottom: draft[key].length > 0 ? 10 : 0,
              }}
            >
              {draft[key].map((value, idx) => (
                <span
                  key={`${value}-${idx}`}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 12,
                    background: "var(--bg-input)",
                    border: "1px solid var(--border)",
                    color: "var(--text-primary)",
                    padding: "3px 8px",
                    clipPath: clipCorner(4),
                  }}
                >
                  {value}
                  <span
                    onClick={() => removeTag(key, idx)}
                    style={{
                      cursor: "pointer",
                      color: "var(--text-dim)",
                      fontFamily: "sans-serif",
                      fontSize: 14,
                      lineHeight: 1,
                      marginLeft: 2,
                    }}
                    onMouseEnter={(e) =>
                      (e.currentTarget.style.color = "var(--amber)")
                    }
                    onMouseLeave={(e) =>
                      (e.currentTarget.style.color = "var(--text-dim)")
                    }
                  >
                    ×
                  </span>
                </span>
              ))}
            </div>

            {/* Input */}
            <input
              type="text"
              placeholder={`Add ${label.toLowerCase()}... (press Enter)`}
              value={inputValues[key]}
              onChange={(e) =>
                setInputValues((prev) => ({ ...prev, [key]: e.target.value }))
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addTag(key, inputValues[key]);
                  setInputValues((prev) => ({ ...prev, [key]: "" }));
                }
              }}
              style={{
                fontFamily: "JetBrains Mono, monospace",
                fontSize: 13,
                background: "var(--bg-input)",
                border: "1px solid var(--border)",
                color: "var(--text-primary)",
                padding: "6px 10px",
                borderRadius: 0,
                clipPath: clipCorner(6),
                outline: "none",
                width: "100%",
                boxSizing: "border-box",
              }}
            />
          </div>
        ))}
      </div>

      {/* ── Save / Reload ── */}
      <div
        style={{
          display: "flex",
          gap: 12,
          marginTop: 20,
          justifyContent: "flex-end",
          alignItems: "center",
        }}
      >
        {saveStatus === "saved" && (
          <span
            style={{
              fontFamily: "Outfit, sans-serif",
              fontSize: 12,
              color: "var(--amber)",
            }}
          >
            Changes saved
          </span>
        )}
        {saveStatus === "error" && (
          <span
            style={{
              fontFamily: "Outfit, sans-serif",
              fontSize: 12,
              color: "#f44",
            }}
          >
            Save failed
          </span>
        )}
        <button
          onClick={load}
          style={{
            fontFamily: "JetBrains Mono, monospace",
            fontSize: 12,
            fontWeight: 600,
            textTransform: "uppercase",
            letterSpacing: 1,
            padding: "8px 20px",
            background: "transparent",
            border: "1px solid var(--border)",
            color: "var(--text-primary)",
            cursor: "pointer",
            clipPath: clipCorner(6),
          }}
        >
          Reload
        </button>
        <button
          onClick={handleSave}
          disabled={!dirty || saveStatus === "saving"}
          style={{
            fontFamily: "JetBrains Mono, monospace",
            fontSize: 12,
            fontWeight: 600,
            textTransform: "uppercase",
            letterSpacing: 1,
            padding: "8px 20px",
            background: dirty ? "var(--amber)" : "var(--bg-input)",
            border: `1px solid ${dirty ? "var(--amber)" : "var(--border)"}`,
            color: dirty ? "#000" : "var(--text-dim)",
            cursor: dirty ? "pointer" : "default",
            clipPath: clipCorner(6),
            opacity: saveStatus === "saving" ? 0.6 : 1,
          }}
        >
          {saveStatus === "saving" ? "Saving..." : "Save Changes"}
        </button>
      </div>
    </div>
  );
}
