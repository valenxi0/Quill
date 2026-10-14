import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { execFile } from "node:child_process";
import { fileURLToPath } from "node:url";

const previewIcons = {
  name: "quill-preview-app-icons",
  configureServer(server: import("vite").ViteDevServer) {
    server.middlewares.use("/__quill_preview_apps", (_request, response) => {
      const script = fileURLToPath(new URL("./scripts/preview-app-icons.ps1", import.meta.url));
      execFile("powershell.exe", ["-NoProfile", "-NonInteractive", "-File", script],
        { windowsHide: true, maxBuffer: 4 * 1024 * 1024, timeout: 10000 }, (error, stdout) => {
          response.setHeader("Content-Type", "application/json");
          response.setHeader("Cache-Control", "no-store");
          response.statusCode = error ? 503 : 200;
          response.end(error ? "[]" : stdout);
        });
    });
  },
};

export default defineConfig({
  plugins: [react(), tailwindcss(), previewIcons],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        settings: "index.html",
        overlay: "overlay.html",
      },
    },
  },
});
