import { useState } from "react";
import { setToken, listInstances, type InstanceInfo } from "../api";
import { clipCorner } from "../theme";

interface Props {
  onLogin: (instances: InstanceInfo[]) => void;
}

export function Login({ onLogin }: Props) {
  const [token, setTokenInput] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handleSubmit = async () => {
    const t = token.trim();
    if (!t) return;
    setLoading(true);
    setError("");
    setToken(t);
    try {
      const instances = await listInstances();
      onLogin(instances);
    } catch {
      setError("Authentication failed");
      setToken("");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div
        style={{
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          clipPath: clipCorner(12),
          padding: 40,
          width: 380,
          textAlign: "center",
        }}
      >
        <h2
          style={{
            fontFamily: "'Syne', sans-serif",
            fontSize: 22,
            fontWeight: 800,
            color: "var(--amber)",
            marginBottom: 6,
            letterSpacing: 2,
          }}
        >
          ZC//GW
        </h2>
        <p
          style={{
            fontSize: 13,
            color: "var(--text-dim)",
            marginBottom: 28,
          }}
        >
          Enter gateway auth token
        </p>
        <input
          type="password"
          placeholder="token"
          value={token}
          onChange={(e) => setTokenInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleSubmit();
          }}
          style={{
            width: "100%",
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 14,
            padding: "12px 14px",
            background: "var(--bg-input)",
            border: "1px solid var(--border)",
            color: "var(--text-primary)",
            marginBottom: 16,
            outline: "none",
            textAlign: "center",
            letterSpacing: 2,
            clipPath: clipCorner(6),
          }}
        />
        <button
          onClick={handleSubmit}
          disabled={loading}
          style={{
            width: "100%",
            fontFamily: "'JetBrains Mono', monospace",
            fontSize: 12,
            fontWeight: 700,
            letterSpacing: 2,
            textTransform: "uppercase",
            padding: 12,
            background: "var(--amber)",
            color: "#000",
            border: "none",
            clipPath: clipCorner(6),
            opacity: loading ? 0.5 : 1,
          }}
        >
          {loading ? "AUTHENTICATING..." : "AUTHENTICATE"}
        </button>
        {error && (
          <div
            style={{
              color: "var(--error-text)",
              fontSize: 12,
              marginTop: 12,
              fontFamily: "'JetBrains Mono', monospace",
            }}
          >
            {error}
          </div>
        )}
      </div>
    </div>
  );
}
