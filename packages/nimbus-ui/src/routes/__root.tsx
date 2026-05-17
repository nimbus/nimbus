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
import { SystemTenantLens } from "../shell/system-tenant-lens";
import { ThemeController } from "../shell/theme-controller";
import { TopNav } from "../shell/top-nav";
import { persistLastRouteForView, useUiStore } from "../store/ui-store";

export const Route = createRootRoute({
  component: ShellLayout,
});

function ShellLayout() {
  useLastRouteTracker();
  return (
    <AppErrorBoundary>
      <ThemeController />
      <KeyboardContract />
      <StalenessProvider>
        <div className="flex h-screen flex-col bg-app text-default">
          <TopNav />
          <div className="flex min-h-0 flex-1">
            <PrimaryDrawer />
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
        <Toaster
          position="bottom-right"
          toastOptions={{
            style: {
              background: "var(--color-surface)",
              color: "var(--color-text)",
              border: "1px solid var(--color-border)",
              fontFamily: "var(--font-mono)",
              fontSize: "12px",
            },
          }}
        />
      </StalenessProvider>
    </AppErrorBoundary>
  );
}

function useLastRouteTracker() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const setLastView = useUiStore((s) => s.setLastView);
  useEffect(() => {
    const view = viewFromPathname(pathname);
    persistLastRouteForView(view, pathname);
    setLastView(view);
  }, [pathname, setLastView]);
}
