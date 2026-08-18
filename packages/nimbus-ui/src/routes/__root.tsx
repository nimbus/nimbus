import {
  createRootRoute,
  Outlet,
  useRouterState,
} from "@tanstack/react-router";
import { useEffect } from "react";
import { Toaster } from "sonner";
import { StalenessProvider } from "../hooks/use-staleness";
import { CommandPalette } from "../shell/command-palette";
import { DisconnectedOverlay } from "../shell/disconnected-overlay";
import { AppErrorBoundary } from "../shell/error-boundary";
import { KeyboardContract } from "../shell/keyboard-contract";
import { viewFromPathname } from "../shell/nav-entries";
import { PrimaryDrawer } from "../shell/primary-drawer";
import { StatusBar } from "../shell/status-bar";
import { SubDrawer, SubDrawerProvider } from "../shell/sub-drawer";
import { SystemTenantLens } from "../shell/system-tenant-lens";
import { ThemeController } from "../shell/theme-controller";
import { TopNav } from "../shell/top-nav";
import {
  useTenantBootstrap,
  useTenantSwitchInvalidation,
} from "../shell/use-tenant-bootstrap";
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
  useLastRouteTracker();
  useTenantBootstrap();
  useTenantSwitchInvalidation();
  return (
    <AppErrorBoundary>
      <ThemeController />
      <KeyboardContract />
      <StalenessProvider>
        <SubDrawerProvider>
          <div className="flex h-screen flex-col bg-canvas text-default">
            <TopNav />
            <div className="flex min-h-0 flex-1">
              <PrimaryDrawer />
              <SubDrawer />
              <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
                <DisconnectedOverlay />
                <div className="flex-1 overflow-auto">
                  <Outlet />
                </div>
              </main>
            </div>
            <StatusBar />
          </div>
          <CommandPalette />
          <SystemTenantLens />
        </SubDrawerProvider>
        <Toaster
          position="bottom-right"
          offset="calc(var(--statusbar-height) + 12px)"
          toastOptions={{
            style: {
              background: "var(--nimbus-surface)",
              color: "var(--nimbus-text)",
              border: "1px solid var(--nimbus-border)",
              fontFamily: "var(--font-mono)",
              fontSize: "12px",
            },
          }}
        />
      </StalenessProvider>
    </AppErrorBoundary>
  );
}

export function useLastRouteTracker() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  // A location that matches no route must not become the view's remembered
  // route: persisting it makes the dead end self-restoring on the next reload
  // and on every view switch.
  const isNotFound = useRouterState({
    select: (s) =>
      s.matches.some(
        (match) => match.status === "notFound" || match.globalNotFound === true,
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
