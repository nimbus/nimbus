import { Link, useRouterState } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { cn } from "../lib/cn";
import { LogoMark } from "./logo-mark";
import {
  type NavEntry,
  navEntriesForView,
  viewFromPathname,
} from "./nav-entries";

export function Sidebar() {
  const routerState = useRouterState();
  const view = viewFromPathname(routerState.location.pathname);
  const entries = navEntriesForView(view);
  return (
    <nav
      aria-label="Primary"
      className="flex h-full w-56 shrink-0 flex-col gap-1 border-r border-app bg-surface px-2 py-3"
      data-view={view}
    >
      <div className="flex items-center gap-2 px-2 pb-3 text-default">
        <LogoMark className="h-6 w-[38px] shrink-0" />
        <div className="flex flex-col leading-tight">
          <span className="text-sm font-medium">Nimbus</span>
          <span className="text-[10px] font-mono uppercase tracking-[0.18em] text-muted">
            {view === "operator" ? "operator console" : "developer console"}
          </span>
        </div>
      </div>
      <ul className="flex flex-col gap-px">
        {entries.map((entry) => (
          <SidebarEntry key={entry.id} entry={entry} />
        ))}
      </ul>
      <div className="mt-auto px-2 pt-3 text-xs text-muted">
        Phase 1 · Embedded SPA
      </div>
    </nav>
  );
}

function SidebarEntry({ entry }: { entry: NavEntry }) {
  const routerState = useRouterState();
  const pathname = routerState.location.pathname;
  const active =
    entry.to === "/app" || entry.to === "/admin"
      ? pathname === entry.to || pathname === `${entry.to}/`
      : pathname.startsWith(entry.to);
  const Icon = entry.icon;
  return (
    <li>
      <Link
        to={entry.to}
        className={cn(
          "group flex h-9 items-center gap-2 rounded-md px-2 text-sm border-l-2 border-transparent",
          active
            ? "bg-surface-2 text-default"
            : "text-muted hover:bg-surface-2 hover:text-default",
        )}
        style={active ? { borderLeftColor: "var(--color-brand)" } : undefined}
        aria-current={active ? "page" : undefined}
        data-testid={`nav-${entry.id}`}
      >
        <Icon size={14} aria-hidden className="shrink-0" />
        <span className="flex-1">{entry.label}</span>
        {entry.countQuery ? <NavCount entry={entry} /> : null}
      </Link>
    </li>
  );
}

function NavCount({ entry }: { entry: NavEntry }) {
  // Cast to satisfy the generic constraint without coupling to the codegen
  // type plumbing.
  const result = useQuery(
    entry.countQuery as never,
    (entry.countArgs ?? undefined) as never,
  ) as unknown[] | Error | undefined;
  const count = Array.isArray(result) ? result.length : undefined;
  if (count === undefined) {
    return (
      <>
        <span
          className="tabular text-xs text-muted"
          aria-hidden="true"
          data-testid={`nav-${entry.id}-count-loading`}
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
      data-testid={`nav-${entry.id}-count`}
    >
      {count}
    </span>
  );
}
