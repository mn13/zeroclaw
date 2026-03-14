import { useState } from "react";
import { applyTheme, type ThemeMode } from "../theme";

const STORAGE_KEY = "zcgw-theme";

function getStoredTheme(): ThemeMode {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // localStorage unavailable
  }
  return "dark";
}

export function ThemeToggle() {
  const [mode, setMode] = useState<ThemeMode>(getStoredTheme);

  const toggle = () => {
    const next: ThemeMode = mode === "dark" ? "light" : "dark";
    setMode(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // ignore
    }
    applyTheme(next);
  };

  return (
    <button
      onClick={toggle}
      title={`Switch to ${mode === "dark" ? "light" : "dark"} theme`}
      style={{
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: 14,
        background: "none",
        border: "1px solid var(--border)",
        color: "var(--text-dim)",
        cursor: "pointer",
        padding: "4px 8px",
        lineHeight: 1,
        borderRadius: 0,
        transition: "color 0.15s ease, border-color 0.15s ease",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.color = "var(--amber)";
        e.currentTarget.style.borderColor = "var(--border-amber)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.color = "var(--text-dim)";
        e.currentTarget.style.borderColor = "var(--border)";
      }}
    >
      {mode === "dark" ? "\u263C" : "\u263E"}
    </button>
  );
}
