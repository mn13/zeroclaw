import { useState, useEffect, useCallback, useRef } from "react";
import {
  getComposio,
  updateComposio,
  listSkills,
  updateSkill,
  deleteSkill,
} from "../api";
import type { ComposioConfig, SkillFile } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

type Tab = "composio" | "skills";

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

// ── Composio Tab ──

function ComposioTab({ instanceId, toast }: Props) {
  const [config, setConfig] = useState<ComposioConfig | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [entityId, setEntityId] = useState("");
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);

  const load = useCallback(() => {
    setLoading(true);
    getComposio(instanceId)
      .then((c) => {
        setConfig(c);
        setEnabled(c.enabled);
        setEntityId(c.entity_id);
        setApiKey("");
        setDirty(false);
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load Composio config", true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    load();
  }, [instanceId, load]);

  const handleSave = useCallback(async () => {
    try {
      const data: { enabled?: boolean; api_key?: string; entity_id?: string } = { enabled, entity_id: entityId };
      if (apiKey) data.api_key = apiKey;
      await updateComposio(instanceId, data);
      toast("Composio config saved");
      setDirty(false);
      setApiKey("");
      load();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
    }
  }, [instanceId, enabled, apiKey, entityId, toast, load]);

  if (loading && !config) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading Composio config...
      </div>
    );
  }

  return (
    <div style={{ padding: 24, maxWidth: 600 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 24 }}>
        <span style={{ fontFamily: "Syne, sans-serif", fontSize: 16, fontWeight: 700, color: "var(--amber)" }}>
          Composio
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ ...labelStyle, marginBottom: 0 }}>{enabled ? "Enabled" : "Disabled"}</span>
          <div
            onClick={() => { setEnabled(!enabled); setDirty(true); }}
            style={{
              width: 40, height: 22, borderRadius: 11,
              background: enabled ? "var(--amber)" : "var(--toggle-off)",
              position: "relative", cursor: "pointer", transition: "background 0.2s",
            }}
          >
            <div
              style={{
                width: 16, height: 16, borderRadius: "50%", background: "#fff",
                position: "absolute", top: 3, left: enabled ? 21 : 3, transition: "left 0.2s",
              }}
            />
          </div>
        </div>
      </div>

      <div style={{ marginBottom: 14 }}>
        <div style={{ ...labelStyle, marginBottom: 4 }}>
          API Key {config?.has_api_key && <span style={{ color: "var(--success)" }}>(set)</span>}
        </div>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => { setApiKey(e.target.value); setDirty(true); }}
          placeholder={config?.has_api_key ? "Leave empty to keep current key" : "Enter Composio API key"}
          style={inputStyle}
        />
      </div>

      <div style={{ marginBottom: 24 }}>
        <div style={{ ...labelStyle, marginBottom: 4 }}>Entity ID</div>
        <input
          value={entityId}
          onChange={(e) => { setEntityId(e.target.value); setDirty(true); }}
          placeholder="default"
          style={inputStyle}
        />
      </div>

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
  );
}

// ── Skills Tab ──

