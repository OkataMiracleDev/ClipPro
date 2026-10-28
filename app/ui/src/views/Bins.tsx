// Bins and smart collections in the sidebar (SPEC P9.8), and the smart-collection query builder.

import * as CM from "@radix-ui/react-context-menu";
import { Folder, FolderPlus, Plus, Sparkles, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Button, cx, Segmented, Sheet } from "../components/ui";
import { FLAGS, ACTIVE_FLAGS } from "../lib/domain";
import { describeRule, factsFor, matches, type Rule, type SmartQuery } from "../lib/smart";
import { useApp } from "../store";
import { LABELS } from "./SelectEditor";

export function parseQuery(q: string | null): SmartQuery {
  try {
    const v = q ? (JSON.parse(q) as SmartQuery) : null;
    if (v && Array.isArray(v.rules)) return v;
  } catch {
    /* fall through */
  }
  return { match: "all", rules: [] };
}

/** Clips the sidebar selection shows. */
export function useLibraryClips() {
  const { clips, scored, annotations, library, bins } = useApp();
  return useMemo(() => {
    if (library.kind === "all") return clips;
    if (library.kind === "bin") return clips.filter((c) => c.bin === library.id);
    const bin = bins.find((b) => b.id === library.id);
    const q = parseQuery(bin?.query ?? null);
    return factsFor(clips, scored, annotations)
      .filter((f) => matches(q, f))
      .map((f) => f.clip);
  }, [clips, scored, annotations, library, bins]);
}

const FIELDS: { value: Rule["field"]; label: string }[] = [
  { value: "flag", label: "Flag" },
  { value: "score", label: "Best score" },
  { value: "duration", label: "Length (s)" },
  { value: "selects", label: "Selects" },
  { value: "rating", label: "Rating" },
  { value: "label", label: "Colour label" },
  { value: "camera", label: "Camera" },
  { value: "name", label: "Name" },
  { value: "codec", label: "Codec" },
  { value: "date", label: "Recorded" },
];

function defaultRule(field: Rule["field"]): Rule {
  switch (field) {
    case "flag":
      return { field, op: "lacks", value: "shaky" };
    case "score":
      return { field, op: ">=", value: 60 };
    case "duration":
      return { field, op: ">=", value: 10 };
    case "selects":
      return { field, op: ">=", value: 1 };
    case "rating":
      return { field, op: ">=", value: 3 };
    case "label":
      return { field, op: "is", value: "green" };
    case "camera":
    case "name":
      return { field, op: "contains", value: "" };
    case "codec":
      return { field, op: "is", value: "prores" };
    case "date":
      return { field, op: "after", value: new Date().toISOString().slice(0, 10) };
  }
}

const ctl = "h-6 rounded border border-separator-strong bg-control px-1 text-[12px]";

function RuleRow({ r, onChange, onRemove }: { r: Rule; onChange: (r: Rule) => void; onRemove: () => void }) {
  return (
    <div className="flex items-center gap-1">
      <select aria-label="Field" className={ctl} value={r.field} onChange={(e) => onChange(defaultRule(e.target.value as Rule["field"]))}>
        {FIELDS.map((f) => (
          <option key={f.value} value={f.value}>
            {f.label}
          </option>
        ))}
      </select>
      {r.field === "flag" && (
        <>
          <select aria-label="Condition" className={ctl} value={r.op} onChange={(e) => onChange({ ...r, op: e.target.value as "has" | "lacks" })}>
            <option value="has">has</option>
            <option value="lacks">has no</option>
          </select>
          <select aria-label="Flag" className={cx(ctl, "min-w-0 flex-1")} value={r.value} onChange={(e) => onChange({ ...r, value: e.target.value })}>
            {FLAGS.filter((f) => ACTIVE_FLAGS.has(f.key)).map((f) => (
              <option key={f.key} value={f.key}>
                {f.label}
              </option>
            ))}
          </select>
        </>
      )}
      {(r.field === "score" || r.field === "duration" || r.field === "selects" || r.field === "rating") && (
        <>
          <select aria-label="Comparison" className={ctl} value={r.op} onChange={(e) => onChange({ ...r, op: e.target.value as ">=" | "<=" })}>
            <option value=">=">at least</option>
            <option value="<=">at most</option>
          </select>
          <input aria-label="Value" type="number" className={cx(ctl, "w-16")} value={r.value} onChange={(e) => onChange({ ...r, value: Number(e.target.value) })} />
        </>
      )}
      {(r.field === "camera" || r.field === "name") && (
        <input aria-label="Contains" placeholder="contains…" className={cx(ctl, "min-w-0 flex-1")} value={r.value} onChange={(e) => onChange({ ...r, value: e.target.value })} />
      )}
      {r.field === "codec" && <input aria-label="Codec" className={cx(ctl, "min-w-0 flex-1")} value={r.value} onChange={(e) => onChange({ ...r, value: e.target.value })} />}
      {r.field === "label" && (
        <select aria-label="Label" className={cx(ctl, "min-w-0 flex-1")} value={r.value} onChange={(e) => onChange({ ...r, value: e.target.value })}>
          {LABELS.map((l) => (
            <option key={l.name} value={l.name}>
              {l.name}
            </option>
          ))}
        </select>
      )}
      {r.field === "date" && (
        <>
          <select aria-label="When" className={ctl} value={r.op} onChange={(e) => onChange({ ...r, op: e.target.value as "after" | "before" })}>
            <option value="after">on or after</option>
            <option value="before">on or before</option>
          </select>
          <input aria-label="Date" type="date" className={ctl} value={r.value} onChange={(e) => onChange({ ...r, value: e.target.value })} />
        </>
      )}
      <button aria-label="Remove condition" className="rounded p-0.5 text-text-3 hover:text-text" onClick={onRemove}>
        <X size={12} />
      </button>
    </div>
  );
}

