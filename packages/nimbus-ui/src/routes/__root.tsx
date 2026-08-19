import {
  createRootRoute,
  Outlet,
  useRouterState,
} from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import type { ToastT } from "sonner";
import { Toaster, toast, useSonner } from "sonner";
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
  const [toastRegion, setToastRegion] = useState<HTMLElement | null>(null);
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
            {/* The first tab stop in the console, and the only way past the
                chrome. Everything the shell renders ahead of <main> is a tab
                stop: the build-hash chip, the view switcher, the tenant
                selector, the appearance menu, every primary-drawer link, the
                two collapse buttons, the sub-drawer search, and then the whole
                function tree, one stop per folder, module and leaf. That last
                one has no bound on a real deployment, so without this link
                reaching page content by keyboard is not a fixed cost.

                It is translated off the top of the viewport rather than
                `hidden` or `display: none`, because either of those would take
                it out of the tab order and leave nothing to skip with. */}
            <a
              href="#main-content"
              className="fixed top-2 left-2 z-50 -translate-y-16 rounded border px-3 py-2 text-sm border-app bg-surface text-default focus:translate-y-0"
            >
              Skip to content
            </a>
            <TopNav />
            <div className="flex min-h-0 flex-1">
              <PrimaryDrawer />
              <SubDrawer />
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
            </div>
            <StatusBar />
          </div>
          <CommandPalette />
          <SystemTenantLens />
        </SubDrawerProvider>
        <ToastLifetimes />
        <ToastOverflow region={toastRegion} />
        <Toaster
          ref={setToastRegion}
          position="bottom-right"
          offset="calc(var(--statusbar-height) + 12px)"
          visibleToasts={VISIBLE_TOAST_LIMIT}
          // Never expire a toast on sonner's clock. See ToastLifetimes: this is
          // half of the split, and the half that keeps an error on screen.
          duration={Number.POSITIVE_INFINITY}
          toastOptions={{
            // A toast that never expires has to be closable, and swiping is not
            // a keyboard gesture. Every toast gets the button so the affordance
            // does not appear only on the failures.
            closeButton: true,
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

/** How long a toast that only confirms an action the operator took stays up. */
export const TRANSIENT_TOAST_MS = 4000;

/**
 * How long `entry` may stay on screen, or `null` when this component must not
 * put a clock on it at all.
 */
function transientLifetimeMs(entry: ToastT): number | null {
  // DESIGN.md: "Errors show until dismissed; never auto-disappear." For a
  // rejected tenant create the toast text is the entire failure report, and
  // the bulk-delete partial count (`Deleted 3/5 documents`) is written nowhere
  // else, so an expiring error toast loses the only record of what went wrong.
  if (entry.type === "error") return null;
  // A loading toast ends when its promise settles.
  if (entry.type === "loading") return null;
  // A caller that named its own duration has already answered this, and sonner
  // still runs that toast's timer itself.
  if (entry.duration !== undefined) return null;
  return TRANSIENT_TOAST_MS;
}

/**
 * Gives confirmations a lifetime and errors none.
 *
 * sonner resolves one duration for every toast it renders --
 * `toast.duration || durationFromToaster || TOAST_LIFETIME` -- so the Toaster
 * has no per-type clock to set. Its 4000ms default applied to errors too,
 * which is the rule DESIGN.md writes down and the console was breaking. The
 * Toaster is therefore infinite, and the transient half of the split lives
 * here: a toast that is only confirming an action still goes away by itself.
 *
 * The cost of owning this timer is sonner's pauses: a confirmation no longer
 * stops its clock on hover or while the tab is hidden. That is worth trading
 * for an error that stays, and it costs nothing where it matters -- an error
 * has no clock to pause.
 */
function ToastLifetimes() {
  const { toasts } = useSonner();
  // A Map, not a ref, so the identity is stable without a `.current` read in
  // the cleanup that runs after the component is gone.
  const [timers] = useState(
    () => new Map<ToastT["id"], ReturnType<typeof setTimeout>>(),
  );

  useEffect(() => {
    const live = new Set(toasts.map((entry) => entry.id));
    for (const [id, timer] of timers) {
      if (live.has(id)) continue;
      clearTimeout(timer);
      timers.delete(id);
    }
    for (const entry of toasts) {
      const scheduled = timers.get(entry.id);
      const lifetime = transientLifetimeMs(entry);
      if (lifetime === null) {
        // A toast can change type in place -- `toast.promise` resolves its
        // loading toast into an error under the same id -- so a clock already
        // running on it has to come off.
        if (scheduled !== undefined) {
          clearTimeout(scheduled);
          timers.delete(entry.id);
        }
        continue;
      }
      if (scheduled !== undefined) continue;
      timers.set(
        entry.id,
        setTimeout(() => {
          timers.delete(entry.id);
          toast.dismiss(entry.id);
        }, lifetime),
      );
    }
  }, [toasts, timers]);

  useEffect(
    () => () => {
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
    },
    [timers],
  );

  return null;
}

/**
 * How many toasts sonner keeps on screen at once. DESIGN.md:1054: "Never stack
 * more than three; collapse the rest into '+N more.'"
 *
 * Passed to the Toaster rather than left to sonner's identical default, so the
 * cap and the count of what it hides come from the same number.
 */
const VISIBLE_TOAST_LIMIT = 3;

/**
 * The "+N more" line DESIGN.md:1054 asks for.
 *
 * sonner keeps every toast past the third mounted at `opacity: 0;
 * pointer-events: none`. Hovering the stack does not reveal them and they
 * cannot be clicked shut; they surface only as the visible three are
 * dismissed. That was survivable while every toast expired after four seconds.
 * Errors now stay until dismissed (see ToastLifetimes), so a fourth failure
 * waits off-stack for as long as the operator leaves the first three alone --
 * a failure report the console holds and never shows. This line is the only
 * evidence that there is anything behind the stack.
 *
 * It is portaled into sonner's own list because that is where the geometry
 * is. `--front-toast-height` is the height sonner measured for the front
 * toast, and a collapsed stack lifts each toast behind it by `--gap`, so the
 * top of a full stack sits `--front-toast-height + 2 * --gap` above the
 * list's bottom edge. Anchoring from outside would mean measuring the stack on
 * every change and still lagging its 400ms transitions.
 */
function ToastOverflow({ region }: { region: HTMLElement | null }) {
  const { toasts } = useSonner();
  const [list, setList] = useState<HTMLElement | null>(null);
  const hidden = toasts.length - VISIBLE_TOAST_LIMIT;

  useEffect(() => {
    // sonner drops the list when the stack empties and builds a new one for
    // the next toast, so this cannot be a mount-time lookup. It can key off
    // the overflow count because the list only ever goes away with the last
    // toast, which drives that count below one first.
    if (hidden < 1) {
      setList(null);
      return;
    }
    setList(
      region?.querySelector<HTMLElement>("[data-sonner-toaster]") ?? null,
    );
  }, [region, hidden]);

  if (hidden < 1 || list === null) return null;

  return createPortal(
    // An <li> because the target is an <ol>, and with no live region of its
    // own: sonner's list already sits inside one (aria-live="polite",
    // aria-relevant="additions text"), which announces this line when it
    // appears and again whenever the count changes.
    <li
      data-testid="toast-overflow"
      className="absolute right-0 bottom-[calc(var(--front-toast-height)_+_2_*_var(--gap)_+_8px)] rounded border px-2 py-0.5 font-mono text-xs border-app bg-surface text-muted"
    >
      +{hidden} more
    </li>,
    list,
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
