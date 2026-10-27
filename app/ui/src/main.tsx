import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/app.css";
import { App } from "./App";

// `?theme=dark|light` forces a theme (screenshots, settings later); default follows the OS.
const theme = new URLSearchParams(location.search).get("theme");
if (theme === "dark" || theme === "light") document.documentElement.dataset.theme = theme;

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
