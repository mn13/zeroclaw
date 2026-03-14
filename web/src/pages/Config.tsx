import { useState, useEffect, useCallback, useMemo } from "react";
import { getConfig, updateConfig } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

type ConfigValue = unknown;
type ConfigData = Record<string, ConfigValue>;
type SaveStatus = null | "saving" | "deployed" | "verifying" | "confirmed" | "error";

function deepClone<T>(obj: T): T {
  return JSON.parse(JSON.stringify(obj));
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

export default function Config({ instanceId, toast }: Props) {
  const [savedConfig, setSavedConfig] = useState<ConfigData | null>(null);
  const [draft, setDraft] = useState<ConfigData | null>(null);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>(null);

  const dirty = useMemo(() => {
    if (!draft || !savedConfig) return false;
    return JSON.stringify(draft) !== JSON.stringify(savedConfig);
  }, [draft, savedConfig]);

  const loadConfig = useCallback(() => {
    setLoading(true);
    getConfig(instanceId)
      .then((data) => {
        const cloned = deepClone(data);
        setSavedConfig(cloned);
        setDraft(deepClone(cloned));
      })
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const toggleSection = useCallback((key: string) => {
    setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));
  }, []);

  const updateField = useCallback(
    (path: string[], value: ConfigValue) => {
      if (!draft) return;
      const next = deepClone(draft);
      let cursor: Record<string, ConfigValue> = next;
      for (let i = 0; i < path.length - 1; i++) {
        const key = path[i]!;
        cursor = cursor[key] as Record<string, ConfigValue>;
      }
      const lastKey = path[path.length - 1]!;
      cursor[lastKey] = value;
      setDraft(next);
    },
    [draft],
  );

  const handleSave = useCallback(async () => {
    if (!draft || saving) return;
    setSaving(true);
    setSaveStatus("saving");
    try {
      const result = await updateConfig(instanceId, draft);
      const msg =
        `Updated ${result.updated_fields.length} field(s)` +
        (result.requires_restart ? " — restart required" : "");
      toast(msg);
      setSavedConfig(deepClone(draft));
      setSaveStatus("deployed");

      // Verify by reloading config
      setSaveStatus("verifying");
      const reloaded = await getConfig(instanceId);
      const cloned = deepClone(reloaded);
      setSavedConfig(cloned);
      setDraft(deepClone(cloned));
      setSaveStatus("confirmed");

      setTimeout(() => {
        setSaveStatus(null);
      }, 3000);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
      setSaveStatus("error");
      setTimeout(() => {
        setSaveStatus(null);
      }, 3000);
    } finally {
      setSaving(false);
    }
  }, [instanceId, draft, saving, toast]);

  const renderField = (
    key: string,
    value: ConfigValue,
    path: string[],
    depth: number,
  ): React.ReactNode => {
    const fullPath = [...path, key];
    const labelStyle: React.CSSProperties = {
      fontFamily: "Outfit, sans-serif",
      fontSize: 13,
      color: "var(--text-dim)",
      minWidth: 180,
      flexShrink: 0,
    };
    const rowStyle: React.CSSProperties = {
      display: "flex",
      alignItems: "center",
      gap: 12,
      padding: "6px 0",
      marginLeft: depth * 16,
    };
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

    if (typeof value === "boolean") {
      return (
        <div key={key} style={rowStyle}>
          <span style={labelStyle}>{key}</span>
          <div
            onClick={() => updateField(fullPath, !value)}
            style={{
              width: 40,
              height: 22,
              borderRadius: 11,
              background: value ? "var(--amber)" : "var(--toggle-off)",
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
                left: value ? 21 : 3,
                transition: "left 0.2s",
              }}
            />
          </div>
        </div>
      );
    }

    if (typeof value === "number") {
      return (
        <div key={key} style={rowStyle}>
          <span style={labelStyle}>{key}</span>
          <input
            type="number"
            value={value}
            onChange={(e) => updateField(fullPath, Number(e.target.value))}
            style={{ ...inputStyle, maxWidth: 120 }}
          />
        </div>
      );
    }

    if (Array.isArray(value)) {
      const hasObjects = value.some(
        (v) => typeof v === "object" && v !== null,
      );
      if (hasObjects) {
        return (
          <div key={key} style={{ marginLeft: depth * 16, marginTop: 4 }}>
            <span style={labelStyle}>{key}</span>
            <textarea
              value={JSON.stringify(value, null, 2)}
              onChange={(e) => {
                try {
                  updateField(fullPath, JSON.parse(e.target.value));
                } catch {
                  /* ignore invalid JSON while typing */
                }
              }}
              style={{
                ...inputStyle,
                marginTop: 4,
                minHeight: 80,
                resize: "vertical",
                display: "block",
                width: "100%",
              }}
            />
          </div>
        );
      }
      return (
        <div key={key} style={rowStyle}>
          <span style={labelStyle}>{key}</span>
          <input
            type="text"
            value={value.join(", ")}
            onChange={(e) =>
              updateField(
                fullPath,
                e.target.value.split(",").map((s) => s.trim()),
              )
            }
            style={inputStyle}
          />
        </div>
      );
    }

    if (typeof value === "object" && value !== null) {
      const obj = value as Record<string, ConfigValue>;
      return (
        <div key={key} style={{ marginLeft: depth * 16, marginTop: 4 }}>
          <div
            style={{
              fontFamily: "Outfit, sans-serif",
              fontSize: 13,
              color: "var(--text-dim)",
              fontWeight: 600,
              marginBottom: 4,
            }}
          >
            {key}
          </div>
          {Object.entries(obj).map(([k, v]) =>
            renderField(k, v, fullPath, depth + 1),
          )}
        </div>
      );
    }

    // string or fallback
    const strVal = value == null ? "" : String(value);
    return (
      <div key={key} style={rowStyle}>
        <span style={labelStyle}>{key}</span>
        <input
          type={isSensitiveKey(key) ? "password" : "text"}
          value={strVal}
          onChange={(e) => updateField(fullPath, e.target.value)}
          style={inputStyle}
        />
      </div>
    );
  };

  if (loading || !draft) {
    return (
      <div
        style={{
          padding: 32,
          color: "var(--text-dim)",
          fontFamily: "Outfit, sans-serif",
          fontSize: 14,
        }}
      >
        Loading configuration...
      </div>
    );
  }

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
        Configuration
      </h2>

      {Object.entries(draft).map(([sectionKey, sectionValue]) => {
        const isObject =
          typeof sectionValue === "object" &&
          sectionValue !== null &&
          !Array.isArray(sectionValue);
        const isCollapsed = collapsed[sectionKey] ?? false;

        return (
          <div
            key={sectionKey}
            style={{
              background: "var(--bg-card)",
              border: "1px solid var(--border)",
              clipPath: clipCorner(10),
              marginBottom: 12,
              overflow: "hidden",
            }}
          >
            <div
              onClick={() => toggleSection(sectionKey)}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "10px 16px",
                cursor: "pointer",
                userSelect: "none",
              }}
            >
              <span
                style={{
                  fontFamily: "Syne, sans-serif",
                  fontSize: 13,
                  fontWeight: 600,
                  textTransform: "uppercase",
                  color: "var(--text-primary)",
                  letterSpacing: 0.8,
                }}
              >
                {sectionKey}
              </span>
              <span
                style={{
                  color: "var(--text-dim)",
                  fontSize: 12,
                  transition: "transform 0.2s",
                  transform: isCollapsed ? "rotate(-90deg)" : "rotate(0deg)",
                }}
              >
                ▼
              </span>
            </div>

            {!isCollapsed && (
              <div style={{ padding: "4px 16px 12px" }}>
                {isObject
                  ? Object.entries(
                      sectionValue as Record<string, ConfigValue>,
                    ).map(([k, v]) => renderField(k, v, [sectionKey], 0))
                  : renderField(sectionKey, sectionValue, [], 0)}
              </div>
            )}
          </div>
        );
      })}

      <div
        style={{
          display: "flex",
          gap: 12,
          marginTop: 20,
          justifyContent: "flex-end",
          alignItems: "center",
        }}
      >
        {saveStatus && (
          <span
            style={{
              fontFamily: "JetBrains Mono, monospace",
              fontSize: 11,
              fontWeight: 700,
              letterSpacing: 1,
              textTransform: "uppercase",
              color:
                saveStatus === "error"
                  ? "#ef4444"
                  : saveStatus === "saving" || saveStatus === "verifying"
                    ? "#f59e0b"
                    : "#22c55e",
              animation:
                saveStatus === "saving"
                  ? "pulse 1.5s ease-in-out infinite"
                  : undefined,
            }}
          >
            {saveStatus === "saving" && "PUSHING TO AGENT..."}
            {saveStatus === "deployed" && "DEPLOYED"}
            {saveStatus === "verifying" && "VERIFYING..."}
            {saveStatus === "confirmed" && "CONFIRMED"}
            {saveStatus === "error" && "FAILED"}
          </span>
        )}
        <button
          onClick={loadConfig}
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
          disabled={!dirty || saving}
          style={{
            fontFamily: "JetBrains Mono, monospace",
            fontSize: 12,
            fontWeight: 600,
            textTransform: "uppercase",
            letterSpacing: 1,
            padding: "8px 20px",
            background: dirty && !saving ? "var(--amber)" : "var(--bg-card)",
            border: `1px solid ${dirty && !saving ? "var(--amber)" : "var(--border)"}`,
            color: dirty && !saving ? "#000" : "var(--text-dim)",
            cursor: dirty && !saving ? "pointer" : "not-allowed",
            clipPath: clipCorner(6),
            opacity: dirty && !saving ? 1 : 0.5,
            transition: "opacity 0.2s, background 0.2s, color 0.2s",
          }}
        >
          Save Changes
        </button>
      </div>
      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.4; }
        }
      `}</style>
    </div>
  );
}
