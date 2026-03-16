import { useState, useRef, useEffect, useCallback } from "react";
import type { View } from "../App";
import type { InstanceInfo } from "../api";
import { clipCorner } from "../theme";

interface SidebarProps {
  view: View;
  onViewChange: (v: View) => void;
  expanded: boolean;
  onToggle: () => void;
  health: "healthy" | "unhealthy" | "unknown";
  instanceSelected: boolean;
  instances: InstanceInfo[];
  currentInstance: string;
  onInstanceChange: (id: string) => void;
}

const navItems: { key: View; icon: string; label: string }[] = [
  { key: "chat", icon: "\u27E9_", label: "INTERACT" },
  { key: "config", icon: "\u2699", label: "CONFIG" },
  { key: "memory", icon: "\u25C7", label: "MEMORY" },
  { key: "tools", icon: "\u25A4", label: "TOOLS" },
  { key: "identity", icon: "\u2662", label: "IDENTITY" },
  { key: "connectors", icon: "\u2261", label: "CONNECT" },
  { key: "integrations", icon: "\u2A01", label: "INTEGRATE" },
  { key: "cron", icon: "\u23F0", label: "CRON" },
  { key: "status", icon: "\u25CB", label: "STATUS" },
];

const healthColor: Record<string, string> = {
  healthy: "#22c55e",
  unhealthy: "#ef4444",
  unknown: "#737373",
};

const healthLabel: Record<string, string> = {
  healthy: "HEALTHY",
  unhealthy: "UNHEALTHY",
  unknown: "UNKNOWN",
};

