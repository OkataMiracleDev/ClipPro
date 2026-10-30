/// <reference types="vite/client" />
// The in-app manual (P14.4): docs/manual/*.md, bundled at build time, and its search.

import { parse, plain, type Block } from "./markdown";

const raw = import.meta.glob("../../../../docs/manual/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

export interface Page {
  /** File name, e.g. "03-tuning.md". */
  file: string;
  title: string;
  blocks: Block[];
}

export const PAGES: Page[] = Object.entries(raw)
  .map(([path, text]) => {
    const file = path.split("/").pop()!;
    const blocks = parse(text);
    const h1 = blocks.find((b) => b.kind === "h" && b.level === 1);
    return { file, title: h1 && h1.kind === "h" ? h1.text : file, blocks };
  })
  // Numbered pages in order; "What's new" last.
  .sort((a, b) => (a.file.startsWith("whats-new") ? 1 : 0) - (b.file.startsWith("whats-new") ? 1 : 0) || a.file.localeCompare(b.file));

export interface Hit {
  page: Page;
  /** The heading the match sits under (for scrolling), if any. */
  heading: string | null;
  headingId: string | null;
  snippet: string;
}

function texts(b: Block): string[] {
  switch (b.kind) {
    case "h":
    case "p":
    case "code":
      return [b.text];
    case "ul":
    case "ol":
      return b.items;
    case "table":
      return [...b.head, ...b.rows.map((r) => r.join(" · "))];
  }
}

/** Every passage containing all the words of `query` (case-insensitive), best pages first. */
export function search(query: string, pages: Page[] = PAGES, limit = 40): Hit[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const hits: (Hit & { score: number })[] = [];
  for (const page of pages) {
    let heading: string | null = null;
    let headingId: string | null = null;
    for (const b of page.blocks) {
      if (b.kind === "h" && b.level > 1) {
        heading = b.text;
        headingId = b.id;
      }
      for (const t of texts(b)) {
        const p = plain(t);
        const lower = p.toLowerCase();
        // A passage matches when every word is in it or in the heading and page it sits under.
        const context = `${page.title} ${heading ?? ""} ${p}`.toLowerCase();
        if (!words.every((w) => context.includes(w)) || !words.some((w) => lower.includes(w))) continue;
        const first = words.find((w) => lower.includes(w))!;
        const at = lower.indexOf(first);
        const from = Math.max(0, at - 50);
        const snippet = `${from > 0 ? "…" : ""}${p.slice(from, at + 110)}${at + 110 < p.length ? "…" : ""}`;
        // Titles and headings rank above body text.
        const score = (b.kind === "h" ? 3 : 1) + (page.title.toLowerCase().includes(words[0]!) ? 2 : 0);
        hits.push({ page, heading: b.kind === "h" ? b.text : heading, headingId: b.kind === "h" ? b.id : headingId, snippet, score });
      }
    }
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, limit);
}
