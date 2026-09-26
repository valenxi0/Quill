import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import App from "./App";

// In a plain browser (no Tauri), preview against a fake backend.
if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installDevMock } = await import("../lib/devMock");
  installDevMock();
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
