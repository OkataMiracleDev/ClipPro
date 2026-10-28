// Keyboard shortcuts (SPEC §14.3, P9.9): every action has a default combo the user can rebind in
// Settings → Keyboard. Handlers ask `actionFor(event)` instead of testing keys themselves.

export type ActionId =
  | "play_pause"
  | "shuttle_back"
  | "shuttle_stop"
  | "shuttle_forward"
  | "prev_frame"
  | "next_frame"
  | "back_second"
  | "forward_second"
  | "go_start"
  | "go_end"
  | "loop"
  | "mark_in"
  | "mark_out"
  | "force"
  | "trim_start"
  | "trim_end"
  | "manual_select"
  | "zebras"
  | "false_colour"
  | "peaking"
  | "focus_map"
  | "motion"
  | "waveform"
  | "split"
  | "inspector"
  | "close_compare"
  | "undo"
  | "redo"
  | "export"
  | "settings";

export const ACTIONS: { id: ActionId; label: string; group: string; key: string }[] = [
  { id: "play_pause", label: "Play / pause", group: "Playback", key: "Space" },
  { id: "shuttle_back", label: "Shuttle backwards (J)", group: "Playback", key: "J" },
  { id: "shuttle_stop", label: "Stop (K)", group: "Playback", key: "K" },
  { id: "shuttle_forward", label: "Shuttle forwards (L)", group: "Playback", key: "L" },
  { id: "prev_frame", label: "Previous frame", group: "Playback", key: "ArrowLeft" },
  { id: "next_frame", label: "Next frame", group: "Playback", key: "ArrowRight" },
  { id: "back_second", label: "Back one second", group: "Playback", key: "Shift+ArrowLeft" },
  { id: "forward_second", label: "Forward one second", group: "Playback", key: "Shift+ArrowRight" },
  { id: "go_start", label: "Go to start", group: "Playback", key: "Home" },
  { id: "go_end", label: "Go to end", group: "Playback", key: "End" },
  { id: "loop", label: "Loop the select", group: "Playback", key: "Ctrl+L" },
  { id: "mark_in", label: "Mark in", group: "Editing", key: "I" },
  { id: "mark_out", label: "Mark out", group: "Editing", key: "O" },
  { id: "force", label: "Force in / out", group: "Editing", key: "F" },
  { id: "trim_start", label: "Trim select start to playhead", group: "Editing", key: "[" },
  { id: "trim_end", label: "Trim select end to playhead", group: "Editing", key: "]" },
  { id: "manual_select", label: "Add a select between the marks", group: "Editing", key: "M" },
  { id: "zebras", label: "Zebras", group: "Viewer", key: "Z" },
  { id: "false_colour", label: "False colour", group: "Viewer", key: "C" },
  { id: "peaking", label: "Focus peaking", group: "Viewer", key: "P" },
  { id: "focus_map", label: "Focus map", group: "Viewer", key: "T" },
  { id: "motion", label: "Motion vectors", group: "Viewer", key: "V" },
  { id: "waveform", label: "Waveform", group: "Viewer", key: "W" },
  { id: "split", label: "Before / after split", group: "Viewer", key: "B" },
  { id: "inspector", label: "Frame inspector", group: "Viewer", key: "Ctrl+I" },
  { id: "close_compare", label: "Close A/B compare", group: "Viewer", key: "Escape" },
  { id: "undo", label: "Undo", group: "General", key: "Ctrl+Z" },
  { id: "redo", label: "Redo", group: "General", key: "Ctrl+Shift+Z" },
  { id: "export", label: "Export…", group: "General", key: "Ctrl+E" },
  { id: "settings", label: "Settings", group: "General", key: "Ctrl+," },
];

const DEFAULTS = Object.fromEntries(ACTIONS.map((a) => [a.id, a.key])) as Record<ActionId, string>;

const NAMED: Record<string, string> = { " ": "Space", Esc: "Escape" };

/** "Ctrl+Shift+Z"-style name of a key event; null for lone modifier presses. */
export function comboOf(e: Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  let k = NAMED[e.key] ?? e.key;
  if (k.length === 1) k = k.toUpperCase();
  // Shifted punctuation keeps its own character ("{"), so Shift is implied, not listed.
  const shift = e.shiftKey && !(k.length === 1 && !/[A-Z0-9]/.test(k));
  return [e.ctrlKey || e.metaKey ? "Ctrl" : "", e.altKey ? "Alt" : "", shift ? "Shift" : "", k].filter(Boolean).join("+");
}

export function bindingsWith(custom: Record<string, string> | undefined | null): Record<ActionId, string> {
  return { ...DEFAULTS, ...(custom ?? {}) } as Record<ActionId, string>;
}

let current: Record<ActionId, string> = { ...DEFAULTS };
let byCombo = new Map<string, ActionId>();
const rebuild = () => {
  byCombo = new Map();
  for (const a of ACTIONS) if (current[a.id]) byCombo.set(current[a.id], a.id);
};
rebuild();

/** Installs the user's bindings (from app settings). */
export function setBindings(custom: Record<string, string> | undefined | null) {
  current = bindingsWith(custom);
  rebuild();
}

export function keyFor(id: ActionId): string {
  return current[id] ?? "";
}

export function actionFor(e: KeyboardEvent): ActionId | null {
  const c = comboOf(e);
  return c ? (byCombo.get(c) ?? null) : null;
}

/** Actions other than `id` already bound to `combo`. */
export function conflicts(bindings: Record<ActionId, string>, id: ActionId, combo: string): ActionId[] {
  return ACTIONS.filter((a) => a.id !== id && bindings[a.id] === combo).map((a) => a.id);
}

/** Only the bindings that differ from the defaults (what settings store). */
export function customOnly(bindings: Record<ActionId, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(bindings).filter(([k, v]) => DEFAULTS[k as ActionId] !== v));
}

export const isTyping = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));
