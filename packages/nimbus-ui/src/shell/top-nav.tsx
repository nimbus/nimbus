import { useQuery } from "@nimbus/nimbus/react";
import { useRouterState } from "@tanstack/react-router";
import { api } from "../../convex/_generated/api";
import { CopyChip } from "../components/copy-chip";
import { AppearanceMenu } from "./appearance-menu";
import { LogoMark } from "./logo-mark";
import { viewFromPathname } from "./nav-entries";
import { EVENTS_TABLE_HAS_TENANT_COLUMN } from "./tenant-scope";
import { TenantSelector, type TenantSelectorMode } from "./tenant-selector";
import { ViewSwitcher } from "./view-switcher";

function selectorModeForRoute(
  pathname: string,
  search: Record<string, unknown> | undefined,
): TenantSelectorMode | null {
  const view = viewFromPathname(pathname);
  if (view === "developer") return { kind: "developer" };
  if (pathname === "/operator/observability") {
    const tenant = search?.tenant;
    return {
      kind: "operator-filter",
      currentFilter: typeof tenant === "string" ? tenant : null,
      // One source of truth with the observability route: while the events
      // table has no tenant column the control renders inert rather than
      // pretending to narrow scope it cannot narrow.
      unavailable: !EVENTS_TABLE_HAS_TENANT_COLUMN,
    };
  }
  return null;
}

export function TopNav() {
  const { pathname, search } = useRouterState({
    select: (s) => ({
      pathname: s.location.pathname,
      search: s.location.search as Record<string, unknown> | undefined,
    }),
  });
  const view = viewFromPathname(pathname);
  const mode = selectorModeForRoute(pathname, search);
  const status = useQuery(api.system.status, {}) as
    | { version?: string; buildHash?: string | null }
    | null
    | undefined;
  const version = status?.version;
  const buildHash = status?.buildHash ?? null;
  return (
    <header
      className="flex h-10 shrink-0 items-center gap-4 border-b border-app bg-surface px-3"
      data-testid="top-nav"
      data-view={view}
    >
      <div className="flex items-center gap-2 text-default">
        <LogoMark className="h-6 w-[38px] shrink-0" />
        <div className="flex flex-col leading-tight">
          <span className="text-sm">
            <span className="font-semibold">nimbus</span>
            {version ? (
              buildHash ? (
                // Two servers can report the same version and run different
                // code. The short hash identifies the build; the chip copies
                // the full hash, which is what an operator pastes into a bug
                // report.
                <CopyChip
                  label="build hash"
                  value={buildHash}
                  testid="top-nav-version"
                  className="ml-1 text-muted"
                >
                  v{version}
                  <span className="text-muted/70">
                    +{buildHash.slice(0, 7)}
                  </span>
                </CopyChip>
              ) : (
                <span
                  className="ml-1 font-mono text-muted"
                  data-testid="top-nav-version"
                >
                  v{version}
                </span>
              )
            ) : null}
          </span>
          <span
            className="text-xs font-mono uppercase tracking-[0.18em] text-muted"
            data-testid="top-nav-wordmark"
          >
            {view === "operator" ? "operator console" : "developer console"}
          </span>
        </div>
      </div>
      <div className="flex flex-1 justify-center">
        <ViewSwitcher />
      </div>
      <div
        className="flex min-w-[10rem] items-center justify-end gap-2"
        data-testid="top-nav-tenant-slot"
        data-mode={mode?.kind ?? "hidden"}
      >
        {mode ? <TenantSelector mode={mode} /> : null}
        {/* Appearance is a per-user preference, so it belongs to the shell and
            not to Operator settings — a Developer-console user has no route
            into the operator console. */}
        <AppearanceMenu />
      </div>
    </header>
  );
}
