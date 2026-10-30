// In-app help (SPEC OTH-2, P14.4): the manual from docs/manual, searchable, with "What's new"
// and a way back into the first-run tour.

import * as RDialog from "@radix-ui/react-dialog";
import { BookOpen, Search, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { create } from "zustand";
import { cx, IconButton } from "../components/ui";
import { Markdown } from "../lib/markdown";
import { PAGES, search } from "../lib/manual";
import { useTour } from "./Tour";
import { Diagnostics } from "./Diagnostics";

interface HelpState {
  open: boolean;
  file: string;
  anchor: string | null;
  show: (file?: string, anchor?: string | null) => void;
  close: () => void;
}

export const useHelp = create<HelpState>((set) => ({
  open: false,
  file: PAGES[0]?.file ?? "",
  anchor: null,
  show: (file, anchor = null) => set((s) => ({ open: true, file: file ?? s.file, anchor })),
  close: () => set({ open: false }),
}));

export function Help() {
  const { open, file, anchor, show, close } = useHelp();
  const [query, setQuery] = useState("");
  const [diag, setDiag] = useState(() => new URLSearchParams(location.search).has("diag"));
  const body = useRef<HTMLDivElement>(null);
  const page = PAGES.find((p) => p.file === file) ?? PAGES[0];
  const hits = useMemo(() => search(query), [query]);

  useEffect(() => {
    if (!open) return;
    const el = anchor ? body.current?.querySelector(`#${CSS.escape(anchor)}`) : null;
    if (el) el.scrollIntoView({ block: "start" });
    else body.current?.scrollTo({ top: 0 });
  }, [open, file, anchor]);

  const follow = (href: string) => {
    const [f, a] = href.split("#");
    show(f || file, a ?? null);
  };

  if (!page) return null;
  return (
    <RDialog.Root open={open} onOpenChange={(o) => !o && close()}>
      <RDialog.Portal>
        <RDialog.Overlay className="fixed inset-0 z-40 bg-black/25" />
        <RDialog.Content
          aria-describedby={undefined}
          className="fixed top-1/2 left-1/2 z-50 flex h-[82vh] w-[960px] max-w-[96vw] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-(--radius-sheet) bg-raised shadow-(--shadow-float) ring-1 ring-separator-strong"
        >
          <nav className="flex w-60 shrink-0 flex-col gap-2 border-r border-separator bg-sidebar p-3" aria-label="Manual">
            <RDialog.Title className="flex items-center gap-2 px-1 text-[13px] font-semibold">
              <BookOpen size={14} /> ClipPro Help
            </RDialog.Title>
            <label className="flex h-7 items-center gap-1.5 rounded-(--radius-control) bg-control px-2 ring-1 ring-separator-strong">
              <Search size={12} className="text-text-3" />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search the manual"
                aria-label="Search the manual"
                className="min-w-0 flex-1 bg-transparent text-[12.5px] outline-none"
              />
              {query && (
                <button aria-label="Clear search" onClick={() => setQuery("")} className="text-text-3 hover:text-text">
                  <X size={12} />
                </button>
              )}
            </label>
            <div className="min-h-0 flex-1 overflow-y-auto">
              {query ? (
                hits.length ? (
                  hits.map((h, i) => (
                    <button
                      key={i}
                      onClick={() => show(h.page.file, h.headingId)}
                      className="flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left hover:bg-control-hover"
                    >
                      <span className="truncate text-[12px] font-medium">
                        {h.page.title}
                        {h.heading && h.heading !== h.page.title ? <span className="text-text-3"> › {h.heading}</span> : null}
                      </span>
                      <span className="line-clamp-2 text-[11px] text-text-3">{h.snippet}</span>
                    </button>
                  ))
                ) : (
                  <div className="px-2 py-3 text-[12px] text-text-3">Nothing in the manual matches “{query}”.</div>
                )
              ) : (
                PAGES.map((p) => (
                  <button
                    key={p.file}
                    onClick={() => show(p.file)}
                    aria-current={p.file === page.file ? "page" : undefined}
                    className={cx("flex h-7 w-full items-center gap-2 rounded-md px-2 text-left text-[12.5px]", p.file === page.file ? "bg-accent-soft text-text" : "text-text-2 hover:bg-control-hover")}
                  >
                    {p.file.startsWith("whats-new") && <Sparkles size={12} className="text-accent" />}
                    <span className="truncate">{p.title}</span>
                  </button>
                ))
              )}
            </div>
            <button
              className="rounded-md px-2 py-1.5 text-left text-[12px] text-text-2 hover:bg-control-hover"
              onClick={() => {
                close();
                useTour.getState().start();
              }}
            >
              Take the tour again
            </button>
            <button className="rounded-md px-2 py-1.5 text-left text-[12px] text-text-2 hover:bg-control-hover" onClick={() => setDiag(true)}>
              Diagnostics and log…
            </button>
          </nav>
          <div ref={body} className="min-w-0 flex-1 overflow-y-auto px-8 py-6">
            <article className="manual">
              <Markdown blocks={page.blocks} onLink={follow} highlight={query.trim().split(/\s+/)[0] || undefined} />
            </article>
          </div>
          <RDialog.Close asChild>
            <IconButton label="Close help" className="absolute top-2 right-2">
              <X size={15} />
            </IconButton>
          </RDialog.Close>
        </RDialog.Content>
      </RDialog.Portal>
      <Diagnostics open={diag} onOpenChange={setDiag} />
    </RDialog.Root>
  );
}
