import type { Folder, RequestEntry } from "@post-tester/bindings";
import { lazy, Suspense, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { appStore, deleteRequest, newRequestTab, openDialog, openRequest, refreshTree } from "../../state/app";
import { useStore } from "../../state/store";
import { Icon } from "../../ui/Icon";
import { Menu } from "../../ui/Menu";
import { EmptyState, MethodBadge } from "../../ui/widgets";

const ExportDialog = lazy(() => import("../import/ExportDialog").then((m) => ({ default: m.ExportDialog })));

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
  // Roving focus: one tree item is in the Tab order; arrow keys move between items.
  const [current, setCurrent] = useState<string | null>(null);
  const treeRef = useRef<HTMLUListElement>(null);
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

  const effectiveCollapsed = search ? NONE : collapsed;
  const toggle = (id: string) => {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setCollapsed(next);
  };
  const visible = visibleIds(filtered, effectiveCollapsed);
  const focusable = current !== null && visible.includes(current) ? current : (openIds !== null && visible.includes(openIds) ? openIds : visible[0]) ?? null;

  const onKeyDown = (e: KeyboardEvent<HTMLUListElement>) => {
    // Keys inside an open actions menu belong to the menu.
    if (e.defaultPrevented || (e.target as HTMLElement).closest(".menu")) return;
    const item = (e.target as HTMLElement).closest<HTMLLIElement>('li[role="treeitem"]');
    if (!item || !treeRef.current) return;
    const onItem = e.target === item;
    const items = [...treeRef.current.querySelectorAll<HTMLLIElement>('li[role="treeitem"]')];
    const index = items.indexOf(item);
    const id = item.dataset.id ?? "";
    const isFolder = item.dataset.kind === "folder";
    const expanded = item.getAttribute("aria-expanded") === "true";
    const focus = (target: HTMLLIElement | null | undefined) => {
      if (!target) return;
      setCurrent(target.dataset.id ?? null);
      target.focus();
    };
    switch (e.key) {
      case "ArrowDown":
        focus(items[index + 1]);
        break;
      case "ArrowUp":
        focus(items[index - 1]);
        break;
      case "Home":
        focus(items[0]);
        break;
      case "End":
        focus(items[items.length - 1]);
        break;
      case "ArrowRight":
        if (!onItem || !isFolder) return;
        if (!expanded) toggle(id);
        else focus(item.querySelector<HTMLLIElement>('li[role="treeitem"]'));
        break;
      case "ArrowLeft":
        if (!onItem) return;
        if (isFolder && expanded) toggle(id);
        else focus(item.parentElement?.closest<HTMLLIElement>('li[role="treeitem"]'));
        break;
      case "Enter":
      case " ":
        if (!onItem) return;
        if (isFolder) toggle(id);
        else void openRequest(id);
        break;
      case "ContextMenu":
      case "F10":
        if (e.key === "F10" && !e.shiftKey) return;
        item.querySelector<HTMLButtonElement>(":scope > .tree-row [aria-haspopup]")?.click();
        break;
      default:
        return;
    }
    e.preventDefault();
  };

  return (
    <>
      <ul className="tree" role="tree" aria-label="Collections" ref={treeRef} onKeyDown={onKeyDown}>
        <FolderChildren
          folder={filtered}
          depth={0}
          collapsed={effectiveCollapsed}
          onToggle={toggle}
          activeId={openIds}
          focusable={focusable}
          onFocusItem={setCurrent}
          onExport={setExporting}
        />
      </ul>
      {exporting !== null && (
        <Suspense fallback={null}>
          <ExportDialog folder={exporting} onClose={() => setExporting(null)} />
        </Suspense>
      )}
    </>
  );
}

const NONE: ReadonlySet<string> = new Set();

/** Ids of the items currently shown, in order (folders' children only when expanded). */
function visibleIds(folder: Folder, collapsed: ReadonlySet<string>): string[] {
  const ids: string[] = [];
  for (const child of folder.folders) {
    ids.push(child.id);
    if (!collapsed.has(child.id)) ids.push(...visibleIds(child, collapsed));
  }
  for (const request of folder.requests) ids.push(request.id);
  return ids;
}

const labelId = (id: string) => `tree-label-${encodeURIComponent(id)}`;

type ChildProps = {
  folder: Folder;
  depth: number;
  collapsed: ReadonlySet<string>;
  onToggle: (id: string) => void;
  activeId: string | null;
  /** The item in the Tab order (roving tabindex). */
  focusable: string | null;
  onFocusItem: (id: string) => void;
  onExport: (folder: string) => void;
};

