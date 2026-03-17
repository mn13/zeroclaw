import { useState, useRef, useEffect, useCallback } from "react";
import type { ChatMessage, ThinkingStep, ToolCallInfo, WsIncoming } from "../types";
import { connectChat, getHistory } from "../api";
import { clipCorner } from "../theme";

interface ChatProps {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

let _msgId = 0;
function nextId(): string {
  return `msg-${++_msgId}-${Date.now()}`;
}

/** Strip emoji progress prefixes from thinking text. */
function cleanThinking(text: string): string {
  return text
    .replace(/\u{1F914}\s*Thinking(?:\s*\(round \d+\))?\.{0,3}\s*/gu, "")
    .replace(/\u{1F4AC}\s*Got \d+ tool call\(s\).*\n?/gu, "")
    .trim();
}

const STATUS_BADGE: Record<ToolCallInfo["status"], { label: string; color: string }> = {
  running: { label: "RUNNING", color: "var(--amber)" },
  success: { label: "OK", color: "var(--success)" },
  fail: { label: "FAIL", color: "var(--error-text)" },
};

export function Chat({ instanceId, toast }: ChatProps) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [connectionStatus, setConnectionStatus] = useState("disconnected");
  const [tokenInfo, setTokenInfo] = useState({ input: 0, output: 0 });
  const [expandedTools, setExpandedTools] = useState<Set<string>>(new Set());
  const [expandedSteps, setExpandedSteps] = useState<Set<string>>(new Set());

