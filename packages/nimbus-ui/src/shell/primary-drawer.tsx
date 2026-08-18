import { Link, useRouterState } from "@tanstack/react-router";
import { ChevronsLeft, ChevronsRight } from "lucide-react";
import { useQuery } from "@nimbus/nimbus/react";
import { api } from "../../convex/_generated/api";
import { useTenantList } from "../hooks/use-tenant-list";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/ui-store";
import {
  type NavCountEntry,
  type NavCountKind,
  type NavEntry,
  navEntriesForView,
  viewFromPathname,
} from "./nav-entries";

const NAV_ID = "primary-drawer-nav";

// Background double-click toggles collapse; ignore double-clicks on interactive
// elements (links, buttons) so they keep their own behavior.
function isInteractiveTarget(target: EventTarget | null): boolean {
  return Boolean(
    (target as HTMLElement | null)?.closest("a,button,input,textarea,select"),
  );
}

export function PrimaryDrawer() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  const entries = navEntriesForView(view);
  const collapsed = useUiStore((s) => s.primaryDrawerCollapsed);
  const togglePrimaryDrawer = useUiStore((s) => s.togglePrimaryDrawer);
  return (
    <nav
      id={NAV_ID}
      aria-label="Primary"
      onDoubleClick={(e) => {
        if (!isInteractiveTarget(e.target)) togglePrimaryDrawer();
      }}
      className={cn(
        "flex h-full shrink-0 flex-col gap-1 border-r border-app bg-surface py-3 transition-[width] duration-150",
        collapsed ? "w-12 px-1" : "w-56 px-2",
      )}
      data-view={view}
      data-collapsed={collapsed ? "true" : "false"}
      data-testid="primary-drawer"
    >
      <ul className="flex flex-col gap-px">
        {entries.map((entry) => (
          <DrawerEntry key={entry.id} entry={entry} collapsed={collapsed} />
        ))}
      </ul>
      <div className="mt-auto flex flex-col gap-2">
        <button
          type="button"
          onClick={togglePrimaryDrawer}
          aria-expanded={!collapsed}
          aria-controls={NAV_ID}
          aria-label={collapsed ? "Expand navigation" : "Collapse navigation"}
          title={collapsed ? "Expand navigation" : "Collapse navigation"}
          data-testid="primary-drawer-toggle"
          className={cn(
            "flex h-8 items-center gap-2 rounded-md text-xs text-muted transition-colors hover:bg-surface-2 hover:text-default",
            collapsed ? "justify-center px-0" : "px-2",
          )}
        >
          {collapsed ? (
            <ChevronsRight size={14} aria-hidden />
          ) : (
            <ChevronsLeft size={14} aria-hidden />
          )}
          {collapsed ? null : <span>Collapse</span>}
        </button>
      </div>
    </nav>
  );
}

function DrawerEntry({
  entry,
  collapsed,
}: {
  entry: NavEntry;
  collapsed: boolean;
}) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const active =
    entry.to === "/developer" || entry.to === "/operator"
      ? pathname === entry.to || pathname === `${entry.to}/`
      : pathname.startsWith(entry.to);
  const Icon = entry.icon;
  return (
    <li>
      <Link
        to={entry.to}
        title={collapsed ? entry.label : undefined}
        className={cn(
          "group flex h-9 items-center rounded-md border-l-2 border-transparent text-sm",
          collapsed ? "justify-center px-0" : "gap-2 px-2",
          active
            ? "bg-surface-2 text-default"
            : "text-muted hover:bg-surface-2 hover:text-default",
        )}
        style={active ? { borderLeftColor: "var(--nimbus-brand)" } : undefined}
        aria-current={active ? "page" : undefined}
        aria-label={collapsed ? entry.label : undefined}
        data-testid={`nav-${entry.id}`}
      >
        <Icon size={14} aria-hidden className="shrink-0" />
        {collapsed ? null : (
          <>
            <span className="flex-1">{entry.label}</span>
            {entry.count ? (
              <QueryNavCount id={entry.id} count={entry.count} />
            ) : entry.countKind ? (
              <SpecialNavCount id={entry.id} kind={entry.countKind} />
            ) : null}
          </>
        )}
      </Link>
    </li>
  );
}

function CountBadge({ id, value }: { id: string; value: number | undefined }) {
  if (value === undefined) {
    return (
      <>
        <span
          className="tabular text-xs text-muted"
          aria-hidden="true"
          data-testid={`nav-${id}-count-loading`}
        >
          ·
        </span>
        <span className="sr-only">loading</span>
      </>
    );
  }
  return (
    <span
      className="tabular font-mono text-xs text-muted"
      data-testid={`nav-${id}-count`}
    >
      {value}
    </span>
  );
}

// Reactive convex array-length count (machines, services, routes, runs, …).
function QueryNavCount({ id, count }: { id: string; count: NavCountEntry }) {
  const result = useQuery(count.ref, count.args);
  return <CountBadge id={id} value={result?.length} />;
}

// Non-query count sources. Each is its own component so hooks stay
// unconditional (one source per render path).
function SpecialNavCount({ id, kind }: { id: string; kind: NavCountKind }) {
  return kind === "tenants" ? (
    <TenantsNavCount id={id} />
  ) : (
    <NodesNavCount id={id} />
  );
}

function TenantsNavCount({ id }: { id: string }) {
  const state = useTenantList();
  return (
    <CountBadge
      id={id}
      value={state.kind === "loaded" ? state.tenants.length : undefined}
    />
  );
}

function NodesNavCount({ id }: { id: string }) {
  // Single-node deployment today: any status response means one live node.
  const status = useQuery(api.system.status, {});
  return <CountBadge id={id} value={status === undefined ? undefined : 1} />;
}
