import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, searchRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer" },
  searchRef: { current: {} as Record<string, unknown> },
}));

vi.mock("@tanstack/react-router", () => ({
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
  useNimbus: () => ({ url: "http://localhost:9000" }),
  useNimbusConnectionState: () => ({
    isWebSocketConnected: true,
    hasEverConnected: true,
    hasInflightRequests: false,
    inflightMutations: 0,
    inflightActions: 0,
  }),
  useQuery: () => ({ version: "0.1.0", buildHash: "abcdef0" }),
}));

vi.mock("../hooks/use-staleness", () => ({
  useStalenessContext: () => ({
    snapshot: { state: "hidden", info: null, targetLatest: null },
    isLocal: false,
    hasDesktopBridge: false,
    openPopover: vi.fn(),
    closePopover: vi.fn(),
    startUpgrade: vi.fn(),
    copyCommand: vi.fn(),
  }),
}));

import { StatusBar } from "./status-bar";
import { useUiStore } from "../store/ui-store";

function setLocation(path: string, search: Record<string, unknown> = {}) {
  pathnameRef.current = path;
  searchRef.current = search;
}

beforeEach(() => {
  setLocation("/developer");
  useUiStore.setState({ activeTenant: null });
});

describe("StatusBar tenant slot", () => {
  it("shows the active dev tenant on /developer/*", () => {
    setLocation("/developer/compute");
    useUiStore.setState({ activeTenant: "beta" });
    render(<StatusBar />);
    expect(screen.getByTestId("status-tenant")).toHaveTextContent("beta");
  });

  it("shows 'all tenants' on /operator/observability without a tenant query", () => {
    setLocation("/operator/observability");
    render(<StatusBar />);
    expect(screen.getByTestId("status-tenant")).toHaveTextContent(
      "all tenants",
    );
  });

  it("shows the requested tenant on /operator/observability?tenant=beta", () => {
    setLocation("/operator/observability", { tenant: "beta" });
    render(<StatusBar />);
    expect(screen.getByTestId("status-tenant")).toHaveTextContent("beta");
  });

  it("shows _nimbus on system-tenant /operator/* views", () => {
    setLocation("/operator/machines");
    render(<StatusBar />);
    expect(screen.getByTestId("status-tenant")).toHaveTextContent("_nimbus");
  });
});
