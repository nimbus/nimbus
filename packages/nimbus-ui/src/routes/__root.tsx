import {
  createRootRoute,
  Outlet,
  useRouterState,
} from "@tanstack/react-router";
import { useEffect } from "react";

import { Toaster } from "@/components/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { StalenessProvider } from "../hooks/use-staleness";
import { CommandPalette } from "../shell/command-palette";
import { DisconnectedOverlay } from "../shell/disconnected-overlay";
import { AppErrorBoundary } from "../shell/error-boundary";
import { KeyboardContract } from "../shell/keyboard-contract";
import { viewFromPathname } from "../shell/nav-entries";
import { MobileTopBar } from "../shell/sidebar/mobile-sheet";
import { Sidebar } from "../shell/sidebar/sidebar";
import { SubPanelLayout, SubPanelProvider } from "../shell/sub-panel";
import { SystemTenantLens } from "../shell/system-tenant-lens";
import { ThemeController } from "../shell/theme-controller";
import {
  useTenantBootstrap,
  useTenantSwitchInvalidation,
} from "../shell/use-tenant-bootstrap";
import { useSmallScreen } from "../shell/use-viewport-tier";
import { persistLastRouteForView, useUiStore } from "../store/ui-store";

type RootSearch = {
  as?: string;
};

export const Route = createRootRoute({
  component: ShellLayout,
  validateSearch: (search: Record<string, unknown>): RootSearch => ({
    as: typeof search.as === "string" ? search.as : undefined,
  }),
});

function ShellLayout() {
  // Below 640px the sidebar is a sheet behind a top-bar button; above it,
  // the column. The tree differs on either side of the line, so the choice
  // is made here and not in a stylesheet.
  const small = useSmallScreen();
  useLastRouteTracker();
  useTenantBootstrap();
  useTenantSwitchInvalidation();
  return (
    <AppErrorBoundary>
      <ThemeController />
      <KeyboardContract />
      <StalenessProvider>
        <TooltipProvider>
          <SubPanelProvider>
            <div className="flex h-screen flex-col bg-bg-canvas text-text-1">
              {/* The first tab stop in the console, and the only way past the
                chrome. Everything the shell renders ahead of <main> is a tab
                stop: the brand link, the view switcher, the tenant selector,
                every sidebar row, the theme toggle, the two collapse buttons,
                the sub-panel search, and then the whole function tree, one
                stop per folder, module and leaf. That last
                one has no bound on a real deployment, so without this link
                reaching page content by keyboard is not a fixed cost.

                It is translated off the top of the viewport rather than
                `hidden` or `display: none`, because either of those would take
                it out of the tab order and leave nothing to skip with. */}
              <a
                href="#main-content"
                className="fixed top-2 left-2 z-50 -translate-y-16 rounded-xs border px-3 py-2 text-sm border-border-2 bg-bg-panel text-text-1 focus:translate-y-0"
              >
                Skip to content
              </a>
              {small ? <MobileTopBar /> : null}
              <div className="flex min-h-0 flex-1">
                {small ? null : <Sidebar />}
                <SubPanelLayout>
                  {/* `tabIndex={-1}` is what moves the caret. An anchor to a
                    container that cannot hold focus scrolls the page in every
                    browser but leaves the next Tab back in the chrome, which is
                    the walk the link exists to avoid. */}
                  <main
                    id="main-content"
                    tabIndex={-1}
                    className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
                  >
                    <DisconnectedOverlay />
                    <div className="flex-1 overflow-auto">
                      <Outlet />
                    </div>
                  </main>
                </SubPanelLayout>
              </div>
            </div>
            <CommandPalette />
            <SystemTenantLens />
          </SubPanelProvider>
          <Toaster />
        </TooltipProvider>
      </StalenessProvider>
    </AppErrorBoundary>
  );
}

export function useLastRouteTracker() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  // A location that matches no route must not become the view's remembered
  // route: persisting it makes the dead end self-restoring on the next reload
  // and on every view switch.
  // `_notFound` is the flag the router sets on the match that renders the
  // global not-found component. It is the same predicate router-core uses
  // for its own first-error lookup.
  const isNotFound = useRouterState({
    select: (s) =>
      s.matches.some(
        (match) => match.status === "notFound" || match._notFound === true,
      ),
  });
  const setLastView = useUiStore((s) => s.setLastView);
  useEffect(() => {
    const view = viewFromPathname(pathname);
    if (!isNotFound) {
      persistLastRouteForView(view, pathname);
    }
    setLastView(view);
  }, [pathname, isNotFound, setLastView]);
}
