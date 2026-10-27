import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/app.css";
import { App } from "./App";
import { isTauri } from "./api";

// `?theme=dark|light` forces a theme (screenshots, settings later); default follows the OS.
const theme = new URLSearchParams(location.search).get("theme");
if (theme === "dark" || theme === "light") document.documentElement.dataset.theme = theme;

function mount() {
  const root = document.getElementById("root");
  if (root) {
    createRoot(root).render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
  }
}

// `ClipPro --bench-preview` runs the transport benchmark instead of the app (ADR-002).
if (isTauri) {
  void import("./bench").then((b) => b.maybeRunBench()).then((ran) => ran || mount(), mount);
} else {
  mount();
}
