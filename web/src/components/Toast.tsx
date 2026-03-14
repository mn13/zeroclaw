import { clipCorner } from "../theme";

interface ToastProps {
  toast: {
    message: string;
    isError: boolean;
    visible: boolean;
  };
}

export function Toast({ toast }: ToastProps) {
  return (
    <div
      style={{
        position: "fixed",
        bottom: 24,
        right: 24,
        background: "var(--bg-card)",
        border: `1px solid ${toast.isError ? "var(--error-text)" : "var(--border-amber)"}`,
        padding: "10px 16px",
        fontFamily: "'JetBrains Mono', monospace",
        fontSize: 12,
        color: toast.isError ? "var(--error-text)" : "var(--text-primary)",
        clipPath: clipCorner(8),
        transform: toast.visible ? "translateY(0)" : "translateY(20px)",
        opacity: toast.visible ? 1 : 0,
        transition: "transform 0.25s ease, opacity 0.25s ease",
        pointerEvents: toast.visible ? "auto" : "none",
        zIndex: 1000,
        maxWidth: 360,
        boxShadow: "0 4px 12px rgba(0,0,0,0.3)",
      }}
    >
      {toast.message}
    </div>
  );
}
