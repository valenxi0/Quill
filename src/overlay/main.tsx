import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";
import Overlay from "./Overlay";
import type { DictationView } from "../lib/api";

let previewView: DictationView | undefined;

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installDevMock } = await import("../lib/devMock");
  await installDevMock();
  document.body.style.background = "#6b7280";
  const preview = new URLSearchParams(location.search).get("preview");
  if (preview === "recording" || preview === "transcribing" || preview === "cleaning" || preview === "cancelled") {
    const { emit } = await import("@tauri-apps/api/event");
    previewView = { phase: preview, context: "general", outcome: null, errorTitle: null, errorDetail: null };
    if (preview === "recording") {
      let sample = 0;
      setInterval(() => emit("level", 0.1 + Math.max(0, Math.sin(sample++ * 0.6)) * 0.6), 100);
      // Fake live words, one every 350 ms, looping.
      const words = "Uh, a voice-to-text dictation app with live transcription and edits, running on my own models.".split(" ");
      let n = 0;
      setTimeout(() => setInterval(() => emit("preview", words.slice(0, (n++ % (words.length + 4)) + 1).join(" ")), 350), 1200);
    }
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Overlay preview={previewView} />
  </StrictMode>,
);
