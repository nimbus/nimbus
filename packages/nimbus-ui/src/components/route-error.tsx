import { useRouter, useRouterState } from "@tanstack/react-router";

import { CopyChip } from "./copy-chip";
import { EmptyState } from "./empty-state";

const ACTION_CLASS =
  "rounded border border-app px-3 py-1 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default";

export type RouteErrorProps = {
  error: unknown;
  // The router omits `reset` for loader errors, so it is optional at runtime
  // even though the router's type declares it required.
  reset?: () => void;
  info?: { componentStack: string };
};

/**
 * Router-level error component. Registered as `defaultErrorComponent`, so the
 * catch boundary sits inside the root `<Outlet/>`: only the failing view is
 * replaced and the shell chrome stays live. Retry invalidates the router,
 * which re-runs loaders and bumps the boundary reset key — a real state
 * change, not a bare re-mount of the same crashing subtree.
 */
export function RouteError({ error, reset }: RouteErrorProps) {
  const router = useRouter();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const message = error instanceof Error ? error.message : String(error);
  const stack = error instanceof Error ? error.stack : undefined;
  const details = `route: ${pathname}\nerror: ${message}${stack ? `\n${stack}` : ""}`;
  return (
    <section
      role="alert"
      data-testid="route-error"
      className="flex h-full flex-col items-center justify-center gap-3 px-6"
    >
      <EmptyState
        className="h-auto"
        title="This view failed to render"
        body={
          <>
            <code className="rounded border border-app bg-surface-2 px-1 font-mono text-default">
              {pathname}
            </code>{" "}
            threw while rendering. Navigation still works — every other view is
            unaffected.
          </>
        }
        testid="route-error-state"
      />
      <CopyChip
        label="error details"
        value={details}
        testid="route-error-copy"
        className="max-w-[64ch] border border-app px-2 py-1 text-danger"
      >
        {message}
      </CopyChip>
      <div className="flex gap-2">
        <button
          type="button"
          className={ACTION_CLASS}
          data-testid="route-error-retry"
          onClick={() => {
            reset?.();
            void router.invalidate();
          }}
        >
          Retry
        </button>
        <button
          type="button"
          className={ACTION_CLASS}
          data-testid="route-error-reload"
          onClick={() => window.location.reload()}
        >
          Reload console
        </button>
      </div>
    </section>
  );
}
