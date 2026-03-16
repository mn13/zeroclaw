import { useState, useEffect, useCallback, useRef } from "react";
import { listIdentity, updateIdentityFile, deleteIdentityFile, clearHistory } from "../api";
import type { IdentityFile } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

const FILE_DESCRIPTIONS: Record<string, string> = {
  "SOUL.md": "Core personality, values, and behavioral guidelines",
  "IDENTITY.md": "Name, role, background, and self-description",
  "AGENTS.md": "Multi-agent collaboration rules and delegation",
  "TOOLS.md": "Tool usage preferences and restrictions",
  "USER.md": "Information about the user this agent serves",
  "HEARTBEAT.md": "Scheduled check-in and autonomous task instructions",
  "BOOTSTRAP.md": "First-run initialization instructions",
  "MEMORY.md": "Memory management and recall guidelines",
};

export default function Identity({ instanceId, toast }: Props) {
  const [files, setFiles] = useState<IdentityFile[]>([]);
  const [knownFiles, setKnownFiles] = useState<string[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [editContent, setEditContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);
  const [newFilename, setNewFilename] = useState("");
  const [showNewForm, setShowNewForm] = useState(false);
  const [needsApply, setNeedsApply] = useState(false);
  const selectedFileRef = useRef<string | null>(null);
  selectedFileRef.current = selectedFile;

  const load = useCallback(() => {
    setLoading(true);
    listIdentity(instanceId)
      .then((resp) => {
        setFiles(resp.files);
        setKnownFiles(resp.known_files);
        const sel = selectedFileRef.current;
        if (!sel && resp.files.length > 0) {
          setSelectedFile(resp.files[0]!.filename);
          setEditContent(resp.files[0]!.content);
          setDirty(false);
        } else if (sel) {
          const f = resp.files.find((f) => f.filename === sel);
          if (f && !dirty) {
            setEditContent(f.content);
          }
        }
      })
      .catch((err) => toast(err instanceof Error ? err.message : "Failed to load identity files", true))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId, toast]);

  useEffect(() => {
    setSelectedFile(null);
    setDirty(false);
    setNeedsApply(false);
    selectedFileRef.current = null;
    load();
  }, [instanceId, load]);

  const selectFile = useCallback(
    (filename: string) => {
      if (dirty) {
        if (!confirm("You have unsaved changes. Discard them?")) return;
      }
      setSelectedFile(filename);
      const f = files.find((f) => f.filename === filename);
      setEditContent(f?.content || "");
      setDirty(false);
    },
    [files, dirty],
  );

  const handleSave = useCallback(async () => {
    if (!selectedFile) return;
    try {
      await updateIdentityFile(instanceId, selectedFile, editContent);
      toast(`Saved ${selectedFile}`);
      setDirty(false);
      setNeedsApply(true);
      setFiles((prev) => {
        const existing = prev.find((f) => f.filename === selectedFile);
        if (existing) {
          return prev.map((f) => (f.filename === selectedFile ? { ...f, content: editContent } : f));
        }
        return [...prev, { filename: selectedFile, content: editContent }];
      });
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Save failed", true);
    }
  }, [instanceId, selectedFile, editContent, toast]);

  const handleApply = useCallback(async () => {
    try {
      await clearHistory(instanceId);
      toast("History cleared — identity changes will take effect on next message");
      setNeedsApply(false);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Failed to clear history", true);
    }
  }, [instanceId, toast]);

  const handleDelete = useCallback(async () => {
    if (!selectedFile) return;
    if (!confirm(`Delete ${selectedFile}?`)) return;
    try {
      await deleteIdentityFile(instanceId, selectedFile);
      toast(`Deleted ${selectedFile}`);
      setFiles((prev) => prev.filter((f) => f.filename !== selectedFile));
      setSelectedFile(null);
      setEditContent("");
      setDirty(false);
      setNeedsApply(true);
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Delete failed", true);
    }
  }, [instanceId, selectedFile, toast]);

  const handleCreateFile = useCallback(
    (filename: string) => {
      let name = filename.trim();
      if (!name) return;
      if (!name.endsWith(".md")) name += ".md";
      if (files.find((f) => f.filename === name)) {
        selectFile(name);
        return;
      }
      setSelectedFile(name);
      setEditContent("");
      setDirty(true);
      setShowNewForm(false);
      setNewFilename("");
    },
    [files, selectFile],
  );

  // Files that exist + known files that don't exist yet (as placeholders)
  const existingNames = new Set(files.map((f) => f.filename));
  const allSlots = [
    ...files,
    ...knownFiles
      .filter((name) => !existingNames.has(name))
      .map((name) => ({ filename: name, content: "" })),
  ];

  // Sort: known files first in order, then custom alphabetically
  allSlots.sort((a, b) => {
    const ai = knownFiles.indexOf(a.filename);
    const bi = knownFiles.indexOf(b.filename);
    if (ai >= 0 && bi >= 0) return ai - bi;
    if (ai >= 0) return -1;
    if (bi >= 0) return 1;
    return a.filename.localeCompare(b.filename);
  });

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

  if (loading && files.length === 0) {
    return (
      <div style={{ padding: 32, color: "var(--text-dim)", fontFamily: "Outfit, sans-serif", fontSize: 14 }}>
        Loading identity files...
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
      {/* File list sidebar */}
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
        <div
          style={{
            padding: "14px 16px 10px",
            ...labelStyle,
          }}
        >
          Identity Files
        </div>

        <div style={{ flex: 1, overflowY: "auto" }}>
          {allSlots.map((slot) => {
            const exists = existingNames.has(slot.filename);
            const active = selectedFile === slot.filename;
            const isKnown = knownFiles.includes(slot.filename);
            return (
              <button
                key={slot.filename}
                onClick={() => (exists ? selectFile(slot.filename) : handleCreateFile(slot.filename))}
                style={{
                  display: "flex",
                  flexDirection: "column",
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
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 12,
                    fontWeight: active ? 600 : 400,
                    color: active ? "var(--amber)" : exists ? "var(--text-primary)" : "var(--text-dim)",
                  }}
                >
                  {slot.filename}
                </span>
                {isKnown && FILE_DESCRIPTIONS[slot.filename] && (
                  <span
                    style={{
                      fontFamily: "Outfit, sans-serif",
                      fontSize: 10,
                      color: "var(--text-dim)",
                      marginTop: 2,
                      lineHeight: 1.3,
                    }}
                  >
                    {FILE_DESCRIPTIONS[slot.filename]}
                  </span>
                )}
                {!exists && (
                  <span
                    style={{
                      fontFamily: "JetBrains Mono, monospace",
                      fontSize: 9,
                      color: "var(--text-dim)",
                      marginTop: 2,
                      letterSpacing: 0.5,
                    }}
                  >
                    + click to create
                  </span>
                )}
              </button>
            );
          })}
        </div>

        {/* New file button */}
        <div style={{ padding: "10px 12px", borderTop: "1px solid var(--border)" }}>
          {showNewForm ? (
            <div style={{ display: "flex", gap: 6 }}>
              <input
                value={newFilename}
                onChange={(e) => setNewFilename(e.target.value)}
                placeholder="CUSTOM.md"
                onKeyDown={(e) => e.key === "Enter" && handleCreateFile(newFilename)}
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
                onClick={() => handleCreateFile(newFilename)}
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
              + New File
            </button>
          )}
        </div>
      </div>

      {/* Editor area */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {/* Apply banner */}
        {needsApply && (
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
            <span
              style={{
                fontFamily: "Outfit, sans-serif",
                fontSize: 12,
                color: "var(--amber)",
              }}
            >
              Identity files saved. Clear history to apply changes to the agent.
            </span>
            <button onClick={handleApply} style={{ ...btnPrimary, padding: "5px 12px", fontSize: 10 }}>
              Apply Now
            </button>
          </div>
        )}

        {selectedFile ? (
          <>
            {/* File header */}
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
                <span
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 14,
                    fontWeight: 600,
                    color: "var(--amber)",
                  }}
                >
                  {selectedFile}
                </span>
                {dirty && (
                  <span
                    style={{
                      ...labelStyle,
                      marginLeft: 12,
                      color: "var(--amber)",
                    }}
                  >
                    UNSAVED
                  </span>
                )}
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
                {existingNames.has(selectedFile) && (
                  <button onClick={handleDelete} style={{ ...btnDanger, padding: "6px 14px", fontSize: 10 }}>
                    Delete
                  </button>
                )}
              </div>
            </div>

            {/* Description hint */}
            {FILE_DESCRIPTIONS[selectedFile] && (
              <div
                style={{
                  padding: "8px 20px",
                  fontFamily: "Outfit, sans-serif",
                  fontSize: 12,
                  color: "var(--text-dim)",
                  borderBottom: "1px solid var(--border)",
                  background: "var(--bg-card)",
                }}
              >
                {FILE_DESCRIPTIONS[selectedFile]}
              </div>
            )}

            {/* Textarea editor */}
            <textarea
              value={editContent}
              onChange={(e) => {
                setEditContent(e.target.value);
                setDirty(true);
              }}
              placeholder={`Write the ${selectedFile} content here...\n\nThis markdown file is injected into the agent's system prompt and shapes its behavior.`}
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
            <span style={{ fontSize: 28, opacity: 0.4 }}>{"\u2662"}</span>
            <span style={{ fontSize: 13, letterSpacing: 1 }}>
              Select a file to edit the agent's identity
            </span>
            <span style={{ fontSize: 11, color: "var(--text-dim)", maxWidth: 400, textAlign: "center", lineHeight: 1.5 }}>
              Identity files are markdown documents injected into the agent's system prompt.
              They define personality, behavior, and knowledge.
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
