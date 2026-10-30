// P14.6 accessibility: text tokens meet WCAG AA contrast (4.5:1) on every surface they sit on,
// in both themes; non-text UI colours (accent rings, flags) meet 3:1.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// Vitest runs from app/ui (the CSS `?raw` import comes back empty under its CSS handling).
const css = readFileSync(join(process.cwd(), "src", "styles", "app.css"), "utf8");

type Rgb = [number, number, number];
type Colour = { rgb: Rgb; a: number };

function parse(v: string): Colour {
  const hex = v.match(/^#([0-9a-f]{6})$/i);
  if (hex) return { rgb: [0, 2, 4].map((i) => parseInt(hex[1]!.slice(i, i + 2), 16)) as Rgb, a: 1 };
  const rgba = v.match(/^rgba?\(([^)]+)\)$/);
  if (rgba) {
    const p = rgba[1]!.split(",").map((x) => Number(x.trim()));
    return { rgb: [p[0]!, p[1]!, p[2]!], a: p[3] ?? 1 };
  }
  throw new Error(`unparsed colour ${v}`);
}

function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of block.matchAll(/--c-([\w-]+):\s*([^;]+);/g)) out[m[1]!] = m[2]!.trim();
  return out;
}

const dark = tokens(css.slice(css.indexOf(":root {"), css.indexOf("@media")));
const lightBlock = css.slice(css.indexOf(':root[data-theme="light"]'));
const light = { ...dark, ...tokens(lightBlock.slice(0, lightBlock.indexOf("}"))) };

const over = (fg: Colour, bg: Rgb): Rgb => fg.rgb.map((c, i) => c * fg.a + bg[i]! * (1 - fg.a)) as Rgb;
const lum = (rgb: Rgb) => {
  const l = rgb.map((c) => {
    const x = c / 255;
    return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * l[0]! + 0.7152 * l[1]! + 0.0722 * l[2]!;
};
const ratio = (a: Rgb, b: Rgb) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x! + 0.05) / (y! + 0.05);
};

describe.each([
  ["dark", dark],
  ["light", light],
])("%s theme contrast", (_, t) => {
  const window = parse(t["window"]!).rgb;
  const surfaces: [string, Rgb][] = [
    ["window", window],
    ["raised", parse(t["raised"]!).rgb],
    ["sidebar", over(parse(t["sidebar"]!), window)],
    ["accent-soft", over(parse(t["accent-soft"]!), window)],
  ];

  it.each(["text", "text-2", "text-3", "accent-text"])("%s is AA on every surface", (tok) => {
    for (const [name, bg] of surfaces) {
      if (tok === "text-3" && name === "accent-soft") continue; // tertiary text never sits on a selection
      const r = ratio(over(parse(t[tok]!), bg), bg);
      expect(r, `${tok} on ${name}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("white on the accent fill is AA", () => {
    expect(ratio(parse(t["accent-fg"]!).rgb, parse(t["accent"]!).rgb)).toBeGreaterThanOrEqual(4.5);
  });

  it("status and accent colours are at least 3:1 as UI elements", () => {
    for (const tok of ["accent", "danger", "warning"]) expect(ratio(parse(t[tok]!).rgb, window), tok).toBeGreaterThanOrEqual(3);
  });
});