export function SmartEditor({ open, edit, onClose }: { open: boolean; edit: { id: number; name: string; query: SmartQuery } | null; onClose: () => void }) {
  const { addBin, updateBin, clips, scored, annotations } = useApp();
  const [name, setName] = useState(edit?.name ?? "");
  const [q, setQ] = useState<SmartQuery>(edit?.query ?? { match: "all", rules: [defaultRule("score")] });
  const count = factsFor(clips, scored, annotations).filter((f) => matches(q, f)).length;
  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()} title={edit ? "Edit smart collection" : "New smart collection"}>
      <div className="flex flex-col gap-3">
        <input autoFocus aria-label="Name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} className="h-7 rounded border border-separator-strong bg-control px-2 text-[13px]" />
        <div className="flex items-center gap-2 text-[12px] text-text-2">
          Match
          <Segmented
            size="sm"
            label="Match"
            value={q.match}
            onChange={(match) => setQ({ ...q, match })}
            options={[
              { value: "all", label: "all" },
              { value: "any", label: "any" },
            ]}
          />
          of these conditions
        </div>
        <div className="flex flex-col gap-1.5">
          {q.rules.map((r, i) => (
            <RuleRow key={i} r={r} onChange={(nr) => setQ({ ...q, rules: q.rules.map((x, j) => (j === i ? nr : x)) })} onRemove={() => setQ({ ...q, rules: q.rules.filter((_, j) => j !== i) })} />
          ))}
          <button className="flex items-center gap-1 self-start text-[12px] text-accent" onClick={() => setQ({ ...q, rules: [...q.rules, defaultRule("flag")] })}>
            <Plus size={12} /> Add condition
          </button>
        </div>
        <div className="flex items-center justify-between border-t border-separator pt-3">
          <span className="text-[12px] text-text-3">
            {count} clip{count === 1 ? "" : "s"} match{count === 1 ? "es" : ""}
          </span>
          <div className="flex gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!name.trim()}
              onClick={() => {
                void (edit ? updateBin(edit.id, name, q) : addBin(name, "smart", q));
                onClose();
              }}
            >
              {edit ? "Save" : "Create"}
            </Button>
          </div>
        </div>
      </div>
    </Sheet>
  );
}

