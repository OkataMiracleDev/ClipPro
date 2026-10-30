import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { PAGES, search } from "./manual";
import { Markdown, parse, slug } from "./markdown";

describe("markdown", () => {
  it("parses the blocks the manual uses", () => {
    const b = parse("# Title\n\nSome **bold** and `code`.\nSame paragraph.\n\n## Part two\n\n- one\n- two\n  continued\n\n1. first\n2. second\n\n| A | B |\n|---|---|\n| x \\| y | z |\n\n```\nraw <b>\n```\n");
    expect(b.map((x) => x.kind)).toEqual(["h", "p", "h", "ul", "ol", "table", "code"]);
    expect(b[1]).toEqual({ kind: "p", text: "Some **bold** and `code`. Same paragraph." });
    expect(b[3]).toEqual({ kind: "ul", items: ["one", "two continued"] });
    expect(b[5]).toEqual({ kind: "table", head: ["A", "B"], rows: [["x | y", "z"]] });
    expect(slug("Review a timeline")).toBe("review-a-timeline");
  });

  it("renders text, never HTML", () => {
    const { container } = render(<Markdown blocks={parse("Hello <img src=x onerror=alert(1)> [link](02-analysis.md) [web](https://example.com)")} onLink={() => {}} />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("<img src=x onerror=alert(1)>");
    expect(container.querySelectorAll("a")).toHaveLength(1);
  });
});

describe("manual", () => {
  it("bundles every page with a title, What's new last", () => {
    expect(PAGES.length).toBeGreaterThanOrEqual(15);
    expect(PAGES[0]!.title).toBe("Getting started");
    expect(PAGES[PAGES.length - 1]!.file).toBe("whats-new.md");
  });

  it("finds passages by every word", () => {
    const hits = search("offload checksum");
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0]!.page.file).toBe("09-ingest.md");
    expect(search("zebras")[0]!.page.title).toBe("The viewer");
    expect(search("qwertyuiop")).toEqual([]);
  });

  it("documents every default shortcut", async () => {
    const { ACTIONS } = await import("./keys");
    const settings = PAGES.find((p) => p.file === "12-settings.md")!;
    const text = JSON.stringify(settings.blocks);
    for (const a of ACTIONS) {
      const key = a.key.replace("ArrowLeft", "←").replace("ArrowRight", "→").replace("Escape", "Esc");
      expect(text, a.id).toContain(key);
    }
  });
});
