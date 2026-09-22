import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, setLastViewMock } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer/compute" },
  setLastViewMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  createRootRoute: (config: Record<string, unknown>) => config,
  // Page content, so "the skip link reaches the page" is a claim the test can
  // check rather than a claim about an empty <main>.
  Outlet: () => (
    <button type="button" data-testid="page-action">
      run
    </button>
  ),
  useRouterState: ({
    select,
  }: {
    select: (s: {
      location: { pathname: string };
      matches: { status: string; _notFound?: boolean }[];
    }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current }, matches: [] }),
}));

// The shell's own chrome is stubbed down to one focusable each. The real
// components pull in the router, the tenant API and the connection state, and the
// only thing these tests need from them is that they are tab stops sitting
// between the top of the document and <main>.
vi.mock("../shell/sidebar/sidebar", () => ({
  Sidebar: () => (
    <button type="button" data-testid="chrome-sidebar">
      sidebar
    </button>
  ),
}));
vi.mock("../shell/sidebar/mobile-sheet", () => ({
  MobileTopBar: () => (
    <button type="button" data-testid="chrome-mobile-top-bar">
      mobile top bar
    </button>
  ),
}));
vi.mock("../shell/use-viewport-tier", () => ({
  useSmallScreen: () => false,
  useViewportTier: () => "desktop",
}));
vi.mock("../shell/sub-panel", () => ({
  SubPanelLayout: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
  SubPanelProvider: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
}));
vi.mock("../shell/command-palette", () => ({ CommandPalette: () => null }));
vi.mock("../shell/disconnected-overlay", () => ({
  DisconnectedOverlay: () => null,
}));
vi.mock("../shell/error-boundary", () => ({
  AppErrorBoundary: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
}));
vi.mock("../shell/keyboard-contract", () => ({ KeyboardContract: () => null }));
vi.mock("../shell/nav-entries", () => ({
  viewFromPathname: () => "developer",
}));
vi.mock("../shell/system-tenant-lens", () => ({
  SystemTenantLens: () => null,
}));
vi.mock("../shell/theme-controller", () => ({ ThemeController: () => null }));
vi.mock("../shell/use-tenant-bootstrap", () => ({
  useTenantBootstrap: () => {},
  useTenantSwitchInvalidation: () => {},
}));
vi.mock("../hooks/use-staleness", () => ({
  StalenessProvider: ({ children }: { children: React.ReactNode }) => (
    <>{children}</>
  ),
}));
vi.mock("../store/ui-store", () => ({
  persistLastRouteForView: vi.fn(),
  useUiStore: (select: (s: { setLastView: unknown }) => unknown) =>
    select({ setLastView: setLastViewMock }),
}));

import { TRANSIENT_TOAST_MS, toast } from "../components/toast";
import { routeComponent } from "../test/route-internals";
import { Route } from "./__root";

const ShellLayout = routeComponent(Route);

function skipLink() {
  return screen.getByRole("link", { name: "Skip to content" });
}

describe("shell skip link", () => {
  it("is the first tab stop, ahead of every piece of chrome", async () => {
    const user = userEvent.setup();
    render(<ShellLayout />);

    // Tab order, not `focus()`: happy-dom focuses whatever it is told to, so
    // only sequential navigation answers the question a keyboard user asks.
    await user.tab();
    expect(document.activeElement).toBe(skipLink());

    // The stops it exists to skip are still there and still after it, so the
    // first assertion is about position and not about an empty shell.
    await user.tab();
    expect(document.activeElement).toBe(screen.getByTestId("chrome-sidebar"));
  });

  it("stays in the tab order while it is out of sight", () => {
    render(<ShellLayout />);
    const link = skipLink();

    // `css: false` in the vitest config means computed styles say nothing
    // here, so the classes are the contract. The pair matters: an offscreen
    // transform keeps the link focusable and `focus:` brings it back, while
    // `hidden` or `invisible` would drop it from the tab order and leave a
    // skip link that cannot be reached to skip with.
    expect(link).toHaveClass("-translate-y-16", "focus:translate-y-0");
    expect(link.className).not.toMatch(/\b(hidden|invisible)\b/);
    expect(link).not.toHaveAttribute("hidden");
    expect(link).not.toHaveAttribute("aria-hidden");
  });

  it("points at a <main> that can hold the caret", () => {
    render(<ShellLayout />);
    const main = screen.getByRole("main");

    expect(skipLink()).toHaveAttribute("href", "#main-content");
    expect(main).toHaveAttribute("id", "main-content");
    // Without this the anchor scrolls the page and leaves focus in the
    // chrome, so the next Tab resumes the walk the link just skipped.
    expect(main).toHaveAttribute("tabindex", "-1");
    expect(main).toContainElement(screen.getByTestId("page-action"));
  });
});

/* The toast clock is shared by the two toast blocks below and deliberately not
   installed for the whole file: the skip-link tests drive `userEvent`, which
   waits on a real clock and deadlocks against a fake one. */

function fakeToastClock() {
  // The stack measures each toast in an animation frame before it settles, so
  // a fake clock that only owns `setTimeout` never sees a toast arrive.
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "Date",
      "requestAnimationFrame",
      "cancelAnimationFrame",
    ],
  });
}

function realToastClock() {
  act(() => {
    toast.close();
    vi.runOnlyPendingTimers();
  });
  vi.useRealTimers();
}

