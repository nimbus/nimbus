import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/operator/machines" },
}));

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({
    select,
  }: {
    select: (state: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

const { toastMock } = vi.hoisted(() => ({
  toastMock: Object.assign(vi.fn(), { error: vi.fn() }),
}));

vi.mock("sonner", () => ({ toast: toastMock }));

import { AppErrorBoundary } from "./error-boundary";

// A child that can stop throwing, so a cleared boundary has something to
// re-render. A permanently-throwing child would only prove the boundary
// re-catches.
let shouldThrow = true;
function Shell() {
  if (shouldThrow) throw new Error("TopNav exploded");
  return <div data-testid="shell-ok">shell</div>;
}

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  pathnameRef.current = "/operator/machines";
  shouldThrow = true;
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
  // React re-throws into console.error for every caught boundary error.
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(() => {
  toastMock.mockClear();
  toastMock.error.mockClear();
  vi.restoreAllMocks();
});

describe("AppErrorBoundary", () => {
  it("renders children while nothing throws", () => {
    shouldThrow = false;
    render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    expect(screen.getByTestId("shell-ok")).toBeInTheDocument();
    expect(screen.queryByTestId("error-boundary")).not.toBeInTheDocument();
  });

  it("catches a shell crash and shows the message in a copy chip", () => {
    render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    expect(screen.getByTestId("error-boundary")).toBeInTheDocument();
    expect(screen.getByTestId("error-boundary-copy")).toHaveTextContent(
      "TopNav exploded",
    );
  });

  // happy-dom does no layout, so the utility class is the only observable a
  // unit test can read here. Measured in Chromium against the console's own
  // built stylesheet, with a realistic message ("TypeError: Cannot read
  // properties of undefined (reading 'machines')"): the copy chip is
  // `truncate`, which gave the card a 486.8px min-content floor — wider than
  // its own 480px — so flex-shrink had nothing to give and the card hung 45px
  // off each edge at a 390px viewport and 80px off each edge at 320px. Capping
  // the specified width drops that floor with it: 351px and 288px measured,
  // fully on screen, and still 480px at 1440px.
  it("keeps the crash card inside a phone viewport", () => {
    render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    const classes = screen
      .getByTestId("error-boundary-card")
      .className.split(/\s+/);
    expect(classes).toContain("w-[min(480px,90vw)]");
    expect(classes).not.toContain("w-[480px]");
  });

  it("copies the failing path alongside the error", async () => {
    render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    fireEvent.click(screen.getByTestId("error-boundary-copy"));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const copied = writeText.mock.calls[0][0] as string;
    expect(copied).toContain("/operator/machines");
    expect(copied).toContain("TopNav exploded");
  });

  it("clears the error when the operator navigates away", () => {
    const { rerender } = render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    expect(screen.getByTestId("error-boundary")).toBeInTheDocument();

    shouldThrow = false;
    pathnameRef.current = "/developer/storage";
    rerender(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );

    expect(screen.queryByTestId("error-boundary")).not.toBeInTheDocument();
    expect(screen.getByTestId("shell-ok")).toBeInTheDocument();
  });

  it("keeps the error across a re-render at the same path", () => {
    const { rerender } = render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    shouldThrow = false;
    rerender(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    expect(screen.getByTestId("error-boundary")).toBeInTheDocument();
  });

  it("retries in place without a navigation", () => {
    render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    shouldThrow = false;
    fireEvent.click(screen.getByTestId("error-boundary-retry"));
    expect(screen.getByTestId("shell-ok")).toBeInTheDocument();
  });

  it("offers reloading the console when retry cannot help", () => {
    const reload = vi.fn();
    Object.defineProperty(window, "location", {
      value: { ...window.location, reload },
      configurable: true,
    });
    render(
      <AppErrorBoundary>
        <Shell />
      </AppErrorBoundary>,
    );
    fireEvent.click(screen.getByTestId("error-boundary-reload"));
    expect(reload).toHaveBeenCalledTimes(1);
  });
});
