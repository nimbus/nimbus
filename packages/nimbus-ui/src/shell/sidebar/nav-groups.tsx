import { Link, useRouterState } from "@tanstack/react-router";
import { Fragment } from "react";

import { cn } from "@/lib/utils";
import { type NavEntry, type NavView, navGroupsForView } from "../nav-entries";
import { RailTooltip, rowClass } from "./rail";

function isActive(entry: NavEntry, pathname: string): boolean {
  // The view's home page is the only exact match: every other route in the
  // view starts with its prefix, so a prefix test would light it up always.
  if (entry.to === "/developer" || entry.to === "/operator") {
    return pathname === entry.to || pathname === `${entry.to}/`;
  }
  return pathname.startsWith(entry.to);
}

// NavGroups is the middle of the sidebar: the view's rows under their group
// labels. Collapsed, the labels go and a rule stands in for each of them, so
// the rail keeps the same rhythm as the expanded column.
export function NavGroups({
  view,
  collapsed,
  inSheet,
  onNavigate,
}: {
  view: NavView;
  collapsed: boolean;
  inSheet: boolean;
  onNavigate?: () => void;
}) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const groups = navGroupsForView(view);
  return (
    <nav
      aria-label="Primary"
      data-testid="sidebar-nav"
      className={cn(
        "flex flex-1 flex-col overflow-y-auto pt-1",
        collapsed ? "items-center px-3" : "px-2",
      )}
    >
      {groups.map((group, index) => (
        <Fragment key={group.label ?? `unlabelled-${index}`}>
          {index === 0 ? null : collapsed || group.label === null ? (
            <div
              aria-hidden
              className={cn(
                "my-2 border-t border-border-1",
                collapsed ? "w-6" : "mx-3",
              )}
            />
          ) : null}
          {group.label && !collapsed ? (
            <div
              data-testid={`sidebar-group-${group.label.toLowerCase()}`}
              className="px-3 pb-1 pt-3 text-[10px] font-medium uppercase tracking-[0.08em] text-text-4"
            >
              {group.label}
            </div>
          ) : null}
          <ul className="flex flex-col gap-px">
            {group.entries.map((entry) => (
              <li key={entry.id}>
                <NavRow
                  entry={entry}
                  active={isActive(entry, pathname)}
                  collapsed={collapsed}
                  inSheet={inSheet}
                  onNavigate={onNavigate}
                />
              </li>
            ))}
          </ul>
        </Fragment>
      ))}
    </nav>
  );
}

function NavRow({
  entry,
  active,
  collapsed,
  inSheet,
  onNavigate,
}: {
  entry: NavEntry;
  active: boolean;
  collapsed: boolean;
  inSheet: boolean;
  onNavigate?: () => void;
}) {
  const Icon = entry.icon;
  const link = (
    <Link
      to={entry.to}
      data-testid={`nav-${entry.id}`}
      aria-current={active ? "page" : undefined}
      aria-label={collapsed ? entry.label : undefined}
      onClick={onNavigate}
      className={rowClass({ collapsed, inSheet, active })}
    >
      {active ? (
        <span
          aria-hidden
          className="absolute inset-y-1.5 left-0 w-0.5 rounded-full bg-accent-edge"
        />
      ) : null}
      <Icon size={16} aria-hidden className="shrink-0" />
      {collapsed ? null : <span className="truncate">{entry.label}</span>}
    </Link>
  );
  return collapsed ? (
    <RailTooltip label={entry.label}>{link}</RailTooltip>
  ) : (
    link
  );
}