function InstanceSelector({
  instances,
  currentInstance,
  onInstanceChange,
  expanded,
  view,
  onViewChange,
}: {
  instances: InstanceInfo[];
  currentInstance: string;
  onInstanceChange: (id: string) => void;
  expanded: boolean;
  view: View;
  onViewChange: (v: View) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  const current = instances.find((i) => i.id === currentInstance);
  const label = current?.display_name || current?.id || "—";
  const initial = label.charAt(0).toUpperCase();

  const handleClickOutside = useCallback((e: MouseEvent) => {
    if (ref.current && !ref.current.contains(e.target as Node)) {
      setOpen(false);
    }
  }, []);

  useEffect(() => {
    if (open) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [open, handleClickOutside]);

  if (instances.length === 0) return null;

  return (
    <div ref={ref} style={{ position: "relative", padding: "0 10px" }}>
      {/* Trigger */}
      <button
        onClick={() => setOpen((o) => !o)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          width: "100%",
          padding: "8px 6px",
          background: open ? "var(--amber-glow)" : "transparent",
          border: "1px solid var(--border)",
          clipPath: clipCorner(6),
          cursor: "pointer",
          transition: "background 0.15s",
        }}
        onMouseEnter={(e) => {
          if (!open) e.currentTarget.style.background = "var(--row-hover-bg)";
        }}
        onMouseLeave={(e) => {
          if (!open) e.currentTarget.style.background = "transparent";
        }}
      >
        {/* Avatar circle */}
        <span
          style={{
            width: 26,
            height: 26,
            minWidth: 26,
            borderRadius: "50%",
            background: "var(--amber)",
            color: "#000",
            fontFamily: "'Syne', sans-serif",
            fontWeight: 800,
            fontSize: 12,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            lineHeight: 1,
          }}
        >
          {initial}
        </span>
        {expanded && (
          <>
            <div style={{ flex: 1, minWidth: 0, textAlign: "left" }}>
              <div
                style={{
                  fontFamily: "'Outfit', sans-serif",
                  fontSize: 12,
                  fontWeight: 600,
                  color: "var(--text-primary)",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {label}
              </div>
              <div
                style={{
                  fontFamily: "'JetBrains Mono', monospace",
                  fontSize: 9,
                  color: "var(--text-dim)",
                  letterSpacing: 0.5,
                }}
              >
                {current?.health === "healthy" ? "ONLINE" : current?.health?.toUpperCase() || ""}
              </div>
            </div>
            <span
              style={{
                fontSize: 10,
                color: "var(--text-dim)",
                transition: "transform 0.2s",
                transform: open ? "rotate(180deg)" : "rotate(0deg)",
              }}
            >
              ▲
            </span>
          </>
        )}
      </button>

      {/* Dropdown */}
      {open && (
        <div
          style={{
            position: "absolute",
            bottom: "calc(100% + 4px)",
            left: 10,
            right: expanded ? 10 : "auto",
            minWidth: expanded ? undefined : 200,
            background: "var(--bg-dropdown)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(8),
            boxShadow: "0 -8px 24px rgba(0,0,0,0.4)",
            zIndex: 100,
            overflow: "hidden",
          }}
        >
          <div
            style={{
              padding: "8px 10px 4px",
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 9,
              color: "var(--text-dim)",
              letterSpacing: 1,
              textTransform: "uppercase",
            }}
          >
            AGENTS
          </div>
          {instances.map((inst) => {
            const active = inst.id === currentInstance;
            const name = inst.display_name || inst.id;
            return (
              <button
                key={inst.id}
                onClick={() => {
                  onInstanceChange(inst.id);
                  setOpen(false);
                }}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  width: "100%",
                  padding: "8px 10px",
                  background: active ? "var(--amber-glow)" : "transparent",
                  border: "none",
                  cursor: "pointer",
                  transition: "background 0.1s",
                }}
                onMouseEnter={(e) => {
                  if (!active) e.currentTarget.style.background = "var(--dropdown-hover)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = active ? "var(--amber-glow)" : "transparent";
                }}
              >
                <span
                  style={{
                    width: 8,
                    height: 8,
                    minWidth: 8,
                    borderRadius: "50%",
                    background:
                      inst.health === "healthy"
                        ? "#22c55e"
                        : inst.health === "unhealthy"
                          ? "#ef4444"
                          : "#737373",
                    boxShadow:
                      inst.health === "healthy"
                        ? "0 0 4px rgba(34,197,94,0.5)"
                        : "none",
                  }}
                />
                <span
                  style={{
                    fontFamily: "'Outfit', sans-serif",
                    fontSize: 12,
                    fontWeight: active ? 600 : 400,
                    color: active ? "var(--amber)" : "var(--text-primary)",
                    whiteSpace: "nowrap",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {name}
                </span>
                {active && (
                  <span
                    style={{
                      marginLeft: "auto",
                      fontFamily: "'JetBrains Mono', monospace",
                      fontSize: 9,
                      color: "var(--amber-dim)",
                      letterSpacing: 1,
                    }}
                  >
                    ✓
                  </span>
                )}
              </button>
            );
          })}
          {/* Divider */}
          <div style={{ height: 1, background: "var(--border)", margin: "4px 0" }} />
          {/* Admin section */}
          <div
            style={{
              padding: "8px 10px 4px",
              fontFamily: "'JetBrains Mono', monospace",
              fontSize: 9,
              color: "var(--text-dim)",
              letterSpacing: 1,
              textTransform: "uppercase",
            }}
          >
            ADMIN
          </div>
          <button
            onClick={() => {
              onViewChange("admin");
              setOpen(false);
            }}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              width: "100%",
              padding: "8px 10px",
              background: view === "admin" ? "var(--amber-glow)" : "transparent",
              border: "none",
              cursor: "pointer",
              transition: "background 0.1s",
            }}
            onMouseEnter={(e) => {
              if (view !== "admin") e.currentTarget.style.background = "var(--dropdown-hover)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = view === "admin" ? "var(--amber-glow)" : "transparent";
            }}
          >
            <span style={{ fontSize: 14, minWidth: 8 }}>⚙</span>
            <span
              style={{
                fontFamily: "'Outfit', sans-serif",
                fontSize: 12,
                fontWeight: view === "admin" ? 600 : 400,
                color: view === "admin" ? "var(--amber)" : "var(--text-primary)",
                whiteSpace: "nowrap",
              }}
            >
              Admin Panel
            </span>
          </button>
        </div>
      )}
    </div>
  );
}

export function Sidebar({
  view,
  onViewChange,
  expanded,
  onToggle,
  health,
  instances,
  currentInstance,
  onInstanceChange,
}: SidebarProps) {
  const width = expanded ? 220 : 56;

  return (
    <nav
      style={{
        width,
        minWidth: width,
        height: "100vh",
        background: "var(--bg-sidebar)",
        borderRight: "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        transition: "width 0.25s ease, min-width 0.25s ease",
        overflow: "hidden",
        userSelect: "none",
      }}
    >
      {/* Logo area */}
      <button
        onClick={onToggle}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "16px 14px",
          background: "none",
          border: "none",
          cursor: "pointer",
          width: "100%",
          textAlign: "left",
        }}
      >
        <span
          style={{
            fontFamily: "'Syne', sans-serif",
            fontWeight: 800,
            fontSize: 20,
            color: "var(--amber)",
            lineHeight: 1,
            minWidth: 28,
            textAlign: "center",
          }}
        >
          ZC
        </span>
        <span
          style={{
            fontFamily: "'Syne', sans-serif",
            fontWeight: 800,
            fontSize: 12,
            color: "var(--text-primary)",
            letterSpacing: 3,
            opacity: expanded ? 1 : 0,
            transition: "opacity 0.2s ease",
            whiteSpace: "nowrap",
          }}
        >
          GATEWAY
        </span>
      </button>

      {/* Nav items */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 2, padding: "8px 0" }}>
        {navItems.map((item) => {
          const active = view === item.key;
          return (
            <button
              key={item.key}
              onClick={() => onViewChange(item.key)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "10px 14px",
                background: active ? "var(--amber-glow)" : "transparent",
                border: "none",
                borderLeft: `2px solid ${active ? "var(--amber)" : "transparent"}`,
                cursor: "pointer",
                width: "100%",
                textAlign: "left",
                transition: "background 0.15s ease, border-color 0.15s ease",
              }}
              onMouseEnter={(e) => {
                if (!active) {
                  e.currentTarget.style.background = "var(--row-hover-bg)";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = active ? "var(--amber-glow)" : "transparent";
              }}
            >
              <span
                style={{
                  fontFamily: "'JetBrains Mono', monospace",
                  fontSize: 16,
                  color: active ? "var(--amber)" : "var(--text-dim)",
                  minWidth: 28,
                  textAlign: "center",
                  lineHeight: 1,
                }}
              >
                {item.icon}
              </span>
              <span
                style={{
                  fontFamily: "'Syne', sans-serif",
                  fontSize: 11,
                  fontWeight: 600,
                  letterSpacing: 2,
                  color: active ? "var(--amber)" : "var(--text-dim)",
                  opacity: expanded ? 1 : 0,
                  transition: "opacity 0.2s ease",
                  whiteSpace: "nowrap",
                }}
              >
                {item.label}
              </span>
            </button>
          );
        })}
      </div>

      {/* Instance selector */}
      <InstanceSelector
        instances={instances}
        currentInstance={currentInstance}
        onInstanceChange={onInstanceChange}
        expanded={expanded}
        view={view}
        onViewChange={onViewChange}
      />

      {/* Health status */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "10px 14px",
          borderTop: "1px solid var(--border)",
        }}
      >
        <span
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            background: healthColor[health],
            minWidth: 8,
            boxShadow:
              health === "healthy"
                ? "0 0 6px rgba(34,197,94,0.5)"
                : health === "unhealthy"
                  ? "0 0 6px rgba(239,68,68,0.5)"
                  : "none",
          }}
        />
        <span
          style={{
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 10,
            color: "var(--text-dim)",
            letterSpacing: 1,
            opacity: expanded ? 1 : 0,
            transition: "opacity 0.2s ease",
            whiteSpace: "nowrap",
          }}
        >
          {healthLabel[health]}
        </span>
      </div>
    </nav>
  );
}
