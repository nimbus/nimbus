import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { navigateMock, pathnameRef } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  pathnameRef: { current: "/developer" },
}));

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    onClick,
    ...rest
  }: {
    to: string;
    children: React.ReactNode;
    onClick?: () => void;
  } & Record<string, unknown>) => (
    <a href={to} onClick={onClick} {...rest}>
      {children}
    </a>
  ),
  useNavigate: () => navigateMock,
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useNimbus: () => ({ url: "http://nimbus.example:9000" }),
  useNimbusConnectionState: () => ({
    isWebSocketConnected: true,
    hasEverConnected: true,
    hasInflightRequests: false,
    inflightMutations: 0,
    inflightActions: 0,
  }),
  useQuery: () => ({ version: "0.1.0" }),
}));

vi.mock("../../hooks/use-staleness", () => ({
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
vi.mock("../../hooks/use-tenant-list", () => ({
  useTenantList: () => ({
    kind: "loaded",
    tenants: [{ id: "acme" }],
    reload: vi.fn(),
  }),
}));

import { useUiStore } from "../../store/ui-store";
import { MobileTopBar } from "./mobile-sheet";

beforeEach(() => {
  pathnameRef.current = "/developer";
  useUiStore.setState({ activeTenant: "acme" });
});

describe("MobileTopBar", () => {
  it("opens the sidebar body in a sheet and closes it on navigation", async () => {
    render(<MobileTopBar />);
    expect(screen.getByTestId("mobile-top-bar")).toBeInTheDocument();
    expect(screen.queryByTestId("sidebar-sheet")).toBeNull();

    fireEvent.click(screen.getByTestId("mobile-menu-button"));
    const sheet = await screen.findByTestId("sidebar-sheet");
    expect(sheet).toHaveAttribute("data-side", "left");
    expect(screen.getByTestId("nav-compute")).toBeInTheDocument();
    expect(screen.getByTestId("view-switcher")).toBeInTheDocument();
    // The sheet has no rail to collapse to, so it has no collapse control.
    expect(screen.queryByTestId("sidebar-toggle")).toBeNull();

    fireEvent.click(screen.getByTestId("nav-compute"));
    await waitFor(() => {
      expect(screen.queryByTestId("sidebar-sheet")).toBeNull();
    });
  });

  it("closes the sheet when the pathname changes under it", async () => {
    const { rerender } = render(<MobileTopBar />);
    fireEvent.click(screen.getByTestId("mobile-menu-button"));
    await screen.findByTestId("sidebar-sheet");

    // The view switcher navigates without being a link; the sheet still
    // has to get out of the way of the page it lands on.
    fireEvent.click(screen.getByTestId("view-switcher-operator"));
    expect(navigateMock).toHaveBeenCalledWith({ to: "/operator" });
    pathnameRef.current = "/operator";
    rerender(<MobileTopBar />);
    await waitFor(() => {
      expect(screen.queryByTestId("sidebar-sheet")).toBeNull();
    });
  });
});
