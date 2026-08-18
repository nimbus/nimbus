import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, searchRef, statusHashRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer" },
  searchRef: { current: {} as Record<string, unknown> },
  statusHashRef: { current: null as string | null },
}));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
  useRouterState: ({
    select,
  }: {
    select: (s: {
      location: { pathname: string; search: Record<string, unknown> };
    }) => unknown;
  }) =>
    select({
      location: { pathname: pathnameRef.current, search: searchRef.current },
    }),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: () => ({ version: "9.9.9", buildHash: statusHashRef.current }),
}));

import { TopNav } from "./top-nav";

function setLocation(path: string, search: Record<string, unknown> = {}) {
  pathnameRef.current = path;
  searchRef.current = search;
}

beforeEach(() => {
  setLocation("/developer");
  statusHashRef.current = null;
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tenants: [] }),
    }),
  );
});

describe("TopNav", () => {
  it("renders the logo, brand text, view switcher, and tenant slot", () => {
    render(<TopNav />);
    expect(screen.getByTestId("top-nav")).toBeInTheDocument();
    expect(screen.getByLabelText("Nimbus")).toBeInTheDocument();
    expect(screen.getByText("nimbus")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher-developer")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher-operator")).toBeInTheDocument();
    expect(screen.getByTestId("top-nav-tenant-slot")).toBeInTheDocument();
  });

  it("shows the version after the brand with a v prefix", () => {
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-version")).toHaveTextContent("v9.9.9");
  });

  it("appends the short build hash and copies the full one", () => {
    statusHashRef.current = "0123456789abcdef";
    render(<TopNav />);
    const chip = screen.getByTestId("top-nav-version");
    expect(chip).toHaveTextContent("v9.9.9+0123456");
    // Two servers can report the same version and run different code, so the
    // chip hands over the whole hash, not the seven characters on screen.
    expect(chip).toHaveAttribute("title", "0123456789abcdef");
  });

  it("offers appearance controls in both consoles", () => {
    render(<TopNav />);
    expect(screen.getByTestId("appearance-menu-trigger")).toBeInTheDocument();
    setLocation("/operator/machines");
    render(<TopNav />);
    expect(screen.getAllByTestId("appearance-menu-trigger")).toHaveLength(2);
  });

  it("shows the developer wordmark on /app routes", () => {
    setLocation("/developer/compute");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-wordmark")).toHaveTextContent(
      "developer console",
    );
    expect(screen.getByTestId("top-nav")).toHaveAttribute(
      "data-view",
      "developer",
    );
  });

  it("shows the operator wordmark on /admin routes", () => {
    setLocation("/operator/machines");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-wordmark")).toHaveTextContent(
      "operator console",
    );
    expect(screen.getByTestId("top-nav")).toHaveAttribute(
      "data-view",
      "operator",
    );
  });

  it("renders the tenant selector in developer mode on /app routes", () => {
    setLocation("/developer/compute");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-tenant-slot")).toHaveAttribute(
      "data-mode",
      "developer",
    );
  });

  it("hides the tenant selector on /operator/machines", () => {
    setLocation("/operator/machines");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-tenant-slot")).toHaveAttribute(
      "data-mode",
      "hidden",
    );
    expect(screen.queryByTestId("tenant-selector")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("tenant-selector-create"),
    ).not.toBeInTheDocument();
  });

  it("renders the tenant selector in operator-filter mode on /operator/observability", () => {
    setLocation("/operator/observability");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-tenant-slot")).toHaveAttribute(
      "data-mode",
      "operator-filter",
    );
  });

  it("renders the observability filter inert while events carry no tenant", () => {
    // The control stays mounted rather than disappearing: a bookmarked
    // `?tenant=` would otherwise have nothing to clear it.
    setLocation("/operator/observability", { tenant: "acme" });
    render(<TopNav />);
    const trigger = screen.getByTestId("tenant-selector-trigger");
    expect(trigger).toBeDisabled();
    expect(trigger).toHaveAttribute("data-unavailable", "true");
  });
});
