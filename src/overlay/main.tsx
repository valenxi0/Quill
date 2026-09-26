import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import Overlay from "./Overlay";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installDevMock } = await import("../lib/devMock");
  installDevMock();
  document.body.style.background = "#6b7280";
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Overlay />
  </StrictMode>,
);
