import { useQuery } from "nimbus/react";
import { Link, useRouterState } from "@tanstack/react-router";

import { NAV_ENTRIES, type NavEntry } from "./nav-entries";
import { cn } from "../lib/cn";

export function Sidebar() {
  return (
    <nav
      aria-label="Primary"
      className="flex h-full w-56 shrink-0 flex-col gap-1 border-r border-app bg-surface px-2 py-3"
    >
      <div className="px-2 pb-3">
        <div className="text-xs font-mono uppercase tracking-[0.18em] text-muted">
          Nimbus
        </div>
        <div className="font-mono text-sm text-default">operator console</div>
      </div>
      <ul className="flex flex-col gap-px" role="list">
        {NAV_ENTRIES.map((entry) => (
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
  const active =
    entry.to === "/"
      ? routerState.location.pathname === "/"
      : routerState.location.pathname.startsWith(entry.to);
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
        style={
          active ? { borderLeftColor: "var(--color-accent)" } : undefined
        }
        aria-current={active ? "page" : undefined}
        data-testid={`nav-${entry.id}`}
      >
        <Icon size={14} aria-hidden className="shrink-0" />
        <span className="flex-1">{entry.label}</span>
        {entry.countQuery ? (
          <NavCount entry={entry} />
        ) : null}
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
      <span
        className="tabular text-xs text-muted"
        aria-label="loading"
        data-testid={`nav-${entry.id}-count-loading`}
      >
        ·
      </span>
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
