import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invalidateMock, pathnameRef } = vi.hoisted(() => ({
  invalidateMock: vi.fn(),
  pathnameRef: { current: "/operator/machines" },
}));

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    ...rest
  }: {
    to: string;
    children: React.ReactNode;
  } & React.AnchorHTMLAttributes<HTMLAnchorElement>) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
  useRouter: () => ({ invalidate: invalidateMock }),
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

import { RouteError } from "./route-error";

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  pathnameRef.current = "/operator/machines";
  invalidateMock.mockReset().mockResolvedValue(undefined);
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

afterEach(() => {
  toastMock.mockClear();
  toastMock.error.mockClear();
});

describe("RouteError", () => {
  it("names the failing route and the raw message", () => {
    render(<RouteError error={new Error("rows.map is not a function")} />);
    const body = screen.getByTestId("route-error-state-body");
    expect(body.querySelector("code")).toHaveTextContent("/operator/machines");
    expect(screen.getByTestId("route-error-copy")).toHaveTextContent(
      "rows.map is not a function",
    );
  });

  it("makes the error details copyable", async () => {
    render(<RouteError error={new Error("boom")} />);
    fireEvent.click(screen.getByTestId("route-error-copy"));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const copied = writeText.mock.calls[0][0] as string;
    expect(copied).toContain("/operator/machines");
    expect(copied).toContain("boom");
  });

  it("retries with a real router state change, not a bare re-mount", () => {
    const reset = vi.fn();
    render(<RouteError error={new Error("boom")} reset={reset} />);
    fireEvent.click(screen.getByTestId("route-error-retry"));
    expect(reset).toHaveBeenCalledTimes(1);
    expect(invalidateMock).toHaveBeenCalledTimes(1);
  });

  it("retries without a reset callback (loader errors supply none)", () => {
    render(<RouteError error={new Error("boom")} />);
    fireEvent.click(screen.getByTestId("route-error-retry"));
    expect(invalidateMock).toHaveBeenCalledTimes(1);
  });

  it("offers reloading the console as the honest fallback", () => {
    const reload = vi.fn();
    Object.defineProperty(window, "location", {
      value: { ...window.location, reload },
      configurable: true,
    });
    render(<RouteError error={new Error("boom")} />);
    fireEvent.click(screen.getByTestId("route-error-reload"));
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("renders a non-Error throw without crashing", () => {
    render(<RouteError error="plain string throw" />);
    expect(screen.getByTestId("route-error-copy")).toHaveTextContent(
      "plain string throw",
    );
  });
});