function SkillsTab({ instanceId, toast }: Props) {
  const [skills, setSkills] = useState<SkillFile[]>([]);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [editContent, setEditContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);
  const [showNewForm, setShowNewForm] = useState(false);
  const [newName, setNewName] = useState("");
  const selectedRef = useRef<string | null>(null);
  selectedRef.current = selectedSkill;

  const load = useCallback(() => {
    setLoading(true);
    listSkills(instanceId)
      .then((resp) => {
        setSkills(resp.skills);
        const sel = selectedRef.current;
        if (!sel && resp.skills.length > 0) {
          setSelectedSkill(resp.skills[0]!.name);
          setEditContent(resp.skills[0]!.content);
          setDirty(false);
        } else if (sel) {
          const s = resp.skills.find((sk) => sk.name === sel);
          if (s && !dirty) setEditContent(s.content);
        }
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load skills", true))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId, toast]);

  useEffect(() => {
    setSelectedSkill(null);
    setDirty(false);
    selectedRef.current = null;
    load();
  }, [instanceId, load]);

  const selectSkill = useCallback(
    (name: string) => {
      if (dirty && !confirm("You have unsaved changes. Discard them?")) return;
      setSelectedSkill(name);
      const s = skills.find((sk) => sk.name === name);
      setEditContent(s?.content || "");
      setDirty(false);
    },
    [skills, dirty],
  );

  const handleSave = useCallback(async () => {
    if (!selectedSkill) return;
    try {
      await updateSkill(instanceId, selectedSkill, editContent);
      toast(`Saved ${selectedSkill}`);
      setDirty(false);
      setSkills((prev) => {
        const existing = prev.find((s) => s.name === selectedSkill);
        if (existing) return prev.map((s) => (s.name === selectedSkill ? { ...s, content: editContent } : s));
        return [...prev, { name: selectedSkill, content: editContent }];
      });
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
    }
  }, [instanceId, selectedSkill, editContent, toast]);

  const handleDelete = useCallback(async () => {
    if (!selectedSkill || !confirm(`Delete ${selectedSkill}?`)) return;
    try {
      await deleteSkill(instanceId, selectedSkill);
      toast(`Deleted ${selectedSkill}`);
      setSkills((prev) => prev.filter((s) => s.name !== selectedSkill));
      setSelectedSkill(null);
      setEditContent("");
      setDirty(false);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Delete failed", true);
    }
  }, [instanceId, selectedSkill, toast]);

  const handleCreate = useCallback(
    (filename: string) => {
      let name = filename.trim();
      if (!name) return;
      if (!name.endsWith(".md")) name += ".md";
      if (skills.find((s) => s.name === name)) {
        selectSkill(name);
        return;
      }
      setSelectedSkill(name);
      setEditContent("");
      setDirty(true);
      setShowNewForm(false);
      setNewName("");
    },
    [skills, selectSkill],
  );

  if (loading && skills.length === 0) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading skills...
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
      {/* Skill list */}
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
        <div style={{ padding: "14px 16px 10px", ...labelStyle }}>Skill Files</div>
        <div style={{ flex: 1, overflowY: "auto" }}>
          {skills.map((skill) => {
            const active = selectedSkill === skill.name;
            return (
              <button
                key={skill.name}
                onClick={() => selectSkill(skill.name)}
                style={{
                  display: "block",
                  width: "100%",
                  padding: "10px 16px",
                  background: active ? "var(--amber-glow)" : "transparent",
                  border: "none",
                  borderLeft: `2px solid ${active ? "var(--amber)" : "transparent"}`,
                  cursor: "pointer",
                  transition: "background 0.1s",
                  textAlign: "left",
                  fontFamily: "JetBrains Mono, monospace",
                  fontSize: 12,
                  fontWeight: active ? 600 : 400,
                  color: active ? "var(--amber)" : "var(--text-primary)",
                }}
                onMouseEnter={(e) => {
                  if (!active) e.currentTarget.style.background = "var(--row-hover-bg)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = active ? "var(--amber-glow)" : "transparent";
                }}
              >
                {skill.name}
              </button>
            );
          })}
        </div>

        <div style={{ padding: "10px 12px", borderTop: "1px solid var(--border)" }}>
          {showNewForm ? (
            <div style={{ display: "flex", gap: 6 }}>
              <input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="skill-name.md"
                onKeyDown={(e) => e.key === "Enter" && handleCreate(newName)}
                style={{
                  flex: 1,
                  padding: "6px 8px",
                  background: "var(--bg-input)",
                  border: "1px solid var(--border)",
                  color: "var(--text-primary)",
                  fontFamily: "JetBrains Mono, monospace",
                  fontSize: 11,
                  outline: "none",
                  clipPath: clipCorner(4),
                }}
                autoFocus
              />
              <button
                onClick={() => handleCreate(newName)}
                style={{ ...btnPrimary, padding: "4px 8px", fontSize: 10 }}
              >
                Add
              </button>
            </div>
          ) : (
            <button
              onClick={() => setShowNewForm(true)}
              style={{ ...btnSecondary, padding: "6px 12px", fontSize: 10, width: "100%" }}
            >
              + New Skill
            </button>
          )}
        </div>
      </div>

      {/* Editor area */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {selectedSkill ? (
          <>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "10px 20px",
                borderBottom: "1px solid var(--border)",
                minHeight: 44,
              }}
            >
              <div>
                <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 14, fontWeight: 600, color: "var(--amber)" }}>
                  {selectedSkill}
                </span>
                {dirty && <span style={{ ...labelStyle, marginLeft: 12, color: "var(--amber)" }}>UNSAVED</span>}
              </div>
              <div style={{ display: "flex", gap: 8 }}>
                <button
                  onClick={handleSave}
                  disabled={!dirty}
                  style={{
                    ...btnPrimary,
                    padding: "6px 14px",
                    fontSize: 10,
                    opacity: dirty ? 1 : 0.4,
                    cursor: dirty ? "pointer" : "default",
                  }}
                >
                  Save
                </button>
                {skills.find((s) => s.name === selectedSkill) && (
                  <button onClick={handleDelete} style={{ ...btnDanger, padding: "6px 14px", fontSize: 10 }}>
                    Delete
                  </button>
                )}
              </div>
            </div>

            <textarea
              value={editContent}
              onChange={(e) => { setEditContent(e.target.value); setDirty(true); }}
              placeholder="Write skill content here..."
              style={{
                flex: 1,
                padding: "16px 20px",
                background: "var(--bg-deep)",
                border: "none",
                color: "var(--text-primary)",
                fontFamily: "JetBrains Mono, monospace",
                fontSize: 13,
                lineHeight: 1.6,
                resize: "none",
                outline: "none",
                boxSizing: "border-box",
              }}
            />
          </>
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
            <span style={{ fontSize: 28, opacity: 0.4 }}>{"\u2A01"}</span>
            <span style={{ fontSize: 13, letterSpacing: 1 }}>Select or create a skill file</span>
            <span style={{ fontSize: 11, maxWidth: 400, textAlign: "center", lineHeight: 1.5 }}>
              Skills are markdown files that define reusable capabilities and instructions the agent can use.
            </span>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Main Integrations Page ──

export default function Integrations({ instanceId, toast }: Props) {
  const [tab, setTab] = useState<Tab>("composio");

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
        <button style={tabBtnStyle(tab === "composio")} onClick={() => setTab("composio")}>
          Composio
        </button>
        <button style={tabBtnStyle(tab === "skills")} onClick={() => setTab("skills")}>
          Skills
        </button>
      </div>

      {tab === "composio" && <ComposioTab key={instanceId} instanceId={instanceId} toast={toast} />}
      {tab === "skills" && <SkillsTab key={instanceId} instanceId={instanceId} toast={toast} />}
    </div>
  );
}
