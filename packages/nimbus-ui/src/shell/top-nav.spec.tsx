import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/app" },
}));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => vi.fn(),
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

import { TopNav } from "./top-nav";

function setPathname(path: string) {
  pathnameRef.current = path;
}

beforeEach(() => {
  setPathname("/app");
});

afterEach(() => {
  // pathname reset in beforeEach
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
    setPathname("/app/compute");
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
    setPathname("/admin/machines");
    render(<TopNav />);
    expect(screen.getByTestId("top-nav-wordmark")).toHaveTextContent(
      "operator console",
    );
    expect(screen.getByTestId("top-nav")).toHaveAttribute(
      "data-view",
      "operator",
    );
  });
});