/**
 * Tree items follow the ARIA tree pattern: the `li` is focusable (arrow keys, Enter,
 * Shift+F10 for the actions menu); the buttons inside serve the mouse, and the focused
 * row's action buttons are next in the Tab order.
 */
function FolderChildren({ folder, depth, collapsed, onToggle, activeId, focusable, onFocusItem, onExport }: ChildProps) {
  return (
    <>
      {folder.folders.map((child) => {
        const open = !collapsed.has(child.id);
        const tab = child.id === focusable ? 0 : -1;
        const name = child.name || child.id.split("/").pop() || child.id;
        return (
          <li
            key={child.id}
            role="treeitem"
            aria-expanded={open}
            aria-level={depth + 1}
            aria-labelledby={labelId(child.id)}
            data-id={child.id}
            data-kind="folder"
            tabIndex={tab}
            onFocus={(e) => e.target === e.currentTarget && onFocusItem(child.id)}
          >
            <div className="tree-row folder-row" style={{ paddingLeft: 8 + depth * 14 }}>
              <button type="button" className="tree-main" tabIndex={-1} onClick={() => onToggle(child.id)} title={child.settings.description || child.id}>
                <Icon name={open ? "chevronDown" : "chevronRight"} size={12} />
                <Icon name="folder" size={13} className="folder-icon" />
                <span className="truncate" id={labelId(child.id)}>
                  {name}
                </span>
              </button>
              <button type="button" className="icon-btn icon-btn-sm tree-action" tabIndex={tab} aria-label={`Run folder ${name}`} title="Run folder" onClick={() => openDialog({ type: "runner", folder: child.id, requests: [] })}>
                <Icon name="play" size={11} />
              </button>
              <Menu
                align="end"
                trigger={(props) => (
                  <button type="button" className="icon-btn icon-btn-sm tree-action" tabIndex={tab} aria-label={`More actions for ${name}`} {...props}>
                    <Icon name="more" size={12} />
                  </button>
                )}
                items={[
                  { id: "run", label: "Run folder…", onSelect: () => openDialog({ type: "runner", folder: child.id, requests: [] }) },
                  { id: "new", label: "New request here", onSelect: () => newRequestTab(undefined, child.id) },
                  { id: "import", label: "Import into this folder…", onSelect: () => openDialog({ type: "import", into: child.id }) },
                  { id: "export", label: "Export…", onSelect: () => onExport(child.id) },
                  { id: "settings", label: "Folder settings…", hint: "headers, auth, variables", onSelect: () => openDialog({ type: "folder-settings", folder: child.id }) },
                ]}
              />
            </div>
            {open && (
              <ul role="group">
                <FolderChildren
                  folder={child}
                  depth={depth + 1}
                  collapsed={collapsed}
                  onToggle={onToggle}
                  activeId={activeId}
                  focusable={focusable}
                  onFocusItem={onFocusItem}
                  onExport={onExport}
                />
              </ul>
            )}
          </li>
        );
      })}
      {folder.requests.map((entry) => (
        <RequestRow key={entry.id} entry={entry} depth={depth} active={entry.id === activeId} focusable={entry.id === focusable} onFocusItem={onFocusItem} />
      ))}
    </>
  );
}

function RequestRow({ entry, depth, active, focusable, onFocusItem }: { entry: RequestEntry; depth: number; active: boolean; focusable: boolean; onFocusItem: (id: string) => void }) {
  const tab = focusable ? 0 : -1;
  const name = entry.name || entry.id.split("/").pop() || entry.id;
  return (
    <li
      role="treeitem"
      aria-selected={active}
      aria-level={depth + 1}
      aria-labelledby={labelId(entry.id)}
      aria-description={entry.error ? `Cannot be read: ${entry.error}` : undefined}
      data-id={entry.id}
      data-kind="request"
      tabIndex={tab}
      onFocus={(e) => e.target === e.currentTarget && onFocusItem(entry.id)}
    >
      <div className={`tree-row${active ? " active" : ""}`} style={{ paddingLeft: 8 + depth * 14 + 16 }}>
        <button type="button" className="tree-main" tabIndex={-1} onClick={() => openRequest(entry.id)} title={entry.error ?? `${entry.method} ${entry.url}`}>
          <span id={labelId(entry.id)} className="row grow" style={{ gap: "inherit", minWidth: 0 }}>
            {entry.error ? <Icon name="alert" size={12} className="tone-danger" /> : <MethodBadge method={entry.method} />}
            <span className="truncate">{name}</span>
          </span>
        </button>
        <Menu
          align="end"
          trigger={(props) => (
            <button type="button" className="icon-btn icon-btn-sm tree-action" tabIndex={tab} aria-label={`More actions for ${name}`} {...props}>
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
