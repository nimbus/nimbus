import { useRouterState } from "@tanstack/react-router";
import { LogoMark } from "./logo-mark";
import { viewFromPathname } from "./nav-entries";
import { TenantSelector, type TenantSelectorMode } from "./tenant-selector";
import { ViewSwitcher } from "./view-switcher";

function selectorModeForRoute(
  pathname: string,
  search: Record<string, unknown> | undefined,
): TenantSelectorMode | null {
  const view = viewFromPathname(pathname);
  if (view === "developer") return { kind: "developer" };
  if (pathname === "/admin/observability") {
    const tenant = search?.tenant;
    return {
      kind: "operator-filter",
      currentFilter: typeof tenant === "string" ? tenant : null,
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
  return (
    <header
      className="flex h-10 shrink-0 items-center gap-4 border-b border-app bg-surface px-3"
      data-testid="top-nav"
      data-view={view}
    >
      <div className="flex items-center gap-2 text-default">
        <LogoMark className="h-6 w-[38px] shrink-0" />
        <div className="flex flex-col leading-tight">
          <span className="text-sm font-medium">Nimbus</span>
          <span
            className="text-[10px] font-mono uppercase tracking-[0.18em] text-muted"
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
        className="flex min-w-[10rem] justify-end"
        data-testid="top-nav-tenant-slot"
        data-mode={mode?.kind ?? "hidden"}
      >
        {mode ? <TenantSelector mode={mode} /> : null}
      </div>
    </header>
  );
}
