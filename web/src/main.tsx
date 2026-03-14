import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyTheme } from "./theme";
import "./styles.css";

// Apply saved theme or default to dark
const saved = localStorage.getItem("zcgw-theme") as "dark" | "light" | null;
applyTheme(saved ?? "dark");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
