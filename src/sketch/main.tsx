import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import { api } from "../lib/api";
import Bar from "./Bar";
import Ink from "./Ink";

const mode = new URLSearchParams(location.search).get("mode");

if ("__TAURI_INTERNALS__" in window) {
  api.animationsEnabled().then((on) => document.documentElement.classList.toggle("reduced-motion", !on));
} else if (import.meta.env.DEV) {
  // Browser preview: a grey desktop behind the window.
  document.body.style.background = "#6b7280";
}

createRoot(document.getElementById("root")!).render(<StrictMode>{mode === "bar" ? <Bar /> : <Ink />}</StrictMode>);
