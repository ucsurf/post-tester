import type { Folder, RequestEntry } from "@post-tester/bindings";
import { useMemo, useState } from "react";
import { appStore, deleteRequest, newRequestTab, openDialog, openRequest, refreshTree } from "../../state/app";
import { useStore } from "../../state/store";
import { Icon } from "../../ui/Icon";
import { Menu } from "../../ui/Menu";
import { EmptyState, MethodBadge } from "../../ui/widgets";
import { ExportDialog } from "../import/ExportDialog";

/** The collection tree: folders and requests with method badges. */
export function CollectionTree({ search }: { search: string }) {
  const tree = useStore(appStore, (s) => s.tree);
  const treeError = useStore(appStore, (s) => s.treeError);
  const openIds = useStore(appStore, (s) => {
    const active = s.activeTab ? s.requests[s.activeTab] : undefined;
    return active?.id ?? null;
  });
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [exporting, setExporting] = useState<string | null>(null);
  const filtered = useMemo(() => (tree ? filterFolder(tree, search.trim().toLowerCase()) : null), [tree, search]);

  if (treeError) {
    return (
      <div className="pad stack">
        <p className="tone-danger hint">{treeError}</p>
        <button type="button" className="btn btn-sm" onClick={() => refreshTree()}>
          Retry
        </button>
      </div>
    );
  }
  if (!tree) return <p className="pad faint">Loading…</p>;
  if (tree.folders.length === 0 && tree.requests.length === 0) {
    return (
      <EmptyState title="No requests yet">
        <p>Create your first request, or import from cURL, Postman or OpenAPI.</p>
        <div className="row" style={{ justifyContent: "center", marginTop: 8 }}>
          <button type="button" className="btn btn-sm btn-primary" onClick={() => newRequestTab()}>
            Create your first request
          </button>
          <button type="button" className="btn btn-sm" onClick={() => openDialog({ type: "import" })}>
            Import
          </button>
        </div>
      </EmptyState>
    );
  }
  if (!filtered) return <p className="pad faint">No requests match “{search}”.</p>;

  const toggle = (id: string) => {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setCollapsed(next);
  };

  return (
    <>
      <ul className="tree" role="tree" aria-label="Collections">
        <FolderChildren folder={filtered} depth={0} collapsed={search ? new Set() : collapsed} onToggle={toggle} activeId={openIds} onExport={setExporting} />
      </ul>
      {exporting !== null && <ExportDialog folder={exporting} onClose={() => setExporting(null)} />}
    </>
  );
}

type ChildProps = {
  folder: Folder;
  depth: number;
  collapsed: ReadonlySet<string>;
  onToggle: (id: string) => void;
  activeId: string | null;
  onExport: (folder: string) => void;
};

function FolderChildren({ folder, depth, collapsed, onToggle, activeId, onExport }: ChildProps) {
  return (
    <>
      {folder.folders.map((child) => {
        const open = !collapsed.has(child.id);
        return (
          <li key={child.id} role="treeitem" aria-expanded={open} aria-selected={false}>
            <div className="tree-row folder-row" style={{ paddingLeft: 8 + depth * 14 }}>
              <button type="button" className="tree-main" onClick={() => onToggle(child.id)} title={child.settings.description || child.id}>
                <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />
                <Icon name="folder" size={13} className="folder-icon" />
                <span className="truncate">{child.name || child.id.split("/").pop()}</span>
              </button>
              <button type="button" className="icon-btn icon-btn-sm tree-action" aria-label={`Run folder ${child.name}`} title="Run folder" onClick={() => openDialog({ type: "runner", folder: child.id, requests: [] })}>
                <Icon name="play" size={11} />
              </button>
              <Menu
                align="end"
                trigger={(props) => (
                  <button type="button" className="icon-btn icon-btn-sm tree-action" aria-label={`More actions for ${child.name}`} {...props}>
                    <Icon name="more" size={12} />
                  </button>
                )}
                items={[
                  { id: "run", label: "Run folder…", onSelect: () => openDialog({ type: "runner", folder: child.id, requests: [] }) },
                  { id: "new", label: "New request here", onSelect: () => newRequestTab(undefined, child.id) },
                  { id: "import", label: "Import into this folder…", onSelect: () => openDialog({ type: "import", into: child.id }) },
                  { id: "export", label: "Export…", onSelect: () => onExport(child.id) },
                ]}
              />
            </div>
            {open && (
              <ul role="group">
                <FolderChildren folder={child} depth={depth + 1} collapsed={collapsed} onToggle={onToggle} activeId={activeId} onExport={onExport} />
              </ul>
            )}
          </li>
        );
      })}
      {folder.requests.map((entry) => (
        <RequestRow key={entry.id} entry={entry} depth={depth} active={entry.id === activeId} />
      ))}
    </>
  );
}

function RequestRow({ entry, depth, active }: { entry: RequestEntry; depth: number; active: boolean }) {
  return (
    <li role="treeitem" aria-selected={active}>
      <div className={`tree-row${active ? " active" : ""}`} style={{ paddingLeft: 8 + depth * 14 + 16 }}>
        <button type="button" className="tree-main" onClick={() => openRequest(entry.id)} title={entry.error ?? `${entry.method} ${entry.url}`}>
          {entry.error ? <Icon name="alert" size={12} className="tone-danger" /> : <MethodBadge method={entry.method} />}
          <span className="truncate">{entry.name || entry.id.split("/").pop()}</span>
        </button>
        <Menu
          align="end"
          trigger={(props) => (
            <button type="button" className="icon-btn icon-btn-sm tree-action" aria-label={`More actions for ${entry.name}`} {...props}>
              <Icon name="more" size={12} />
            </button>
          )}
          items={[
            { id: "open", label: "Open", onSelect: () => openRequest(entry.id) },
            { id: "run", label: "Run…", onSelect: () => openDialog({ type: "runner", folder: null, requests: [entry.id] }) },
            { id: "delete", label: "Delete…", danger: true, onSelect: () => deleteRequest(entry.id) },
          ]}
        />
      </div>
    </li>
  );
}

/** Keeps folders that match or contain a matching request; null when nothing matches. */
function filterFolder(folder: Folder, needle: string): Folder | null {
  if (!needle) return folder;
  const requests = folder.requests.filter((r) => `${r.name} ${r.id} ${r.url} ${r.method}`.toLowerCase().includes(needle));
  const folders = folder.folders.map((f) => (f.name.toLowerCase().includes(needle) ? f : filterFolder(f, needle))).filter((f): f is Folder => f !== null);
  if (requests.length === 0 && folders.length === 0) return null;
  return { ...folder, requests, folders };
}
