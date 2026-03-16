import type { View } from "../App";
import { ThemeToggle } from "./ThemeToggle";

interface HeaderProps {
  view: View;
}

const viewTitles: Record<View, string> = {
  chat: "INTERACT",
  config: "CONFIGURATION",
  memory: "MEMORY",
  tools: "TOOLS",
  status: "STATUS",
  admin: "ADMIN PANEL",
};

export function Header({ view }: HeaderProps) {
  return (
    <header
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "0 20px",
        height: 48,
        minHeight: 48,
        background: "var(--bg-card)",
        borderBottom: "1px solid var(--border)",
      }}
    >
      {/* Page title */}
      <span
        style={{
          fontFamily: "'Syne', sans-serif",
          fontSize: 16,
          fontWeight: 700,
          color: "var(--text-primary)",
          letterSpacing: 1,
        }}
      >
        {viewTitles[view]}
      </span>

      <ThemeToggle />
    </header>
  );
}
