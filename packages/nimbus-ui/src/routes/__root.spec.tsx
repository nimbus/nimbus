import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
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
      matches: { status: string; globalNotFound?: boolean }[];
    }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current }, matches: [] }),
}));

// The shell's own chrome is stubbed down to one focusable each. The real
// components pull in the router, the tenant API and the nav counts, and the
// only thing these tests need from them is that they are tab stops sitting
// between the top of the document and <main>.
vi.mock("../shell/top-nav", () => ({
  TopNav: () => (
    <button type="button" data-testid="chrome-top-nav">
      tenant
    </button>
  ),
}));
vi.mock("../shell/primary-drawer", () => ({
  PrimaryDrawer: () => (
    <button type="button" data-testid="chrome-primary-drawer">
      compute
    </button>
  ),
}));
vi.mock("../shell/sub-drawer", () => ({
  SubDrawer: () => (
    <button type="button" data-testid="chrome-sub-drawer">
      search
    </button>
  ),
  SubDrawerProvider: ({ children }: { children: React.ReactNode }) => (
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
vi.mock("../shell/status-bar", () => ({ StatusBar: () => null }));
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

import { routeComponent } from "../test/route-internals";
import { Route, TRANSIENT_TOAST_MS } from "./__root";

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
    expect(document.activeElement).toBe(screen.getByTestId("chrome-top-nav"));
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
  // sonner hands its removals to an animation frame, so a fake clock that only
  // owns `setTimeout` starts a dismissal it can never finish.
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
    toast.dismiss();
    vi.runOnlyPendingTimers();
  });
  vi.useRealTimers();
}

// sonner defers every state update through a timer and its removals through an
// animation frame, so nothing it does is observable until both run. The second
// pass is not padding: a removal ends in an effect that React only flushes when
// the first `act` returns, and that effect starts the 200ms unmount timer.
function settle(ms = 0) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
  act(() => {
    vi.advanceTimersByTime(500);
  });
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
    expect(
      screen.getByText("tenant id rejected: already in use"),
    ).toBeInTheDocument();

    settle(TRANSIENT_TOAST_MS * 10);

    // DESIGN.md: "Errors show until dismissed; never auto-disappear." On
    // /operator/tenants this toast is the whole failure report.
    expect(
      screen.getByText("tenant id rejected: already in use"),
    ).toBeInTheDocument();
  });

  it("still expires a toast that only confirms an action", () => {
    render(<ShellLayout />);
    act(() => {
      toast.success("Started machine-01");
    });
    settle();
    expect(screen.getByText("Started machine-01")).toBeInTheDocument();

    settle(TRANSIENT_TOAST_MS + 1000);

    expect(screen.queryByText("Started machine-01")).not.toBeInTheDocument();
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

    expect(
      screen.queryByText("upgrade failed: checksum mismatch"),
    ).not.toBeInTheDocument();
  });

  it("leaves a caller that named its own duration alone", () => {
    render(<ShellLayout />);
    act(() => {
      // use-staleness.ts pins the upgrade-available toast open this way.
      toast("Nimbus 0.1.46 is available", {
        duration: Number.POSITIVE_INFINITY,
      });
    });
    settle();

    settle(TRANSIENT_TOAST_MS * 10);

    expect(screen.getByText("Nimbus 0.1.46 is available")).toBeInTheDocument();
  });
});

describe("toast overflow", () => {
  beforeEach(fakeToastClock);
  afterEach(realToastClock);

  /* Three is the cap DESIGN.md:1054 sets, so the tests count against the
     document rather than against the constant in the route. */
  function raise(count: number) {
    act(() => {
      for (let n = 1; n <= count; n += 1) toast.error(`write ${n} failed`);
    });
    settle();
  }

  it("stays quiet while the whole stack is on screen", () => {
    render(<ShellLayout />);
    raise(3);

    expect(screen.getByText("write 3 failed")).toBeInTheDocument();
    expect(screen.queryByTestId("toast-overflow")).not.toBeInTheDocument();
  });

  it("reports the errors sonner is holding off the stack", () => {
    render(<ShellLayout />);
    raise(5);

    // The premise: the two oldest are mounted but invisible and unclickable,
    // and hovering the stack does not bring them back. Without a count they
    // are failures the console was told about and never showed.
    for (const text of ["write 1 failed", "write 2 failed"]) {
      expect(
        screen.getByText(text).closest("[data-sonner-toast]"),
      ).toHaveAttribute("data-visible", "false");
    }

    expect(screen.getByTestId("toast-overflow")).toHaveTextContent("+2 more");
  });

  it("counts down as the operator clears the stack", () => {
    render(<ShellLayout />);
    raise(5);

    // Index 0 is the front toast: the Toaster prepends, so the newest is on
    // top and the backlog is what falls out of sight.
    act(() => {
      fireEvent.click(screen.getAllByLabelText("Close toast")[0]);
    });
    settle(1000);

    expect(screen.getByText("write 4 failed")).toBeInTheDocument();
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

    // Inside sonner's list, which is the only place the stack's live height is
    // readable. A fixed offset outside it would overlap a wrapped error toast.
    expect(line.closest("[data-sonner-toaster]")).not.toBeNull();
    expect(line.className).toContain(
      "bottom-[calc(var(--front-toast-height)_+_2_*_var(--gap)_+_8px)]",
    );
  });
});
