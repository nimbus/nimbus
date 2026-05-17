import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { navigateMock, pathnameRef } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  pathnameRef: { current: "/app" },
}));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigateMock,
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

import { ViewSwitcher } from "./view-switcher";

function setPathname(path: string) {
  pathnameRef.current = path;
}

beforeEach(() => {
  navigateMock.mockClear();
  setPathname("/app");
});

afterEach(() => {
  navigateMock.mockReset();
});

describe("ViewSwitcher", () => {
  it("marks the developer segment active on /app pathnames", () => {
    setPathname("/app/compute");
    render(<ViewSwitcher />);
    expect(screen.getByTestId("view-switcher-developer")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByTestId("view-switcher-operator")).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("marks the operator segment active on /admin pathnames", () => {
    setPathname("/admin/machines");
    render(<ViewSwitcher />);
    expect(screen.getByTestId("view-switcher-operator")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByTestId("view-switcher-developer")).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("navigates to /admin default on first switch from a developer route with no stored operator route", () => {
    setPathname("/app/compute");
    render(<ViewSwitcher />);
    fireEvent.click(screen.getByTestId("view-switcher-operator"));
    expect(navigateMock).toHaveBeenCalledWith({ to: "/admin" });
  });

  it("persists the current pathname under nimbus-ui:last-route:<view> on switch", () => {
    setPathname("/app/compute");
    render(<ViewSwitcher />);
    fireEvent.click(screen.getByTestId("view-switcher-operator"));
    expect(window.localStorage.getItem("nimbus-ui:last-route:developer")).toBe(
      "/app/compute",
    );
  });

  it("restores the other view's last route when one is stored", () => {
    window.localStorage.setItem(
      "nimbus-ui:last-route:operator",
      "/admin/machines",
    );
    setPathname("/app/compute");
    render(<ViewSwitcher />);
    fireEvent.click(screen.getByTestId("view-switcher-operator"));
    expect(navigateMock).toHaveBeenCalledWith({ to: "/admin/machines" });
  });

  it("ignores a stored last route that does not match the target view's prefix", () => {
    window.localStorage.setItem(
      "nimbus-ui:last-route:operator",
      "/app/compute",
    );
    setPathname("/app/compute");
    render(<ViewSwitcher />);
    fireEvent.click(screen.getByTestId("view-switcher-operator"));
    expect(navigateMock).toHaveBeenCalledWith({ to: "/admin" });
  });

  it("does not navigate when clicking the already-active segment", () => {
    setPathname("/app/compute");
    render(<ViewSwitcher />);
    fireEvent.click(screen.getByTestId("view-switcher-developer"));
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("moves focus between segments on ArrowLeft/ArrowRight", () => {
    setPathname("/app");
    render(<ViewSwitcher />);
    const dev = screen.getByTestId("view-switcher-developer");
    const op = screen.getByTestId("view-switcher-operator");
    dev.focus();
    fireEvent.keyDown(dev, { key: "ArrowRight" });
    expect(document.activeElement).toBe(op);
    fireEvent.keyDown(op, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(dev);
  });
});
