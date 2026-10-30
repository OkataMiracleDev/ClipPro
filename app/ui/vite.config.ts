import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri expects a fixed dev port and no screen clearing.
// `--mode preview` builds one self-contained bundle into target/preview/dist for
// app/design/build_preview.py (the single-file review page).
export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  // The in-app manual is bundled from docs/manual (P14.4).
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] }, fs: { allow: [".", "../../docs/manual"] } },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build:
    mode === "preview"
      ? { target: "es2022", outDir: "../../target/preview/dist", emptyOutDir: true, sourcemap: false, rollupOptions: { output: { inlineDynamicImports: true } } }
      : { target: "es2022", outDir: "dist", sourcemap: false },
  test: { environment: "jsdom", globals: true, setupFiles: ["./src/test/setup.ts"] },
}));
