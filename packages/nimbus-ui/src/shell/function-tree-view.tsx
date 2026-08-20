import { Link } from "@tanstack/react-router";
import {
  ChevronDown,
  ChevronRight,
  Code,
  Eye,
  FileCode,
  Folder,
  FolderOpen,
  Globe,
  type LucideIcon,
  Pencil,
  Zap,
} from "lucide-react";
import { useMemo, useState } from "react";

import { cn } from "../lib/cn";
import {
  type FolderNode,
  type FunctionLeaf,
  type FunctionTree,
  type ModuleNode,
  collectAllPaths,
  filterFunctionTree,
} from "./function-tree";

// Leaf icon by function kind (query reads, mutation writes, action runs,
// http serves). Falls back to a generic code glyph.
function kindIcon(kind: string | undefined): LucideIcon {
  switch ((kind ?? "").toLowerCase()) {
    case "query":
      return Eye;
    case "mutation":
      return Pencil;
    case "action":
      return Zap;
    case "http":
    case "httpaction":
    case "http_action":
      return Globe;
    default:
      return Code;
  }
}

export function FunctionTreeView({
  tree,
  filter,
  testidPrefix,
}: {
  tree: FunctionTree;
  filter: string;
  testidPrefix: string;
}) {
  const filtered = useMemo(
    () => filterFunctionTree(tree, filter),
    [tree, filter],
  );
  const expandableKeys = useMemo(() => collectAllPaths(filtered), [filtered]);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());

  const isCollapsed = (key: string) => collapsed.has(key);
  const toggle = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  // When filter is active, default-expand everything (collapsed set ignored
  // for visibility, but interaction still works on the unfiltered view).
  const filterActive = filter.trim().length > 0;

  if (
    filtered.folders.length === 0 &&
    filtered.modules.length === 0 &&
    tree.count === 0
  ) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        <p>No functions registered.</p>
        <p className="mt-2">
          Deploy a Convex, Nimbus, or Cloud Functions app to populate this list.
        </p>
      </div>
    );
  }
  if (filtered.folders.length === 0 && filtered.modules.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        No functions match the filter.
      </div>
    );
  }

  return (
    <div
      className="flex flex-col gap-px px-2 py-2"
      data-testid={`${testidPrefix}-tree`}
      data-expandable-keys={expandableKeys.length}
    >
      {filtered.folders.map((folder) => (
        <FolderRow
          key={folder.fullPath}
          folder={folder}
          depth={0}
          collapsed={filterActive ? new Set() : collapsed}
          toggle={toggle}
          isCollapsed={(k) => !filterActive && isCollapsed(k)}
          testidPrefix={testidPrefix}
        />
      ))}
      {filtered.modules.map((mod) => (
        <ModuleRow
          key={mod.fullPath}
          mod={mod}
          depth={0}
          collapsed={filterActive ? new Set() : collapsed}
          toggle={toggle}
          isCollapsed={(k) => !filterActive && isCollapsed(k)}
          testidPrefix={testidPrefix}
        />
      ))}
    </div>
  );
}

type RowProps = {
  depth: number;
  collapsed: Set<string>;
  toggle: (key: string) => void;
  isCollapsed: (key: string) => boolean;
  testidPrefix: string;
};

function FolderRow({
  folder,
  ...rest
}: RowProps & {
  folder: FolderNode;
}) {
  const key = `folder:${folder.fullPath}`;
  const collapsed = rest.isCollapsed(key);
  return (
    <div data-testid={`${rest.testidPrefix}-folder-${folder.fullPath}`}>
      <button
        type="button"
        onClick={() => rest.toggle(key)}
        aria-expanded={!collapsed}
        className={cn(
          "flex h-7 w-full items-center gap-1 rounded-md px-1 text-left text-muted hover:bg-surface-2 hover:text-default",
        )}
        style={{ paddingLeft: `${rest.depth * 12 + 4}px` }}
        data-testid={`${rest.testidPrefix}-folder-toggle-${folder.fullPath}`}
      >
        {collapsed ? (
          <ChevronRight size={12} aria-hidden />
        ) : (
          <ChevronDown size={12} aria-hidden />
        )}
        {collapsed ? (
          <Folder size={13} aria-hidden className="shrink-0" />
        ) : (
          <FolderOpen size={13} aria-hidden className="shrink-0" />
        )}
        <span className="truncate font-mono text-xs uppercase tracking-wide">
          {folder.name}
        </span>
      </button>
      {collapsed ? null : (
        <>
          {folder.folders.map((sub) => (
            <FolderRow
              key={sub.fullPath}
              folder={sub}
              depth={rest.depth + 1}
              collapsed={rest.collapsed}
              toggle={rest.toggle}
              isCollapsed={rest.isCollapsed}
              testidPrefix={rest.testidPrefix}
            />
          ))}
          {folder.modules.map((mod) => (
            <ModuleRow
              key={mod.fullPath}
              mod={mod}
              depth={rest.depth + 1}
              collapsed={rest.collapsed}
              toggle={rest.toggle}
              isCollapsed={rest.isCollapsed}
              testidPrefix={rest.testidPrefix}
            />
          ))}
        </>
      )}
    </div>
  );
}

function ModuleRow({
  mod,
  ...rest
}: RowProps & {
  mod: ModuleNode;
}) {
  const key = `module:${mod.fullPath}`;
  const collapsed = rest.isCollapsed(key);
  return (
    <div data-testid={`${rest.testidPrefix}-module-${mod.fullPath}`}>
      <button
        type="button"
        onClick={() => rest.toggle(key)}
        aria-expanded={!collapsed}
        className="flex h-7 w-full items-center gap-1 rounded-md px-1 text-left text-muted hover:bg-surface-2 hover:text-default"
        style={{ paddingLeft: `${rest.depth * 12 + 4}px` }}
        data-testid={`${rest.testidPrefix}-module-toggle-${mod.fullPath}`}
      >
        {collapsed ? (
          <ChevronRight size={12} aria-hidden />
        ) : (
          <ChevronDown size={12} aria-hidden />
        )}
        <FileCode size={13} aria-hidden className="shrink-0" />
        <span className="truncate font-mono text-xs">{mod.name}</span>
      </button>
      {collapsed
        ? null
        : mod.functions.map((leaf) => (
            <LeafRow
              key={leaf.path}
              leaf={leaf}
              depth={rest.depth + 1}
              testidPrefix={rest.testidPrefix}
            />
          ))}
    </div>
  );
}

function LeafRow({
  leaf,
  depth,
  testidPrefix,
}: {
  leaf: FunctionLeaf;
  depth: number;
  testidPrefix: string;
}) {
  const Icon = kindIcon(leaf.fnKind);
  return (
    <Link
      to="/developer/compute/$function"
      params={{ function: leaf.path }}
      search={{ tab: "source" }}
      data-testid={`${testidPrefix}-fn-${leaf.path}`}
      className="flex h-7 items-center gap-2 rounded-md px-1 text-sm text-muted hover:bg-surface-2 hover:text-default"
      style={{ paddingLeft: `${depth * 12 + 16}px` }}
    >
      <Icon size={13} aria-hidden className="shrink-0" />
      <span className="flex-1 truncate font-mono text-xs">{leaf.name}</span>
      {leaf.fnKind ? (
        <span className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
          {leaf.fnKind}
        </span>
      ) : leaf.lastStatus ? (
        <span className="tabular font-mono text-xs uppercase tracking-[0.18em] text-muted">
          {leaf.lastStatus}
        </span>
      ) : null}
    </Link>
  );
}
