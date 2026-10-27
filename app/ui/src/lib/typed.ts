// Typed slider values (SPEC §14.2). Mirrors crates/clippro-types/src/typed.rs; both are
// checked against testdata/typed_values.json.

const UNLIMITED = new Set(["none", "inf", "infinity", "∞", "unlimited", "no limit", "max"]);

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

function num(s: string): number | null {
  if (s === "" || s.startsWith("+")) return null;
  if (!/^-?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i.test(s)) return null;
  const v = Number(s);
  return Number.isFinite(v) ? v : null;
}

/** Duration in seconds; `null` value means unlimited. */
export function parseDuration(input: string): Parsed<number | null> {
  const s = input.trim().toLowerCase();
  const bad = (): Parsed<number | null> => ({ ok: false, error: `couldn't read '${input.trim()}'` });
  if (s === "") return { ok: false, error: "empty value" };
  if (UNLIMITED.has(s)) return { ok: true, value: null };
  if (s.includes(":")) {
    const parts = s.split(":");
    if (parts.length > 3 || parts.some((p) => p === "")) return bad();
    let total = 0;
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i]!;
      const last = i === parts.length - 1;
      let v: number | null;
      if (last) v = num(p);
      else v = /^\d+$/.test(p) ? Number(p) : null;
      if (v === null) return bad();
      if (v < 0 || (i > 0 && v >= 60)) return { ok: false, error: `'${input.trim()}' is out of range` };
      total = total * 60 + v;
    }
    return { ok: true, value: total };
  }
  const n = num(s);
  if (n !== null) {
    if (n < 0) return { ok: false, error: `'${input.trim()}' is out of range` };
    return { ok: true, value: n };
  }
  const compact = s.replace(/\s+/g, "");
  const re = /(\d+(?:\.\d+)?|\.\d+)([a-z]*)/y;
  let total = 0;
  let any = false;
  let pos = 0;
  while (pos < compact.length) {
    re.lastIndex = pos;
    const m = re.exec(compact);
    if (!m) return bad();
    const v = Number(m[1]);
    const unit = m[2] ?? "";
    const mult =
      unit === "ms"
        ? 0.001
        : ["s", "sec", "secs", "second", "seconds"].includes(unit)
          ? 1
          : ["m", "min", "mins", "minute", "minutes"].includes(unit)
            ? 60
            : ["h", "hr", "hrs", "hour", "hours"].includes(unit)
              ? 3600
              : null;
    if (mult === null || !Number.isFinite(v)) return bad();
    total += v * mult;
    any = true;
    pos = re.lastIndex;
  }
  return any ? { ok: true, value: total } : bad();
}

/** Count; `null` means unlimited. */
export function parseCount(input: string): Parsed<number | null> {
  const s = input.trim().toLowerCase();
  if (s === "") return { ok: false, error: "empty value" };
  if (UNLIMITED.has(s)) return { ok: true, value: null };
  if (!/^\d+$/.test(s)) return { ok: false, error: `couldn't read '${input.trim()}'` };
  const v = Number(s);
  return v <= 0xffffffff ? { ok: true, value: v } : { ok: false, error: `'${input.trim()}' is out of range` };
}

/** Score typed as 0–100 (optional %), returned as a fraction. */
export function parseScore(input: string): Parsed<number> {
  const s = input.trim().replace(/%$/, "").trim();
  if (s === "") return { ok: false, error: "empty value" };
  const v = num(s);
  if (v === null) return { ok: false, error: `couldn't read '${input.trim()}'` };
  if (v < 0 || v > 100) return { ok: false, error: `'${input.trim()}' is out of range` };
  return { ok: true, value: v / 100 };
}

/** Display form used by sliders: `0.25s`, `4s`, `1:30`, `1:02:03`, `none`. */
export function formatDuration(secs: number | null): string {
  if (secs === null) return "none";
  if (secs < 60) {
    const r = Math.round(secs * 100) / 100;
    return `${Number.isInteger(r) ? r.toFixed(0) : String(r)}s`;
  }
  const t = Math.round(secs);
  const h = Math.floor(t / 3600);
  const m = Math.floor(t / 60) % 60;
  const s = t % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Timecode-ish display of microseconds: `MM:SS.cc` or `H:MM:SS`. */
export function formatClock(us: number): string {
  const t = Math.max(0, us) / 1e6;
  const h = Math.floor(t / 3600);
  const m = Math.floor(t / 60) % 60;
  const s = t % 60;
  const pad = (n: number, w = 2) => String(Math.floor(n)).padStart(w, "0");
  if (h > 0) return `${h}:${pad(m)}:${pad(s)}`;
  return `${pad(m)}:${pad(s)}.${pad((s % 1) * 100)}`;
}
