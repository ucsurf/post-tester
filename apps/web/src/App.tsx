import { lazy, Suspense, useEffect, useState } from "react";
import { getToken } from "./api/http";
import { readLocal, writeLocal } from "./lib/storage";
import { ActivityPanel } from "./features/activity/ActivityPanel";
import { Sidebar } from "./features/sidebar/Sidebar";
import { ConnectionErrorScreen, LoadingScreen, NoTokenScreen, NoWorkspaceScreen } from "./features/workspace/Screens";
import { liveConnection, useLiveSelector } from "./live/live";
import { appStore, loadWorkspace } from "./state/app";
import { useStore } from "./state/store";
import { MainArea } from "./MainArea";
import { QuestionDialog } from "./ui/QuestionDialog";
import { Splitter, usePersistentNumber } from "./ui/Splitter";
import { Toasts } from "./ui/Toasts";
import "./styles/layout.css";

// Loaded when first opened, to keep the initial bundle small.
const SpanDrawer = lazy(() => import("./features/activity/SpanDrawer"));
const ImportDialog = lazy(() => import("./features/import/ImportDialog").then((m) => ({ default: m.ImportDialog })));
const RunnerDialog = lazy(() => import("./features/runner/RunnerDialog").then((m) => ({ default: m.RunnerDialog })));
const FolderSettingsDialog = lazy(() => import("./features/sidebar/FolderSettingsDialog").then((m) => ({ default: m.FolderSettingsDialog })));
const LoadTestDialog = lazy(() => import("./features/runner/LoadTestDialog").then((m) => ({ default: m.LoadTestDialog })));

export function App() {
  const token = getToken();
  const info = useStore(appStore, (s) => s.info);
  const infoError = useStore(appStore, (s) => s.infoError);

  useEffect(() => {
    if (!token) return;
    liveConnection.start();
    void loadWorkspace();
    return () => liveConnection.stop();
  }, [token]);

  if (!token) return <NoTokenScreen />;
  let content;
  if (infoError && !info) content = <ConnectionErrorScreen message={infoError} />;
  else if (!info) content = <LoadingScreen />;
  else if (!info.workspace) content = <NoWorkspaceScreen />;
  else content = <Shell />;

  return (
    <>
      {content}
      <QuestionDialog />
      <Toasts />
    </>
  );
}

type DockPreference = "auto" | "right" | "bottom";
const DOCK_KEY = "post-tester.activityDock";

function useWindowWidth(): number {
  const [width, setWidth] = useState(() => window.innerWidth);
  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return width;
}

function Shell() {
  const width = useWindowWidth();
  const [sidebarWidth, setSidebarWidth] = usePersistentNumber("post-tester.sidebarWidth", 260);
  const [activityWidth, setActivityWidth] = usePersistentNumber("post-tester.activityWidth", 480);
  const [activityHeight, setActivityHeight] = usePersistentNumber("post-tester.activityHeight", 280);
  const [activityOpen, setActivityOpen] = useState(true);
  const [dockPref, setDockPref] = useState<DockPreference>(() => readDock());
  const dock = dockPref === "auto" ? (width >= 1280 ? "right" : "bottom") : dockPref;

  const toggleDock = () => {
    const next = dock === "right" ? "bottom" : "right";
    setDockPref(next);
    writeLocal(DOCK_KEY, next);
  };

  const narrow = width < 1000;
  const effectiveSidebar = narrow ? 220 : sidebarWidth;

  return (
    <div className="shell">
      <div className="shell-sidebar" style={{ width: effectiveSidebar }}>
        <Sidebar />
      </div>
      {!narrow && <Splitter orientation="vertical" size={sidebarWidth} onResize={setSidebarWidth} min={200} max={420} direction={1} label="Resize sidebar" />}
      <div className={`workarea dock-${dock}`}>
        <main className="main" aria-label="Editor">
          <MainArea />
          {!activityOpen && <ActivityReopen onOpen={() => setActivityOpen(true)} />}
        </main>
        {activityOpen && (
          <>
            {dock === "right" ? (
              <Splitter orientation="vertical" size={activityWidth} onResize={setActivityWidth} min={320} max={Math.max(360, width - effectiveSidebar - 420)} direction={-1} label="Resize Live Activity" />
            ) : (
              <Splitter orientation="horizontal" size={activityHeight} onResize={setActivityHeight} min={140} max={Math.max(200, window.innerHeight - 240)} direction={-1} label="Resize Live Activity" />
            )}
            <div className="activity-dock" style={dock === "right" ? { width: activityWidth } : { height: activityHeight }}>
              <ActivityPanel dock={dock} onToggleDock={toggleDock} onCollapse={() => setActivityOpen(false)} />
            </div>
          </>
        )}
      </div>
      {/* Separate boundaries: loading a dialog must not hide an open drawer. */}
      <Suspense fallback={null}>
        <LazyDrawer />
      </Suspense>
      <Suspense fallback={null}>
        <Dialogs />
      </Suspense>
    </div>
  );
}

function readDock(): DockPreference {
  const v = readLocal(DOCK_KEY);
  return v === "right" || v === "bottom" ? v : "auto";
}

/** Shown while Live Activity is hidden, so running work is never invisible. */
function ActivityReopen({ onOpen }: { onOpen: () => void }) {
  const running = useLiveSelector((state) => state.roots.filter((id) => state.spans.get(id)?.outcome === null).length);
  return (
    <button type="button" className="activity-reopen" onClick={onOpen}>
      <span className="live-dot" data-state="open" />
      Live Activity{running > 0 ? ` · ${running} running` : ""}
    </button>
  );
}

/** Mounts the drawer module only once something is selected. */
function LazyDrawer() {
  const selected = useStore(appStore, (s) => s.selectedSpan !== null);
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    if (selected) setLoaded(true);
  }, [selected]);
  return selected || loaded ? <SpanDrawer /> : null;
}

function Dialogs() {
  const dialog = useStore(appStore, (s) => s.dialog);
  if (!dialog) return null;
  switch (dialog.type) {
    case "import":
      return <ImportDialog into={dialog.into} />;
    case "runner":
      return <RunnerDialog folder={dialog.folder} requests={dialog.requests} />;
    case "load-test":
      return <LoadTestDialog requestKey={dialog.requestKey} />;
    case "folder-settings":
      return <FolderSettingsDialog folder={dialog.folder} />;
  }
}
