import { useState, useEffect, useCallback } from "react";
import {
  listMemory,
  searchMemory,
  storeMemory,
  forgetMemory,
} from "../api";
import type { MemoryEntry } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

export default function Memory({ instanceId, toast }: Props) {
  const [entries, setEntries] = useState<MemoryEntry[]>([]);
  const [selected, setSelected] = useState<MemoryEntry | null>(null);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [showStore, setShowStore] = useState(false);

  // Store form
  const [formKey, setFormKey] = useState("");
  const [formCategory, setFormCategory] = useState("");
  const [formContent, setFormContent] = useState("");

  const fetchAll = useCallback(() => {
    setLoading(true);
    listMemory(instanceId)
      .then((res) => {
        setEntries(res.entries);
        setSelected(null);
      })
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    fetchAll();
  }, [fetchAll]);

  const handleSearch = useCallback(() => {
    if (!query.trim()) {
      fetchAll();
      return;
    }
    setLoading(true);
    searchMemory(instanceId, query.trim())
      .then((res) => {
        setEntries(res.entries);
        setSelected(null);
      })
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
  }, [instanceId, query, fetchAll, toast]);

  const handleStore = useCallback(async () => {
    if (!formKey.trim() || !formContent.trim()) {
      toast("Key and content are required", true);
      return;
    }
    try {
      await storeMemory(
        instanceId,
        formKey.trim(),
        formContent.trim(),
        formCategory.trim() || "general",
      );
      toast("Memory stored");
      setShowStore(false);
      setFormKey("");
      setFormCategory("");
      setFormContent("");
      fetchAll();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Store failed", true);
    }
  }, [instanceId, formKey, formContent, formCategory, fetchAll, toast]);

  const handleDelete = useCallback(
    async (key: string) => {
      try {
        await forgetMemory(instanceId, key);
        toast("Memory deleted");
        setSelected(null);
        fetchAll();
      } catch (err: unknown) {
        toast(err instanceof Error ? err.message : "Delete failed", true);
      }
    },
    [instanceId, fetchAll, toast],
  );

  const handleCopy = useCallback(
    (content: string) => {
      navigator.clipboard.writeText(content).then(
        () => toast("Copied to clipboard"),
        () => toast("Copy failed", true),
      );
    },
    [toast],
  );

  const inputStyle: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 13,
    background: "var(--bg-input)",
    border: "1px solid var(--border)",
    color: "var(--text-primary)",
    padding: "6px 10px",
    clipPath: clipCorner(6),
    outline: "none",
    flex: 1,
  };

  const btnSecondary: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 11,
    fontWeight: 600,
    textTransform: "uppercase",
    letterSpacing: 1,
    padding: "7px 16px",
    background: "transparent",
    border: "1px solid var(--border)",
    color: "var(--text-primary)",
    cursor: "pointer",
    clipPath: clipCorner(6),
  };

  const btnPrimary: React.CSSProperties = {
    ...btnSecondary,
    background: "var(--amber)",
    border: "1px solid var(--amber)",
    color: "#000",
  };

  return (
    <div style={{ padding: 24, maxWidth: 1000, flex: 1, overflowY: "auto" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 12, marginBottom: 20 }}>
        <h2
          style={{
            fontFamily: "Syne, sans-serif",
            fontSize: 18,
            fontWeight: 700,
            color: "var(--amber)",
            textTransform: "uppercase",
            letterSpacing: 1,
          }}
        >
          Memory
        </h2>
        <span
          style={{
            fontFamily: "JetBrains Mono, monospace",
            fontSize: 11,
            color: "var(--text-dim)",
          }}
        >
          {entries.length} {entries.length === 1 ? "entry" : "entries"}
        </span>
      </div>

      {/* Toolbar */}
      <div style={{ display: "flex", gap: 8, marginBottom: 16, flexWrap: "wrap" }}>
        <input
          type="text"
          placeholder="Search memory..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSearch()}
          style={inputStyle}
        />
        <button onClick={handleSearch} style={btnSecondary}>
          Search
        </button>
        <button onClick={fetchAll} style={btnSecondary}>
          List All
        </button>
        <button
          onClick={() => setShowStore(!showStore)}
          style={btnPrimary}
        >
          + Store
        </button>
      </div>

      {/* Store form */}
      {showStore && (
        <div
          style={{
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            padding: 16,
            marginBottom: 16,
          }}
        >
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <input
              type="text"
              placeholder="Key"
              value={formKey}
              onChange={(e) => setFormKey(e.target.value)}
              style={inputStyle}
            />
            <input
              type="text"
              placeholder="Category"
              value={formCategory}
              onChange={(e) => setFormCategory(e.target.value)}
              style={{ ...inputStyle, maxWidth: 200 }}
            />
          </div>
          <textarea
            placeholder="Content..."
            value={formContent}
            onChange={(e) => setFormContent(e.target.value)}
            rows={4}
            style={{
              ...inputStyle,
              width: "100%",
              resize: "vertical",
              marginBottom: 8,
              flex: "unset",
            }}
          />
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button
              onClick={() => {
                setShowStore(false);
                setFormKey("");
                setFormCategory("");
                setFormContent("");
              }}
              style={btnSecondary}
            >
              Cancel
            </button>
            <button onClick={handleStore} style={btnPrimary}>
              Store
            </button>
          </div>
        </div>
      )}

      {loading && (
        <div
          style={{
            color: "var(--text-dim)",
            fontFamily: "Outfit, sans-serif",
            fontSize: 14,
            padding: 16,
          }}
        >
          Loading...
        </div>
      )}

      {/* Memory list + detail */}
      <div style={{ display: "flex", gap: 16 }}>
        {/* List */}
        <div
          style={{
            flex: 1,
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            overflow: "hidden",
          }}
        >
          {entries.length === 0 && !loading && (
            <div
              style={{
                padding: 32,
                textAlign: "center",
                color: "var(--text-dim)",
                fontFamily: "Outfit, sans-serif",
                fontSize: 13,
              }}
            >
              <div
                style={{
                  fontSize: 28,
                  fontFamily: "JetBrains Mono, monospace",
                  marginBottom: 8,
                  opacity: 0.4,
                }}
              >
                {"◇"}
              </div>
              No memory entries found
              <div style={{ fontSize: 11, marginTop: 4, color: "var(--text-dim)" }}>
                Use "+ Store" to add entries or chat with the agent to build memory
              </div>
            </div>
          )}
          {entries.map((entry) => {
            const isSelected = selected?.key === entry.key;
            return (
              <div
                key={entry.key}
                onClick={() => setSelected(isSelected ? null : entry)}
                style={{
                  padding: "10px 14px",
                  borderBottom: "1px solid var(--border)",
                  cursor: "pointer",
                  borderLeft: isSelected
                    ? "3px solid var(--amber)"
                    : "3px solid transparent",
                  background: isSelected
                    ? "var(--amber-glow)"
                    : "transparent",
                  transition: "background 0.15s",
                }}
              >
                <div
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 12,
                    fontWeight: 600,
                    color: "var(--amber-bright)",
                    marginBottom: 2,
                  }}
                >
                  {entry.key}
                </div>
                <div
                  style={{
                    fontFamily: "monospace",
                    fontSize: 10,
                    textTransform: "uppercase",
                    color: "var(--text-dim)",
                    marginBottom: 4,
                  }}
                >
                  {entry.category}
                </div>
                <div
                  style={{
                    fontFamily: "Outfit, sans-serif",
                    fontSize: 12,
                    color: "var(--text-dim)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {entry.content}
                </div>
              </div>
            );
          })}
        </div>

        {/* Detail panel */}
        {selected && (
          <div
            style={{
              width: 360,
              flexShrink: 0,
              border: "1px solid var(--border-amber)",
              clipPath: clipCorner(10),
              padding: 16,
              background: "var(--bg-card)",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                marginBottom: 12,
              }}
            >
              <span
                style={{
                  fontFamily: "JetBrains Mono, monospace",
                  fontSize: 14,
                  fontWeight: 700,
                  color: "var(--amber-bright)",
                }}
              >
                {selected.key}
              </span>
              <div style={{ display: "flex", gap: 6 }}>
                <button
                  onClick={() => handleCopy(selected.content)}
                  style={{
                    ...btnSecondary,
                    padding: "4px 10px",
                    fontSize: 10,
                  }}
                >
                  Copy
                </button>
                <button
                  onClick={() => handleDelete(selected.key)}
                  style={{
                    ...btnSecondary,
                    padding: "4px 10px",
                    fontSize: 10,
                    borderColor: "var(--error-text)",
                    color: "var(--error-text)",
                  }}
                >
                  Delete
                </button>
              </div>
            </div>

            <div
              style={{
                background: "var(--bg-input)",
                fontFamily: "JetBrains Mono, monospace",
                fontSize: 12,
                color: "var(--text-primary)",
                padding: 12,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                maxHeight: 300,
                overflowY: "auto",
                clipPath: clipCorner(6),
                marginBottom: 12,
              }}
            >
              {selected.content}
            </div>

            <div
              style={{
                fontFamily: "Outfit, sans-serif",
                fontSize: 12,
                color: "var(--text-dim)",
                display: "flex",
                flexDirection: "column",
                gap: 4,
              }}
            >
              <span>
                Category:{" "}
                <span style={{ color: "var(--text-primary)" }}>
                  {selected.category}
                </span>
              </span>
              <span>
                Timestamp:{" "}
                <span style={{ color: "var(--text-primary)" }}>
                  {selected.timestamp}
                </span>
              </span>
              {selected.score != null && (
                <span>
                  Score:{" "}
                  <span style={{ color: "var(--amber)" }}>
                    {selected.score.toFixed(4)}
                  </span>
                </span>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
