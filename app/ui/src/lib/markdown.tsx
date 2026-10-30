// A small Markdown renderer for the in-app manual (P14.4): headings, paragraphs, lists, pipe
// tables, fenced code, `code`, **bold**, *italic* and links. It builds React elements, so text is
// never interpreted as HTML.

import type { ReactNode } from "react";

export type Block =
  | { kind: "h"; level: number; text: string; id: string }
  | { kind: "p"; text: string }
  | { kind: "ul" | "ol"; items: string[] }
  | { kind: "table"; head: string[]; rows: string[][] }
  | { kind: "code"; text: string };

export function slug(s: string): string {
  return s
    .toLowerCase()
    .replace(/[`*]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replace(/\\\|/g, "|"));

export function parse(md: string): Block[] {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (!line.trim()) {
      i++;
      continue;
    }
    const h = /^(#{1,3})\s+(.*)$/.exec(line);
    if (h) {
      out.push({ kind: "h", level: h[1]!.length, text: h[2]!.trim(), id: slug(h[2]!) });
      i++;
      continue;
    }
    if (line.startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i]!.startsWith("```")) body.push(lines[i++]!);
      i++;
      out.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    if (line.trim().startsWith("|") && i + 1 < lines.length && /^\s*\|?\s*:?-{3,}/.test(lines[i + 1]!)) {
      const head = cells(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i]!.trim().startsWith("|")) rows.push(cells(lines[i++]!));
      out.push({ kind: "table", head, rows });
      continue;
    }
    const li = /^\s*(?:[-*]|(\d+)\.)\s+(.*)$/.exec(line);
    if (li) {
      const ordered = li[1] !== undefined;
      const items: string[] = [];
      while (i < lines.length) {
        const m = /^\s*(?:[-*]|(\d+)\.)\s+(.*)$/.exec(lines[i]!);
        if (m && (m[1] !== undefined) === ordered) {
          items.push(m[2]!);
          i++;
        } else if (lines[i]!.startsWith("  ") && lines[i]!.trim() && items.length) {
          items[items.length - 1] += " " + lines[i]!.trim();
          i++;
        } else break;
      }
      out.push({ kind: ordered ? "ol" : "ul", items });
      continue;
    }
    const para: string[] = [];
    while (i < lines.length && lines[i]!.trim() && !/^(#{1,3}\s|```|\s*[-*]\s|\s*\d+\.\s|\s*\|)/.test(lines[i]!)) para.push(lines[i++]!.trim());
    if (!para.length) para.push(lines[i++]!.trim());
    out.push({ kind: "p", text: para.join(" ") });
  }
  return out;
}

/** Inline markup to plain text (for search). */
export function plain(s: string): string {
  return s.replace(/\[([^\]]+)\]\([^)]+\)/g, "$1").replace(/[`*]/g, "");
}

/** Inline markup to React nodes. `onLink` handles links; external links are shown as text. */
export function inline(s: string, onLink?: (href: string) => void, highlight?: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*]+\*)|(\[[^\]]+\]\([^)]+\))/g;
  let last = 0;
  let k = 0;
  const text = (t: string) => {
    if (!highlight) return t;
    const parts: ReactNode[] = [];
    const lower = t.toLowerCase();
    const q = highlight.toLowerCase();
    let at = 0;
    for (let j = lower.indexOf(q); j >= 0 && q; j = lower.indexOf(q, j + q.length)) {
      parts.push(t.slice(at, j), <mark key={`m${k++}`}>{t.slice(j, j + q.length)}</mark>);
      at = j + q.length;
    }
    parts.push(t.slice(at));
    return parts;
  };
  for (const m of s.matchAll(re)) {
    if (m.index! > last) out.push(text(s.slice(last, m.index)));
    const t = m[0];
    if (m[1]) out.push(<code key={k++}>{t.slice(1, -1)}</code>);
    else if (m[2]) out.push(<strong key={k++}>{text(t.slice(2, -2))}</strong>);
    else if (m[3]) out.push(<em key={k++}>{text(t.slice(1, -1))}</em>);
    else {
      const [, label, href] = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(t)!;
      if (onLink && href && !/^[a-z]+:/i.test(href))
        out.push(
          <a
            key={k++}
            href={`#${href}`}
            onClick={(e) => {
              e.preventDefault();
              onLink(href);
            }}
          >
            {label}
          </a>,
        );
      else out.push(label);
    }
    last = m.index! + t.length;
  }
  if (last < s.length) out.push(text(s.slice(last)));
  return out;
}

export function Markdown({ blocks, onLink, highlight }: { blocks: Block[]; onLink?: (href: string) => void; highlight?: string }) {
  return (
    <>
      {blocks.map((b, i) => {
        const il = (s: string) => inline(s, onLink, highlight);
        switch (b.kind) {
          case "h":
            return b.level === 1 ? (
              <h1 key={i} id={b.id}>
                {il(b.text)}
              </h1>
            ) : b.level === 2 ? (
              <h2 key={i} id={b.id}>
                {il(b.text)}
              </h2>
            ) : (
              <h3 key={i} id={b.id}>
                {il(b.text)}
              </h3>
            );
          case "p":
            return <p key={i}>{il(b.text)}</p>;
          case "ul":
            return (
              <ul key={i}>
                {b.items.map((t, j) => (
                  <li key={j}>{il(t)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={i}>
                {b.items.map((t, j) => (
                  <li key={j}>{il(t)}</li>
                ))}
              </ol>
            );
          case "table":
            return (
              <div key={i} className="md-table">
                <table>
                  <thead>
                    <tr>
                      {b.head.map((h, j) => (
                        <th key={j}>{il(h)}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {b.rows.map((r, j) => (
                      <tr key={j}>
                        {r.map((c, n) => (
                          <td key={n}>{il(c)}</td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "code":
            return <pre key={i}>{b.text}</pre>;
        }
      })}
    </>
  );
}
