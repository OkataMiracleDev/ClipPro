// Watch folders in the sidebar (SPEC §19, P9.5): new media in these folders is imported and
// analysed once it has finished copying.

import { Eye, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type WatchFolder } from "../api";
import { Popover, Switch } from "../components/ui";
import { pickFolder } from "../lib/dialogs";
import { useApp } from "../store";

function FolderOptions({ f, onChange }: { f: WatchFolder; onChange: (f: WatchFolder) => void }) {
  const [ext, setExt] = useState(f.extensions.join(", "));
  return (
    <div className="flex w-60 flex-col gap-2 p-2">
      <div className="truncate text-[11px] text-text-3" title={f.path}>
        {f.path}
      </div>
      <label className="flex items-center justify-between text-[12px]">
        Include subfolders
        <Switch label="Include subfolders" checked={f.subfolders} onChange={(subfolders) => onChange({ ...f, subfolders })} />
      </label>
      <label className="flex flex-col gap-1 text-[12px]">
        Only these extensions
        <input
          value={ext}
          placeholder="All media (e.g. mov, mxf)"
          onChange={(e) => setExt(e.target.value)}
          onBlur={() => onChange({ ...f, extensions: ext.split(/[\s,;]+/).map((x) => x.replace(/^\./, "").toLowerCase()).filter(Boolean) })}
          className="h-6 rounded border border-separator-strong bg-control px-1.5 text-[12px]"
        />
      </label>
    </div>
  );
}

export function WatchFolders() {
  const project = useApp((s) => s.project);
  const toast = useApp((s) => s.toast);
  const [folders, setFolders] = useState<WatchFolder[]>([]);
  useEffect(() => {
    if (!project) return;
    api.watchFoldersGet().then(setFolders, () => setFolders([]));
  }, [project]);
  if (!project) return null;
  const save = async (next: WatchFolder[]) => {
    try {
      await api.watchFoldersSet(next);
      setFolders(next);
    } catch (e) {
      toast(`Watch folders: ${e instanceof Error ? e.message : e}`, "error");
    }
  };
  return (
    <div>
      <div className="mb-1 flex items-center justify-between px-2">
        <span className="section-header">Watch folders</span>
        <button
          aria-label="Add a watch folder"
          title="Add a watch folder"
          className="rounded p-0.5 text-text-3 hover:bg-control-hover hover:text-text"
          onClick={async () => {
            const p = await pickFolder();
            if (p && !folders.some((f) => f.path === p)) await save([...folders, { path: p, subfolders: true, extensions: [] }]);
          }}
        >
          <Plus size={13} />
        </button>
      </div>
      {folders.length === 0 ? (
        <div className="px-2 text-[11px] text-text-3">New footage in a watched folder is analysed when it finishes copying.</div>
      ) : (
        folders.map((f, i) => (
          <div key={f.path} className="group flex h-7 items-center gap-2 rounded-md px-2 text-[12px] text-text-2 hover:bg-control-hover">
            <Eye size={13} className="shrink-0 text-accent" />
            <Popover
              trigger={
                <button className="min-w-0 flex-1 truncate text-left" title={`${f.path}${f.subfolders ? " (with subfolders)" : ""}${f.extensions.length ? ` · ${f.extensions.join(", ")}` : ""}`}>
                  {f.path.split(/[\\/]/).filter(Boolean).pop()}
                </button>
              }
            >
              <FolderOptions f={f} onChange={(nf) => void save(folders.map((x, j) => (j === i ? nf : x)))} />
            </Popover>
            <button aria-label={`Stop watching ${f.path}`} className="rounded p-0.5 text-text-3 opacity-0 group-hover:opacity-100 hover:text-text" onClick={() => void save(folders.filter((_, j) => j !== i))}>
              <X size={12} />
            </button>
          </div>
        ))
      )}
    </div>
  );
}
