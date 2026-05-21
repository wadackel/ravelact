import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
// Cascade order is load-bearing: ReactFlow lib CSS → Tailwind (preflight
// + utilities + @theme) → project lib overrides (react-flow.css, added
// in T2). The lib CSS must come first so Tailwind preflight does not
// override library defaults, and react-flow.css must come last so
// project overrides win.
import "@xyflow/react/dist/style.css";
import "./index.css";
import "./styles/react-flow.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing from index.html");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
