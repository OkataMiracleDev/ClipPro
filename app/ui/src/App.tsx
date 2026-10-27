import * as RTooltip from "@radix-ui/react-tooltip";
import { FilePlus2, FolderOpen, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { isTauri } from "./api";
import { Button, cx, EmptyState, Segmented } from "./components/ui";
import { pickFolder, pickMedia, pickNewProject, pickProjectToOpen } from "./lib/dialogs";
import { useApp } from "./store";
import { ClipList, Sidebar } from "./views/ClipList";
import { ScoreChart } from "./views/ScoreChart";
import { Timeline } from "./views/Timeline";
import { StatusBar } from "./views/StatusBar";
import { TitleBar } from "./views/TitleBar";
import { FramePanel } from "./views/FramePanel";
import { TuningPanel } from "./views/TuningPanel";
import { Viewer } from "./views/Viewer";

function Toasts() {
  const { toasts, dismissToast } = useApp();
  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-[360px] flex-col gap-2" aria-live="polite">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={cx(
            "pointer-events-auto flex items-start gap-2 rounded-(--radius-panel) bg-raised px-3 py-2 text-[12px] shadow-(--shadow-float) ring-1",
            t.tone === "error" ? "ring-danger/50" : "ring-separator-strong",
          )}
        >
          <span className="flex-1">{t.text}</span>
          <button aria-label="Dismiss" className="text-text-3 hover:text-text" onClick={() => dismissToast(t.id)}>
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}

function Welcome() {
  const { newProject, openProject } = useApp();
  return (
    <EmptyState icon={<FilePlus2 size={30} />} title="Create a project for your footage, or open an existing one.">
      <div className="flex gap-2">
        <Button
          variant="primary"
          onClick={async () => {
            const p = await pickNewProject();
            if (p) await newProject(p, p.split(/[\\/]/).pop()!.replace(/\.clippro$/i, ""));
          }}
        >
          New project…
        </Button>
        <Button
          onClick={async () => {
            const p = await pickProjectToOpen();
            if (p) await openProject(p);
          }}
        >
          <FolderOpen size={14} /> Open…
        </Button>
      </div>
    </EmptyState>
  );
}

function NoMedia() {
  const { importPaths } = useApp();
  return (
    <EmptyState icon={<Plus size={30} />} title="Drop footage here, or add files or a folder.">
      <div className="flex gap-2">
        <Button
          variant="primary"
          onClick={async () => {
            const paths = await pickMedia();
            if (paths.length) await importPaths(paths);
          }}
        >
          Add media…
        </Button>
        <Button
          onClick={async () => {
            const f = await pickFolder();
            if (f) await importPaths([f]);
          }}
        >
          Add folder…
        </Button>
      </div>
    </EmptyState>
  );
}

function useDragDrop() {
  const { importPaths, project } = useApp();
  const [over, setOver] = useState(false);
  useEffect(() => {
    if (!isTauri) return;
    let off: (() => void) | undefined;
    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      off = await getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setOver(true);
        else if (e.payload.type === "leave") setOver(false);
        else if (e.payload.type === "drop") {
          setOver(false);
          if (project && e.payload.paths.length) void importPaths(e.payload.paths);
        }
      });
    })();
    return () => off?.();
  }, [importPaths, project]);
  return over;
}

export function App() {
  const { init, project, clips, inspectorOpen, rightTab, setRightTab } = useApp();
  const dropping = useDragDrop();
  useEffect(() => {
    void init();
  }, [init]);

  return (
    <RTooltip.Provider>
      <div className="flex h-full flex-col">
        <TitleBar />
        {!project ? (
          <div className="min-h-0 flex-1 bg-window">
            <Welcome />
          </div>
        ) : (
          <div
            className="grid min-h-0 flex-1"
            style={{ gridTemplateColumns: `240px 1fr ${inspectorOpen ? "300px" : "0px"}`, gridTemplateRows: "1fr 150px" }}
          >
            <aside className="row-span-2 flex min-h-0 flex-col border-r border-separator bg-sidebar">
              <Sidebar />
              <div className="min-h-0 flex-1 border-t border-separator">
                <ClipList />
              </div>
            </aside>
            <main className="grid min-h-0 grid-rows-[1fr_190px] bg-window">
              {clips.length ? (
                <>
                  <Viewer />
                  <div className="border-t border-separator">
                    <ScoreChart />
                  </div>
                </>
              ) : (
                <div className="row-span-2">
                  <NoMedia />
                </div>
              )}
            </main>
            <aside className={cx("row-span-2 flex min-h-0 flex-col border-l border-separator bg-sidebar", !inspectorOpen && "hidden")}>
              <div className="flex justify-center border-b border-separator px-4 py-2">
                <Segmented
                  label="Panel"
                  size="sm"
                  value={rightTab}
                  onChange={setRightTab}
                  options={[
                    { value: "tuning", label: "Tuning" },
                    { value: "frame", label: "Frame", title: "Frame inspector (I)" },
                  ]}
                />
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto">{rightTab === "tuning" ? <TuningPanel /> : <FramePanel />}</div>
            </aside>
            <section className="col-start-2 min-h-0 border-t border-separator bg-window">
              <Timeline />
            </section>
          </div>
        )}
        {project && <StatusBar />}
        {dropping && (
          <div className="pointer-events-none fixed inset-2 z-40 grid place-items-center rounded-(--radius-sheet) border-2 border-dashed border-accent bg-accent-soft text-[14px] font-medium">
            Drop to add footage
          </div>
        )}
        <Toasts />
      </div>
    </RTooltip.Provider>
  );
}
