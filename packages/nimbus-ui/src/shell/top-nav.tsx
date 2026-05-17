import { useRouterState } from "@tanstack/react-router";
import { LogoMark } from "./logo-mark";
import { viewFromPathname } from "./nav-entries";
import { ViewSwitcher } from "./view-switcher";

export function TopNav() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
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
      <fieldset
        className="flex min-w-[10rem] justify-end"
        data-testid="top-nav-tenant-slot"
        aria-label="Tenant selector"
      >
        <legend className="sr-only">Tenant selector</legend>
        <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
          Tenant
        </span>
      </fieldset>
    </header>
  );
}