  const wsRef = useRef<WebSocket | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const currentTurnIdRef = useRef<string | null>(null);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (el) {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          el.scrollTop = el.scrollHeight;
        });
      });
    }
  }, []);

  // Load history on mount / instance change
  useEffect(() => {
    if (!instanceId) return;
    let cancelled = false;

    getHistory(instanceId)
      .then((res) => {
        if (cancelled) return;
        const loaded: ChatMessage[] = res.messages.map((m) => ({
          id: nextId(),
          role: m.role === "user" ? "user" : "assistant",
          content: m.content,
        }));
        setMessages(loaded);
        setTimeout(scrollToBottom, 50);
      })
      .catch((err) => {
        if (!cancelled) toast(`Failed to load history: ${err.message}`, true);
      });

    return () => {
      cancelled = true;
    };
  }, [instanceId, toast, scrollToBottom]);

  // WebSocket connection
  useEffect(() => {
    if (!instanceId) return;

    const ws = connectChat(instanceId);
    wsRef.current = ws;
    setConnectionStatus("connecting");

    ws.onopen = () => setConnectionStatus("connected");
    ws.onclose = () => { setConnectionStatus("disconnected"); setStreaming(false); };
    ws.onerror = () => { setConnectionStatus("error"); setStreaming(false); };

    ws.onmessage = (ev) => {
      let msg: WsIncoming;
      try { msg = JSON.parse(ev.data) as WsIncoming; } catch { return; }

      switch (msg.type) {
        case "turn_start":
          currentTurnIdRef.current = msg.turn_id;
          setStreaming(true);
          setMessages((prev) => [
            ...prev,
            { id: msg.turn_id, role: "assistant", content: "", steps: [], toolCalls: [] },
          ]);
          break;

        case "clear":
          // Each CLEAR pushes accumulated content + toolCalls into a new step.
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== msg.turn_id) return m;
              const stepText = m.content || "";
              const stepTools = m.toolCalls ?? [];
              if (!stepText && stepTools.length === 0) return { ...m, content: "" };
              const step: ThinkingStep = { text: stepText, toolCalls: stepTools.length > 0 ? stepTools : undefined };
              return { ...m, steps: [...(m.steps ?? []), step], content: "", toolCalls: [] };
            }),
          );
          break;

        case "delta":
          setMessages((prev) =>
            prev.map((m) =>
              m.id === msg.turn_id ? { ...m, content: m.content + msg.content } : m,
            ),
          );
          scrollToBottom();
          break;

        case "tool_start":
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== msg.turn_id) return m;
              const tc: ToolCallInfo = { tool: msg.tool, arguments: msg.arguments, status: "running" };
              return { ...m, toolCalls: [...(m.toolCalls ?? []), tc] };
            }),
          );
          scrollToBottom();
          break;

        case "tool_result":
          setMessages((prev) =>
            prev.map((m) => {
              if (m.id !== msg.turn_id) return m;
              const calls = (m.toolCalls ?? []).map((tc) =>
                tc.tool === msg.tool && tc.status === "running"
                  ? { ...tc, status: (msg.success ? "success" : "fail") as ToolCallInfo["status"], output: msg.output }
                  : tc,
              );
              return { ...m, toolCalls: calls };
            }),
          );
          scrollToBottom();
          break;

        case "done":
          setMessages((prev) =>
            prev.map((m) =>
              m.id === msg.turn_id ? { ...m, content: m.content || msg.content } : m,
            ),
          );
          setStreaming(false);
          setTokenInfo({ input: msg.input_tokens, output: msg.output_tokens });
          try {
            const raw = localStorage.getItem("zcgw-token-usage");
            const usage = raw ? JSON.parse(raw) : { input: 0, output: 0, turns: 0, lastReset: new Date().toISOString() };
            usage.input += msg.input_tokens;
            usage.output += msg.output_tokens;
            usage.turns += 1;
            localStorage.setItem("zcgw-token-usage", JSON.stringify(usage));
          } catch { /* ignore */ }
          currentTurnIdRef.current = null;
          scrollToBottom();
          break;

        case "error":
          setMessages((prev) => [
            ...prev,
            { id: nextId(), role: "error", content: msg.message },
          ]);
          setStreaming(false);
          currentTurnIdRef.current = null;
          scrollToBottom();
          break;

        case "queued":
          setConnectionStatus(`queued #${msg.position}`);
          break;

        case "status":
          break;
      }
    };

    return () => { ws.close(); wsRef.current = null; };
  }, [instanceId, scrollToBottom]);

  const sendMessage = useCallback(() => {
    const text = input.trim();
    if (!text || !wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return;
    setMessages((prev) => [...prev, { id: nextId(), role: "user", content: text }]);
    setInput("");
    try { wsRef.current.send(JSON.stringify({ type: "message", content: text })); }
    catch (err) { toast(`Send failed: ${(err as Error).message}`, true); }
    scrollToBottom();
    inputRef.current?.focus();
  }, [input, toast, scrollToBottom]);

  const cancelStream = useCallback(() => {
    if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify({ type: "cancel" }));
    }
    setStreaming(false);
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); if (!streaming) sendMessage(); }
    },
    [sendMessage, streaming],
  );

  const toggleToolExpanded = useCallback((key: string) => {
    setExpandedTools((prev) => { const n = new Set(prev); n.has(key) ? n.delete(key) : n.add(key); return n; });
  }, []);

  const toggleStep = useCallback((key: string) => {
    setExpandedSteps((prev) => { const n = new Set(prev); n.has(key) ? n.delete(key) : n.add(key); return n; });
  }, []);

  // ── Render helpers ──

  function renderToolCard(tc: ToolCallInfo, idx: number, parentKey: string) {
    const badge = STATUS_BADGE[tc.status];
    const toolKey = `${parentKey}-${tc.tool}-${idx}`;
    const hasLongOutput = (tc.output?.length ?? 0) > 500;
    const isExpanded = expandedTools.has(toolKey);

    return (
      <div key={toolKey} style={{ background: "var(--bg-input)", border: "1px solid var(--border)", clipPath: clipCorner(6), marginTop: 4, marginBottom: 2, padding: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 8px", borderBottom: "1px solid var(--border)" }}>
          <span style={{ color: "var(--text-dim)", fontSize: 10 }}>{tc.status === "running" ? "\u25B8" : "\u25BE"}</span>
          <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 9, fontWeight: 700, color: "var(--amber-bright)", textTransform: "uppercase", letterSpacing: 1 }}>{tc.tool}</span>
          <span style={{ marginLeft: "auto", fontFamily: "'JetBrains Mono', monospace", fontSize: 8, fontWeight: 600, color: badge.color, letterSpacing: 1, padding: "1px 4px", border: `1px solid ${badge.color}`, borderRadius: 2 }}>{badge.label}</span>
        </div>
        <div style={{ padding: "4px 8px", fontFamily: "'JetBrains Mono', monospace", fontSize: 9, color: "var(--text-dim)", whiteSpace: "pre-wrap", wordBreak: "break-all", lineHeight: 1.4 }}>{tc.arguments}</div>
        {tc.status === "running" && (
          <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "4px 8px", borderTop: "1px solid var(--border)" }}>
            <span style={{ display: "inline-block", width: 5, height: 5, background: "var(--amber)", animation: "pulse-cube 1.5s ease-in-out infinite" }} />
            <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 9, color: "var(--text-dim)", letterSpacing: 1 }}>processing</span>
          </div>
        )}
        {tc.output != null && (
          <div style={{ padding: "4px 8px", borderTop: "1px solid var(--border)" }}>
            {hasLongOutput && (
              <button onClick={() => toggleToolExpanded(toolKey)} style={{ background: "none", border: "none", fontFamily: "'JetBrains Mono', monospace", fontSize: 9, color: "var(--amber-dim)", cursor: "pointer", padding: "2px 0", marginBottom: 2, letterSpacing: 1 }}>
                {isExpanded ? "\u25BE COLLAPSE" : "\u25B8 EXPAND OUTPUT"}
              </button>
            )}
            <div style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 9, color: tc.status === "fail" ? "var(--error-text)" : "var(--text-primary)", whiteSpace: "pre-wrap", wordBreak: "break-all", lineHeight: 1.4, maxHeight: hasLongOutput && !isExpanded ? 60 : undefined, overflow: hasLongOutput && !isExpanded ? "hidden" : undefined }}>{tc.output}</div>
          </div>
        )}
      </div>
    );
  }

  /** Render a single collapsible thinking step. */
  function renderStep(step: ThinkingStep, idx: number, msgId: string, total: number) {
    const stepKey = `${msgId}-step-${idx}`;
    const isOpen = expandedSteps.has(stepKey);
    const cleaned = cleanThinking(step.text);
    const toolCount = (step.toolCalls ?? []).length;
    const label = total > 1 ? `step ${idx + 1}` : "thinking";
    const detail = toolCount > 0 ? ` + ${toolCount} tool${toolCount > 1 ? "s" : ""}` : "";

    return (
      <div key={stepKey} style={{ borderBottom: "1px solid var(--border)" }}>
        <button
          onClick={() => toggleStep(stepKey)}
          style={{
            display: "flex", alignItems: "center", gap: 6, background: "none", border: "none",
            cursor: "pointer", padding: "5px 12px", width: "100%",
          }}
        >
          <span style={{
            fontFamily: "'JetBrains Mono', monospace", fontSize: 9, color: "var(--text-dim)",
            transition: "transform 0.15s", transform: isOpen ? "rotate(90deg)" : "rotate(0deg)", display: "inline-block",
          }}>{"\u25B6"}</span>
          <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 10, color: "var(--text-dim)", letterSpacing: 0.5 }}>
            {label}{detail}
          </span>
        </button>
        {isOpen && (
          <div style={{ padding: "0 12px 6px 24px" }}>
            {cleaned && (
              <div style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 10, color: "var(--text-dim)", whiteSpace: "pre-wrap", lineHeight: 1.4, marginBottom: toolCount > 0 ? 4 : 0 }}>
                {cleaned}
              </div>
            )}
            {step.toolCalls?.map((tc, i) => renderToolCard(tc, i, stepKey))}
          </div>
        )}
      </div>
    );
  }

  function renderMessage(msg: ChatMessage) {
    const isUser = msg.role === "user";
    const isError = msg.role === "error";
    const isActive = streaming && msg.id === currentTurnIdRef.current;
    const steps = msg.steps ?? [];
    const hasSteps = steps.length > 0;
    // While streaming before any CLEAR, show a live indicator if content is accumulating (thinking phase)
    const isThinkingPhase = isActive && !hasSteps && !msg.content;

    const senderLabel = isError ? "ERR" : isUser ? "YOU" : "ZC";
    const senderColor = isError ? "var(--error-text)" : isUser ? "var(--text-dim)" : "var(--amber)";
    const bubbleBg = isError ? "var(--error-bg)" : isUser ? "var(--bg-card)" : "var(--amber-glow)";
    const bubbleBorder = isError ? "1px solid var(--error-text)" : isUser ? "1px solid var(--border)" : "1px solid var(--amber-dim)";

    return (
      <div key={msg.id} style={{ display: "flex", justifyContent: isUser ? "flex-end" : "flex-start", padding: "4px 16px" }}>
        <div style={{ maxWidth: isUser ? "75%" : "85%", minWidth: isUser ? 60 : 300 }}>
          {/* Sender label */}
          <div style={{
            fontFamily: "'JetBrains Mono', monospace", fontSize: 9, fontWeight: 700,
            color: senderColor, textTransform: "uppercase", letterSpacing: 1,
            marginBottom: 3, textAlign: isUser ? "right" : "left", userSelect: "none",
          }}>
            {senderLabel}
          </div>

          {/* Bubble */}
          <div style={{ background: bubbleBg, border: bubbleBorder, clipPath: clipCorner(8), overflow: "hidden" }}>
            {/* Thinking steps (collapsed toggles inside the bubble) */}
            {!isUser && !isError && hasSteps && steps.map((s, i) => renderStep(s, i, msg.id, steps.length))}

            {/* Live thinking indicator (before first CLEAR) */}
            {!isUser && !isError && isThinkingPhase && (
              <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "8px 12px" }}>
                <span style={{ display: "inline-block", width: 5, height: 5, background: "var(--amber)", animation: "pulse-cube 1.5s ease-in-out infinite" }} />
                <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 10, color: "var(--text-dim)", letterSpacing: 1 }}>thinking</span>
              </div>
            )}

            {/* Main content */}
            {msg.content && (
              <div style={{
                fontFamily: "'Outfit', sans-serif", fontSize: 13, lineHeight: 1.6,
                color: isError ? "var(--error-text)" : "var(--text-primary)",
                whiteSpace: "pre-wrap", wordBreak: "break-word", padding: "8px 12px",
              }}>
                {msg.content}
              </div>
            )}

            {/* Streaming cursor — waiting for first byte */}
            {!isUser && !isError && isActive && !msg.content && !isThinkingPhase && hasSteps && (
              <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "8px 12px" }}>
                <span style={{ display: "inline-block", width: 5, height: 5, background: "var(--amber)", animation: "pulse-cube 1.5s ease-in-out infinite" }} />
                <span style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: 10, color: "var(--text-dim)", letterSpacing: 1 }}>writing</span>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  // ── Main layout ──

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", background: "var(--bg-dark)" }}>
      {/* Messages area */}
      <div ref={scrollRef} style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4, padding: "12px 0" }}>
        {messages.length === 0 && (
          <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-dim)", fontFamily: "'JetBrains Mono', monospace", fontSize: 11, letterSpacing: 2 }}>
            NO MESSAGES YET
          </div>
        )}
        {messages.map((msg) => renderMessage(msg))}
      </div>

      {/* Input bar */}
      <div style={{ borderTop: "1px solid var(--border)", padding: "10px 16px", display: "flex", gap: 8, alignItems: "flex-end", background: "var(--bg-card)" }}>
        <textarea
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type a message..."
          rows={1}
          style={{
            flex: 1, fontFamily: "'JetBrains Mono', monospace", fontSize: 13,
            background: "var(--bg-input)", color: "var(--text-primary)", border: "1px solid var(--border)",
            clipPath: clipCorner(6), padding: "10px 12px", resize: "none", outline: "none",
            lineHeight: 1.5, minHeight: 40, maxHeight: 160,
          }}
          onInput={(e) => {
            const t = e.currentTarget;
            t.style.height = "auto";
            t.style.height = `${Math.min(t.scrollHeight, 160)}px`;
          }}
        />
        {streaming ? (
          <button onClick={cancelStream} style={{
            fontFamily: "'JetBrains Mono', monospace", fontSize: 11, fontWeight: 700,
            textTransform: "uppercase", letterSpacing: 1, background: "transparent",
            color: "var(--error-text)", border: "1px solid var(--error-text)",
            clipPath: clipCorner(6), padding: "10px 18px", cursor: "pointer", whiteSpace: "nowrap",
          }}>CANCEL</button>
        ) : (
          <button onClick={sendMessage} disabled={!input.trim() || connectionStatus !== "connected"} style={{
            fontFamily: "'JetBrains Mono', monospace", fontSize: 11, fontWeight: 700,
            textTransform: "uppercase", letterSpacing: 1,
            background: !input.trim() || connectionStatus !== "connected" ? "var(--amber-dim)" : "var(--amber)",
            color: "#000", border: "none", clipPath: clipCorner(6), padding: "10px 18px",
            cursor: !input.trim() || connectionStatus !== "connected" ? "not-allowed" : "pointer",
            whiteSpace: "nowrap", opacity: !input.trim() || connectionStatus !== "connected" ? 0.5 : 1,
          }}>SEND</button>
        )}
      </div>

      {/* Status bar */}
      <div style={{
        padding: "4px 16px", borderTop: "1px solid var(--border)", display: "flex",
        alignItems: "center", justifyContent: "space-between",
        fontFamily: "'JetBrains Mono', monospace", fontSize: 10, color: "var(--text-dim)",
        letterSpacing: 1, background: "var(--bg-card)", userSelect: "none",
      }}>
        <span>
          {connectionStatus === "connected" ? "\u25CF CONNECTED"
            : connectionStatus === "connecting" ? "\u25CB CONNECTING"
            : connectionStatus === "error" ? "\u25CF ERROR"
            : connectionStatus.startsWith("queued") ? `\u25CB ${connectionStatus.toUpperCase()}`
            : "\u25CB DISCONNECTED"}
        </span>
        {tokenInfo.input > 0 && (
          <span>IN {tokenInfo.input.toLocaleString()} / OUT {tokenInfo.output.toLocaleString()} TOKENS</span>
        )}
      </div>
    </div>
  );
}
