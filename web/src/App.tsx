import { useState, useCallback, useEffect, useRef } from "react";
import { Sidebar } from "./components/Sidebar";
import { Header } from "./components/Header";
import { Toast } from "./components/Toast";
import { Login } from "./pages/Login";
import { Chat } from "./pages/Chat";
import Config from "./pages/Config";
import Memory from "./pages/Memory";
import Tools from "./pages/Tools";
import Status from "./pages/Status";
import { useToast } from "./hooks/useToast";
import { getToken, setToken, restoreToken, listInstances } from "./api";
import type { InstanceInfo } from "./api";

export type View = "chat" | "config" | "memory" | "tools" | "status";

export function App() {
  const [loggedIn, setLoggedIn] = useState(false);
  const [instances, setInstances] = useState<InstanceInfo[]>([]);
  const [currentInstance, setCurrentInstance] = useState("");
  const [view, setView] = useState<View>("chat");
  const [sidebarExpanded, setSidebarExpanded] = useState(true);
  const { toast, show: showToast } = useToast();
  const autoLoginAttempted = useRef(false);

  // Auto-login from ?token= URL parameter or saved cookie
  useEffect(() => {
    if (autoLoginAttempted.current || loggedIn) return;
    autoLoginAttempted.current = true;

    const params = new URLSearchParams(window.location.search);
    const urlToken = params.get("token");
    const savedToken = restoreToken();
    const token = urlToken || savedToken;
    if (!token) return;

    setToken(token);
    listInstances()
      .then((insts) => {
        setLoggedIn(true);
        setInstances(insts);
        if (insts.length > 0 && insts[0]) {
          setCurrentInstance(insts[0].id);
        }
        // Clean the token from the URL
        if (urlToken) {
          const url = new URL(window.location.href);
          url.searchParams.delete("token");
          window.history.replaceState({}, "", url.toString());
        }
      })
      .catch(() => {
        // Token invalid, fall back to manual login
        setToken("");
      });
  }, [loggedIn]);

  const handleLogin = useCallback((insts: InstanceInfo[]) => {
    setLoggedIn(true);
    setInstances(insts);
    if (insts.length > 0 && insts[0]) {
      setCurrentInstance(insts[0].id);
    }
  }, []);

  const handleLogout = useCallback(() => {
    setToken("");
    setLoggedIn(false);
    setInstances([]);
    setCurrentInstance("");
  }, []);

  if (!loggedIn || !getToken()) {
    return (
      <div style={{ display: "flex", height: "100vh" }}>
        <Sidebar
          view={view}
          onViewChange={setView}
          expanded={sidebarExpanded}
          onToggle={() => setSidebarExpanded((e) => !e)}
          health="unknown"
          instanceSelected={false}
          instances={[]}
          currentInstance=""
          onInstanceChange={() => {}}
        />
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minWidth: 0 }}>
          <Header view={view} />
          <Login onLogin={handleLogin} />
        </div>
        <Toast toast={toast} />
      </div>
    );
  }

  const health = instances.find((i) => i.id === currentInstance)?.health ?? "unknown";

  const renderView = () => {
    switch (view) {
      case "chat":
        return <Chat instanceId={currentInstance} toast={showToast} />;
      case "config":
        return <Config instanceId={currentInstance} toast={showToast} />;
      case "memory":
        return <Memory instanceId={currentInstance} toast={showToast} />;
      case "tools":
        return <Tools instanceId={currentInstance} toast={showToast} />;
      case "status":
        return (
          <Status
            instanceId={currentInstance}
            toast={showToast}
            onLogout={handleLogout}
          />
        );
    }
  };

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <Sidebar
        view={view}
        onViewChange={setView}
        expanded={sidebarExpanded}
        onToggle={() => setSidebarExpanded((e) => !e)}
        health={health}
        instanceSelected={!!currentInstance}
        instances={instances}
        currentInstance={currentInstance}
        onInstanceChange={setCurrentInstance}
      />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minWidth: 0 }}>
        <Header view={view} />
        <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}>
          {currentInstance ? (
            renderView()
          ) : (
            <div
              style={{
                flex: 1,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "var(--text-dim)",
                fontFamily: "'JetBrains Mono', monospace",
                fontSize: 12,
                letterSpacing: 1,
              }}
            >
              SELECT AN INSTANCE TO BEGIN
            </div>
          )}
        </div>
      </div>
      <Toast toast={toast} />
    </div>
  );
}