export function BinsSection({ item }: { item: (active: boolean) => string }) {
  const { bins, clips, library, setLibrary, addBin, deleteBin, updateBin, moveToBin } = useApp();
  const [smart, setSmart] = useState<{ id: number; name: string; query: SmartQuery } | null | "new">(null);
  const [naming, setNaming] = useState<{ id: number | null; name: string } | null>(null);
  const counts = useMemo(() => {
    const m = new Map<number, number>();
    for (const c of clips) if (c.bin !== null) m.set(c.bin, (m.get(c.bin) ?? 0) + 1);
    return m;
  }, [clips]);
  const commitName = () => {
    if (!naming) return;
    const nm = naming.name.trim();
    if (nm) {
      if (naming.id === null) void addBin(nm, "manual");
      else {
        const b = bins.find((x) => x.id === naming.id);
        void updateBin(naming.id, nm, b?.query ? JSON.parse(b.query) : undefined);
      }
    }
    setNaming(null);
  };
  return (
    <div>
      <div className="mb-1 flex items-center justify-between px-2">
        <span className="section-header">Bins</span>
        <span className="flex items-center gap-0.5">
          <button aria-label="New bin" title="New bin" className="rounded p-0.5 text-text-3 hover:bg-control-hover hover:text-text" onClick={() => setNaming({ id: null, name: "" })}>
            <FolderPlus size={13} />
          </button>
          <button aria-label="New smart collection" title="New smart collection" className="rounded p-0.5 text-text-3 hover:bg-control-hover hover:text-text" onClick={() => setSmart("new")}>
            <Sparkles size={13} />
          </button>
        </span>
      </div>
      {naming && naming.id === null && (
        <input
          autoFocus
          aria-label="Bin name"
          placeholder="Bin name"
          value={naming.name}
          onChange={(e) => setNaming({ ...naming, name: e.target.value })}
          onBlur={commitName}
          onKeyDown={(e) => (e.key === "Enter" ? commitName() : e.key === "Escape" && setNaming(null))}
          className="mx-2 mb-1 h-6 w-[calc(100%-1rem)] rounded border border-accent bg-control px-1.5 text-[12px]"
        />
      )}
      {bins.map((b) => {
        const active = library.kind !== "all" && library.id === b.id;
        const isSmart = b.kind === "smart";
        return (
          <CM.Root key={b.id}>
            <CM.Trigger asChild>
              <div
                className={item(active)}
                role="button"
                tabIndex={0}
                onClick={() => setLibrary({ kind: isSmart ? "smart" : "bin", id: b.id })}
                title={isSmart ? `${parseQuery(b.query).match === "all" ? "All of" : "Any of"}: ${parseQuery(b.query).rules.map(describeRule).join(" · ")}` : undefined}
                onDragOver={(e) => !isSmart && e.dataTransfer.types.includes("application/x-clippro-clips") && e.preventDefault()}
                onDrop={(e) => {
                  const ids = JSON.parse(e.dataTransfer.getData("application/x-clippro-clips") || "[]") as number[];
                  if (!isSmart && ids.length) void moveToBin(ids, b.id);
                }}
                onDoubleClick={() => (isSmart ? setSmart({ id: b.id, name: b.name, query: parseQuery(b.query) }) : setNaming({ id: b.id, name: b.name }))}
              >
                {isSmart ? <Sparkles size={14} /> : <Folder size={14} />}
                {naming?.id === b.id ? (
                  <input
                    autoFocus
                    aria-label="Rename bin"
                    value={naming.name}
                    onChange={(e) => setNaming({ ...naming, name: e.target.value })}
                    onBlur={commitName}
                    onKeyDown={(e) => (e.key === "Enter" ? commitName() : e.key === "Escape" && setNaming(null))}
                    className="h-5 min-w-0 flex-1 rounded border border-accent bg-control px-1 text-[12px]"
                  />
                ) : (
                  <span className="min-w-0 flex-1 truncate">{b.name}</span>
                )}
                {!isSmart && <span className="font-mono text-[11px] text-text-3 tabular">{counts.get(b.id) ?? 0}</span>}
              </div>
            </CM.Trigger>
            <CM.Portal>
              <CM.Content className="z-50 min-w-40 rounded-lg bg-raised p-1 shadow-(--shadow-float) ring-1 ring-separator-strong">
                {(isSmart
                  ? [
                      ["Edit conditions…", () => setSmart({ id: b.id, name: b.name, query: parseQuery(b.query) })],
                      ["Delete collection", () => void deleteBin(b.id)],
                    ]
                  : [
                      ["Rename", () => setNaming({ id: b.id, name: b.name })],
                      ["Delete bin (keeps its clips)", () => void deleteBin(b.id)],
                    ]
                ).map(([label, fn]) => (
                  <CM.Item key={label as string} className="flex h-6 items-center rounded px-2 text-[13px] outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-fg" onSelect={fn as () => void}>
                    {label as string}
                  </CM.Item>
                ))}
              </CM.Content>
            </CM.Portal>
          </CM.Root>
        );
      })}
      {bins.length === 0 && !naming && <div className="px-2 text-[11px] text-text-3">Group clips into bins, or collect them by rule.</div>}
      {smart && <SmartEditor key={smart === "new" ? "new" : smart.id} open edit={smart === "new" ? null : smart} onClose={() => setSmart(null)} />}
    </div>
  );
}
