import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/app.css";
import { App } from "./App";
import { SettingsWindow } from "./views/SettingsWindow";
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
// `?preset=Pan` starts with that preset (screenshots).
const preset = q.get("preset");
if (preset) {
  const stop = useApp.subscribe((s) => {
    if (!s.project) return;
    stop();
    void s.setPreset(preset);
  });
}

function mount() {
  const root = document.getElementById("root");
  if (root) {
    // `?window=settings` is the Settings window (opened by the backend's open_settings).
    const settings = q.get("window") === "settings";
    createRoot(root).render(<StrictMode>{settings ? <SettingsWindow /> : <App />}</StrictMode>);
  }
}

// `ClipPro --bench-preview` runs the transport benchmark instead of the app (ADR-002).
if (isTauri) {
  void import("./bench").then((b) => b.maybeRunBench()).then((ran) => ran || mount(), mount);
} else {
  mount();
}

// `?sync=1` syncs three demo clips and shows the result (screenshots).
if (q.get("sync") === "1") {
  const stop = useApp.subscribe((s) => {
    if (s.clips.length < 8 || !s.scored) return;
    stop();
    const ids = s.clips.filter((c) => /INTERVIEW|A001C00[34]/.test(c.name)).map((c) => c.clip);
    void import("./views/SyncGroups").then(({ useSync }) =>
      import("./api").then(({ api }) => api.syncClips("Sarah interview", ids, "audio").then((g) => useSync.getState().show([], g))),
    );
  });
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
