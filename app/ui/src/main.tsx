import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/app.css";
import { App } from "./App";
import { isTauri } from "./api";
import { useApp } from "./store";

// `?theme=dark|light` forces a theme (screenshots, settings later); default follows the OS.
const theme = new URLSearchParams(location.search).get("theme");
if (theme === "dark" || theme === "light") document.documentElement.dataset.theme = theme;
// Screenshot state: `?overlay=zebras|false_colour|peaking`, `?measure=tiles|motion`, `?panel=frame`.
const q = new URLSearchParams(location.search);
const overlay = q.get("overlay");
if (overlay === "zebras" || overlay === "false_colour" || overlay === "peaking") useApp.setState({ overlay });
const measure = q.get("measure");
if (measure === "tiles" || measure === "motion") useApp.setState({ measureOverlay: measure });
if (q.get("panel") === "frame") useApp.setState({ rightTab: "frame" });
if (q.get("scope") === "1") useApp.setState({ scope: true });

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

// `?compare=1` opens the A/B compare with the second select once scores arrive (screenshots).
if (q.get("compare") === "1") {
  const stop = useApp.subscribe((s) => {
    const sel = s.scored?.selects;
    if (!sel || sel.length < 2 || s.selectedClip === null) return;
    stop();
    const first = sel.find((x) => x.clip === s.selectedClip) ?? sel[0]!;
    const other = sel.find((x) => x.clip !== first.clip) ?? sel[1]!;
    useApp.setState({ playheadUs: first.segment.startUs, compare: { clip: other.clip, startUs: other.segment.startUs, endUs: other.segment.endUs, anchorUs: first.segment.startUs } });
  });
}
