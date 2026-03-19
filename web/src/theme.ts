export type ThemeMode = "dark" | "light";

const themes = {
  dark: {
    amber: "#F59E0B",
    "amber-dim": "#92600a",
    "amber-glow": "rgba(245,158,11,0.10)",
    "amber-bright": "#FBBF24",
    "bg-dark": "#0c0c0e",
    "bg-card": "#141418",
    "bg-card-hover": "#1c1c22",
    border: "#252530",
    "border-amber": "rgba(245,158,11,0.25)",
    "text-primary": "#e8e8ed",
    "text-dim": "#7a7a8a",
    "bg-input": "#101014",
    "bg-deep": "#08080a",
    "bg-sidebar": "#0a0a0e",
    "error-text": "#fca5a5",
    "error-bg": "rgba(239,68,68,0.08)",
    "success": "#22c55e",
    "toggle-off": "#3a3a44",
    "row-hover-bg": "rgba(255,255,255,0.03)",
    "log-border": "rgba(255,255,255,0.04)",
    "bg-dropdown": "#18181e",
    "dropdown-hover": "#22222a",
  },
  light: {
    amber: "#d97706",
    "amber-dim": "#b45309",
    "amber-glow": "rgba(217,119,6,0.10)",
    "amber-bright": "#f59e0b",
    "bg-dark": "#fbfbfc",
    "bg-card": "#ffffff",
    "bg-card-hover": "#f7f7f9",
    border: "#e8e8ec",
    "border-amber": "rgba(217,119,6,0.22)",
    "text-primary": "#111111",
    "text-dim": "#555e6b",
    "bg-input": "#f7f8fa",
    "bg-deep": "#ffffff",
    "bg-sidebar": "#f8f9fb",
    "error-text": "#dc2626",
    "error-bg": "rgba(239,68,68,0.06)",
    "success": "#16a34a",
    "toggle-off": "#c5c9d0",
    "row-hover-bg": "rgba(0,0,0,0.025)",
    "log-border": "rgba(0,0,0,0.06)",
    "bg-dropdown": "#ffffff",
    "dropdown-hover": "#f2f3f7",
  },
} as const;

export function applyTheme(mode: ThemeMode) {
  const palette = themes[mode];
  const root = document.documentElement;
  root.setAttribute("data-theme", mode);
  for (const [key, value] of Object.entries(palette)) {
    root.style.setProperty(`--${key}`, value);
  }
}

export const clipCorner = (size = 12) =>
  `polygon(${size}px 0%, 100% 0%, 100% calc(100% - ${size}px), calc(100% - ${size}px) 100%, 0% 100%, 0% ${size}px)`;
