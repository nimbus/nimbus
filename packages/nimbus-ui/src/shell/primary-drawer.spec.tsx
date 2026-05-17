import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/app" },
}));

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    title,
    "aria-label": ariaLabel,
    "aria-current": ariaCurrent,
    "data-testid": testId,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    title?: string;
    "aria-label"?: string;
    "aria-current"?: "page" | undefined;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a
      href={to}
      title={title}
      aria-label={ariaLabel}
      aria-current={ariaCurrent}
      data-testid={testId}
      className={className}
    >
      {children}
    </a>
  ),
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

vi.mock("nimbus/react", () => ({
  useQuery: () => undefined,
}));

import { PrimaryDrawer } from "./primary-drawer";

function setPathname(path: string) {
  pathnameRef.current = path;
}

beforeEach(() => {
  setPathname("/app");
});

afterEach(() => {
  // localStorage cleared by global setup
});

describe("PrimaryDrawer", () => {
  it("renders all 7 developer entries on /app routes", () => {
    setPathname("/app/compute");
    render(<PrimaryDrawer />);
    expect(screen.getByTestId("primary-drawer")).toHaveAttribute(
      "data-view",
      "developer",
    );
    for (const id of [
      "overview",
      "compute",
      "schedules",
      "storage",
      "files",
      "observability",
      "settings",
    ]) {
      expect(screen.getByTestId(`nav-${id}`)).toBeInTheDocument();
    }
  });

  it("renders all 7 operator entries on /admin routes", () => {
    setPathname("/admin/machines");
    render(<PrimaryDrawer />);
    expect(screen.getByTestId("primary-drawer")).toHaveAttribute(
      "data-view",
      "operator",
    );
    for (const id of [
      "system",
      "tenants",
      "machines",
      "network",
      "services",
      "observability",
      "settings",
    ]) {
      expect(screen.getByTestId(`nav-${id}`)).toBeInTheDocument();
    }
  });

  it("starts expanded by default with aria-expanded=true and Collapse label", () => {
    render(<PrimaryDrawer />);
    const toggle = screen.getByTestId("primary-drawer-toggle");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(toggle).toHaveAttribute("aria-label", "Collapse navigation");
    expect(screen.getByTestId("primary-drawer")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
  });

  it("toggles collapsed state and persists to localStorage", () => {
    render(<PrimaryDrawer />);
    const toggle = screen.getByTestId("primary-drawer-toggle");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAttribute("aria-label", "Expand navigation");
    expect(screen.getByTestId("primary-drawer")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(
      window.localStorage.getItem("nimbus-ui:primary-drawer-collapsed"),
    ).toBe("true");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(
      window.localStorage.getItem("nimbus-ui:primary-drawer-collapsed"),
    ).toBe("false");
  });

  it("hides labels in collapsed mode but keeps entries reachable via aria-label", () => {
    window.localStorage.setItem("nimbus-ui:primary-drawer-collapsed", "true");
    // Reset module to pick up persisted initial state
    vi.resetModules();
    return import("./primary-drawer").then(({ PrimaryDrawer: Fresh }) => {
      render(<Fresh />);
      expect(screen.getByTestId("primary-drawer")).toHaveAttribute(
        "data-collapsed",
        "true",
      );
      const computeLink = screen.getByTestId("nav-compute");
      expect(computeLink).toHaveAttribute("title", "Compute");
      expect(computeLink).toHaveAttribute("aria-label", "Compute");
      expect(computeLink).not.toHaveTextContent("Compute");
    });
  });

  it("keeps focus on the toggle after clicking", () => {
    render(<PrimaryDrawer />);
    const toggle = screen.getByTestId("primary-drawer-toggle");
    toggle.focus();
    fireEvent.click(toggle);
    expect(document.activeElement).toBe(toggle);
  });

  it("wires aria-controls to the nav id", () => {
    render(<PrimaryDrawer />);
    const toggle = screen.getByTestId("primary-drawer-toggle");
    const nav = screen.getByTestId("primary-drawer");
    expect(toggle).toHaveAttribute("aria-controls", nav.id);
  });

  it("flips entries in place when the pathname changes view", () => {
    setPathname("/app");
    const { rerender } = render(<PrimaryDrawer />);
    expect(screen.getByTestId("nav-compute")).toBeInTheDocument();
    expect(screen.queryByTestId("nav-machines")).toBeNull();
    setPathname("/admin");
    rerender(<PrimaryDrawer />);
    expect(screen.getByTestId("nav-machines")).toBeInTheDocument();
    expect(screen.queryByTestId("nav-compute")).toBeNull();
  });
});
