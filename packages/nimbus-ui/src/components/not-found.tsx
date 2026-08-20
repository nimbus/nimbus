import { useRouterState } from "@tanstack/react-router";

import { navEntriesForView, viewFromPathname } from "../shell/nav-entries";
import { EmptyState } from "./empty-state";

/**
 * Router-level not-found component. It renders inside the root `<Outlet/>`,
 * so the top nav, primary drawer, tenant selector and status bar stay mounted
 * and the operator keeps a working way out of a stale link.
 */
export function NotFound() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  const home = navEntriesForView(view).find((entry) => entry.to === `/${view}`);
  return (
    <EmptyState
      title="Route not found"
      body={
        <>
          Nothing is mounted at{" "}
          <code className="rounded border border-app bg-surface-2 px-1 font-mono text-default">
            {pathname}
          </code>
          . The link is stale, or the view moved to a different path.
        </>
      }
      cta={{
        label: home ? `Go to ${home.label}` : "Go back",
        to: `/${view}`,
      }}
      testid="route-not-found"
    />
  );
}
