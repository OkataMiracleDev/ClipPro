// Update available (SPEC OTH-4): a slim bar under the title bar with the new version, its release
// notes on demand, and the user's choice. Nothing installs without "Install and restart".

import { ChevronDown, ChevronRight, Download, X } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { Button, Spinner } from "../components/ui";
import { Markdown, parse } from "../lib/markdown";
import { useApp } from "../store";

export function UpdateBanner() {
  const { update, dismissUpdate, skipUpdate, installUpdate } = useApp();
  const [notesOpen, setNotesOpen] = useState(false);
  if (!update) return null;
  const busy = update.state === "downloading" || update.state === "installing";
  const pct = update.total ? Math.round((update.downloaded / update.total) * 100) : null;
  return (
    <div className="shrink-0 border-b border-separator bg-accent-soft" role="region" aria-label="Update available">
      <div className="flex items-center gap-3 px-4 py-1.5 text-[12px]">
        <Download size={14} className="shrink-0 text-accent-text" />
        <span className="min-w-0 flex-1 truncate">
          <span className="font-medium">ClipPro {update.info.version} is available</span>
          <span className="text-text-2"> · you have {update.info.current}</span>
        </span>
        {update.info.notes && (
          <button className="flex items-center gap-0.5 text-text-2 hover:text-text" aria-expanded={notesOpen} onClick={() => setNotesOpen(!notesOpen)}>
            {notesOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            What's new
          </button>
        )}
        {update.state === "failed" && <span className="text-danger">{update.message ?? "The update failed."}</span>}
        {busy ? (
          <span className="flex items-center gap-1.5 text-text-2 tabular">
            <Spinner size={11} />
            {update.state === "installing" ? "Installing…" : pct !== null ? `Downloading ${pct}%` : "Downloading…"}
          </span>
        ) : (
          <>
            <Button variant="primary" onClick={() => void installUpdate()}>
              {update.state === "failed" ? "Try again" : "Install and restart"}
            </Button>
            <button className="text-text-2 hover:text-text" onClick={() => skipUpdate()} title="Don't offer this version again">
              Skip this version
            </button>
            <button aria-label="Later" title="Later" className="rounded p-0.5 text-text-3 hover:text-text" onClick={() => dismissUpdate()}>
              <X size={13} />
            </button>
          </>
        )}
      </div>
      {notesOpen && update.info.notes && (
        <div className="max-h-56 overflow-y-auto border-t border-separator bg-window px-6 py-3 text-[12px] leading-relaxed">
          <Markdown blocks={parse(update.info.notes)} onLink={(href) => void api.openUrl(href)} />
        </div>
      )}
    </div>
  );
}
