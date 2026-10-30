// Settings → Keyboard (SPEC OTH-1, §14.3, P9.9): rebind any shortcut. Click a key, press the new
// combination; a combination already in use is reported with the option to swap.

import { RotateCcw } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSettings } from "../api";
import { cx } from "../components/ui";
import { ACTIONS, bindingsWith, comboOf, conflicts, customOnly, type ActionId } from "../lib/keys";

export function KeyboardTab({ s, update }: { s: AppSettings; update: (p: Partial<AppSettings>) => void }) {
  const bindings = bindingsWith(s.keybindings);
  const [recording, setRecording] = useState<ActionId | null>(null);
  const [clash, setClash] = useState<{ id: ActionId; combo: string; others: ActionId[] } | null>(null);
  const save = (b: Record<ActionId, string>) => update({ keybindings: customOnly(b) });

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape" && !e.ctrlKey && !e.shiftKey) return setRecording(null);
      const combo = comboOf(e);
      if (!combo) return;
      const others = conflicts(bindings, recording, combo);
      if (others.length) setClash({ id: recording, combo, others });
      else save({ ...bindings, [recording]: combo });
      setRecording(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording, s.keybindings]);

  const label = (id: ActionId) => ACTIONS.find((a) => a.id === id)!.label;
  const groups = [...new Set(ACTIONS.map((a) => a.group))];
  return (
    <div className="flex flex-col gap-3">
      {clash && (
        <div className="flex items-center justify-between gap-2 rounded-md bg-warning/15 px-3 py-2 text-[12px] ring-1 ring-warning/40">
          <span>
            <b>{clash.combo}</b> is used by {clash.others.map(label).join(", ")}.
          </span>
          <span className="flex gap-1">
            <button
              className="rounded px-2 py-0.5 ring-1 ring-separator-strong hover:bg-control-hover"
              onClick={() => {
                // Swap: the other action takes this action's old combination.
                const b = { ...bindings, [clash.id]: clash.combo };
                for (const o of clash.others) b[o] = bindings[clash.id];
                save(b);
                setClash(null);
              }}
            >
              Swap
            </button>
            <button className="rounded px-2 py-0.5 hover:bg-control-hover" onClick={() => setClash(null)}>
              Cancel
            </button>
          </span>
        </div>
      )}
      {groups.map((g) => (
        <section key={g}>
          <h3 className="section-header mb-1">{g}</h3>
          {ACTIONS.filter((a) => a.group === g).map((a) => {
            const custom = bindings[a.id] !== a.key;
            return (
              <div key={a.id} className="flex h-7 items-center justify-between gap-2 border-b border-separator text-[13px]">
                <span className="text-text-2">{a.label}</span>
                <span className="flex items-center gap-1">
                  {custom && (
                    <button aria-label={`Reset ${a.label}`} title={`Reset to ${a.key}`} className="rounded p-0.5 text-text-3 hover:text-text" onClick={() => save({ ...bindings, [a.id]: a.key })}>
                      <RotateCcw size={12} />
                    </button>
                  )}
                  <button
                    onClick={() => setRecording(recording === a.id ? null : a.id)}
                    aria-label={`Shortcut for ${a.label}: ${bindings[a.id]}. Click to change.`}
                    className={cx(
                      "min-w-24 rounded px-2 py-0.5 text-right font-mono text-[11.5px] ring-1",
                      recording === a.id ? "bg-accent text-accent-fg ring-accent" : custom ? "ring-accent/60" : "ring-separator-strong",
                    )}
                  >
                    {recording === a.id ? "Press keys…" : bindings[a.id] || "—"}
                  </button>
                </span>
              </div>
            );
          })}
        </section>
      ))}
      <button className="self-start text-[12px] text-accent-text" onClick={() => update({ keybindings: {} })}>
        Reset all shortcuts to the defaults
      </button>
    </div>
  );
}
