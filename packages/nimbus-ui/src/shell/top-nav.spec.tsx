import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, searchRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/app" },
  searchRef: { current: {} as Record<string, unknown> },
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

import { TopNav } from "./top-nav";

function setLocation(path: string, search: Record<string, unknown> = {}) {
  pathnameRef.current = path;
  searchRef.current = search;
}

beforeEach(() => {
  setLocation("/app");
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
    expect(screen.getByText("Nimbus")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher-developer")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher-operator")).toBeInTheDocument();
    expect(screen.getByTestId("top-nav-tenant-slot")).toBeInTheDocument();
  });

  it("shows the developer wordmark on /app routes", () => {
    setLocation("/app/compute");
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
    setLocation("/admin/machines");
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
    setLocation("/app/compute");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-tenant-slot")).toHaveAttribute(
      "data-mode",
      "developer",
    );
  });

  it("hides the tenant selector on /admin/machines", () => {
    setLocation("/admin/machines");
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

  it("renders the tenant selector in operator-filter mode on /admin/observability", () => {
    setLocation("/admin/observability");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-tenant-slot")).toHaveAttribute(
      "data-mode",
      "operator-filter",
    );
  });
});