// The stack measures a new toast in an animation frame and unmounts a closed
// one after its exit transition, so nothing it does is observable until both
// run. The second pass is not padding: a removal ends in an effect that React
// only flushes when the first `act` returns, and that effect starts the
// unmount timer.
function settle(ms = 0) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
  act(() => {
    vi.advanceTimersByTime(500);
  });
}

// The stack announces every toast a second time through a visually hidden
// live region, so a text query has to pick the copy inside a toast root.
function toastNamed(text: string): HTMLElement | null {
  for (const node of screen.queryAllByText(text)) {
    const root = node.closest<HTMLElement>('[data-slot="toast"]');
    if (root) return root;
  }
  return null;
}

describe("toast lifetimes", () => {
  beforeEach(fakeToastClock);
  afterEach(realToastClock);

  it("keeps an error toast up long past the transient lifetime", () => {
    render(<ShellLayout />);
    act(() => {
      toast.error("tenant id rejected: already in use");
    });
    settle();
    expect(toastNamed("tenant id rejected: already in use")).not.toBeNull();

    settle(TRANSIENT_TOAST_MS * 10);

    // DESIGN.md: "Errors show until dismissed; never auto-disappear." On
    // /operator/tenants this toast is the whole failure report.
    expect(toastNamed("tenant id rejected: already in use")).not.toBeNull();
  });

  it("still expires a toast that only confirms an action", () => {
    render(<ShellLayout />);
    act(() => {
      toast.success("Started machine-01");
    });
    settle();
    expect(toastNamed("Started machine-01")).not.toBeNull();

    settle(TRANSIENT_TOAST_MS + 1000);

    expect(toastNamed("Started machine-01")).toBeNull();
  });

  it("gives a persistent error a way to be dismissed", () => {
    render(<ShellLayout />);
    act(() => {
      toast.error("upgrade failed: checksum mismatch");
    });
    settle();

    // A toast with no clock has to be closable by hand, and swiping is not a
    // keyboard gesture.
    act(() => {
      fireEvent.click(screen.getByLabelText("Close toast"));
    });
    settle(1000);

    expect(toastNamed("upgrade failed: checksum mismatch")).toBeNull();
  });

  it("leaves a caller that named its own lifetime alone", () => {
    render(<ShellLayout />);
    act(() => {
      // use-staleness.ts pins the upgrade-available toast open this way.
      toast.message("Nimbus 0.1.46 is available", { timeout: 0 });
    });
    settle();

    settle(TRANSIENT_TOAST_MS * 10);

    expect(toastNamed("Nimbus 0.1.46 is available")).not.toBeNull();
  });
});

describe("toast overflow", () => {
  beforeEach(fakeToastClock);
  afterEach(realToastClock);

  /* Three is the cap DESIGN.md sets, so the tests count against the document
     rather than against the constant in the toast module. */
  function raise(count: number) {
    act(() => {
      for (let n = 1; n <= count; n += 1) toast.error(`write ${n} failed`);
    });
    settle();
  }

  it("stays quiet while the whole stack is on screen", () => {
    render(<ShellLayout />);
    raise(3);

    expect(toastNamed("write 3 failed")).not.toBeNull();
    expect(screen.queryByTestId("toast-overflow")).not.toBeInTheDocument();
  });

  it("reports the errors the stack is holding off screen", () => {
    render(<ShellLayout />);
    raise(5);

    // The premise: the two oldest are mounted but transparent and inert, and
    // hovering the stack does not bring them back. Without a count they are
    // failures the console was told about and never showed.
    for (const text of ["write 1 failed", "write 2 failed"]) {
      expect(toastNamed(text)).toHaveAttribute("data-limited");
    }

    expect(screen.getByTestId("toast-overflow")).toHaveTextContent("+2 more");
  });

  it("counts down as the operator clears the stack", () => {
    render(<ShellLayout />);
    raise(5);

    // Index 0 is the front toast: the stack prepends, so the newest is on top
    // and the backlog is what falls out of sight.
    act(() => {
      fireEvent.click(screen.getAllByLabelText("Close toast")[0]);
    });
    settle(1000);

    expect(toastNamed("write 4 failed")).not.toBeNull();
    expect(screen.getByTestId("toast-overflow")).toHaveTextContent("+1 more");
  });

  it("stops counting once nothing is left behind the stack", () => {
    render(<ShellLayout />);
    raise(4);
    expect(screen.getByTestId("toast-overflow")).toHaveTextContent("+1 more");

    act(() => {
      fireEvent.click(screen.getAllByLabelText("Close toast")[0]);
    });
    settle(1000);

    expect(screen.queryByTestId("toast-overflow")).not.toBeInTheDocument();
  });

  it("sits above the stack it is counting", () => {
    render(<ShellLayout />);
    raise(4);
    const line = screen.getByTestId("toast-overflow");

    // Inside the stack's viewport, which is the only place the front toast's
    // live height is readable. A fixed offset outside it would overlap a
    // wrapped error toast.
    expect(line.closest('[data-slot="toast-viewport"]')).not.toBeNull();
    expect(line.className).toContain("bottom-(--overflow-offset)");
    expect(line.style.getPropertyValue("--overflow-offset")).toContain(
      "--toast-frontmost-height",
    );
    // Hovering expands the stack to full height, and the line climbs with it.
    expect(line.className).toContain(
      "in-data-expanded:bottom-(--overflow-offset-expanded)",
    );
  });
});
