import { Outlet, createRootRoute } from "@tanstack/react-router";
import { Toaster } from "sonner";

import { Sidebar } from "../shell/sidebar";
import { StatusBar } from "../shell/status-bar";
import { CommandPalette } from "../shell/command-palette";
import { SystemTenantLens } from "../shell/system-tenant-lens";
import { KeyboardContract } from "../shell/keyboard-contract";
import { AppErrorBoundary } from "../shell/error-boundary";
import { DisconnectedOverlay } from "../shell/disconnected-overlay";
import { ThemeController } from "../shell/theme-controller";

export const Route = createRootRoute({
  component: ShellLayout,
});

function ShellLayout() {
  return (
    <AppErrorBoundary>
      <ThemeController />
      <KeyboardContract />
      <div className="flex h-screen flex-col bg-app text-default">
        <div className="flex min-h-0 flex-1">
          <Sidebar />
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
    </AppErrorBoundary>
  );
}
