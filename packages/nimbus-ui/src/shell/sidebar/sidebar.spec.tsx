import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
  useQuery: () => ({ version: "0.1.0", buildHash: "abcdef0" }),
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
import { Sidebar } from "./sidebar";

const DEVELOPER_IDS = [
  "overview",
  "compute",
  "deploys",
  "storage",
  "files",
  "services",
  "sandboxes",
  "schedules",
  "observability",
  "settings",
];
const OPERATOR_IDS = [
  "nodes",
  "machines",
  "network",
  "services",
  "tenants",
  "observability",
  "settings",
];

function setPathname(path: string) {
  pathnameRef.current = path;
}

// The global test setup stubs matchMedia to always report `matches: false`,
// which is the desktop tier. Narrow it per query to drive the other tiers.
function stubTier(tier: "mobile" | "tablet") {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: tier === "mobile" ? true : query.includes("1023px"),
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }));
}

function renderedNavIds(): string[] {
  return Array.from(
    screen.getByTestId("sidebar-nav").querySelectorAll("a[data-testid^=nav-]"),
  ).map((el) => el.getAttribute("data-testid")?.replace("nav-", "") ?? "");
}

beforeEach(() => {
  setPathname("/developer");
  navigateMock.mockClear();
  useUiStore.setState({
    sidebarCollapsed: false,
    themeMode: "system",
    theme: "light",
    activeTenant: "acme",
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("Sidebar", () => {
  it("renders the developer rows in group order under uppercase group labels", () => {
    setPathname("/developer/compute");
    render(<Sidebar />);
    expect(screen.getByTestId("sidebar")).toHaveAttribute(
      "data-view",
      "developer",
    );
    expect(renderedNavIds()).toEqual(DEVELOPER_IDS);
    for (const label of ["build", "run", "observe"]) {
      const heading = screen.getByTestId(`sidebar-group-${label}`);
      expect(heading.className).toContain("uppercase");
    }
    // The home row and Settings sit outside any group.
    expect(screen.queryByTestId("sidebar-group-null")).toBeNull();
  });

  it("switches the nav groups when the scope row switches the view", () => {
    const { rerender } = render(<Sidebar />);
    expect(screen.getByTestId("tenant-selector")).toBeInTheDocument();
    expect(screen.queryByTestId("sidebar-server")).toBeNull();

    fireEvent.click(screen.getByTestId("view-switcher-operator"));
    expect(navigateMock).toHaveBeenCalledWith({ to: "/operator" });

    // The router mock does not move the location; the test moves it the
    // way a navigation would and re-renders.
    setPathname("/operator");
    rerender(<Sidebar />);
    expect(screen.getByTestId("sidebar")).toHaveAttribute(
      "data-view",
      "operator",
    );
    expect(renderedNavIds()).toEqual(OPERATOR_IDS);
    for (const label of ["fleet", "access", "observe"]) {
      expect(screen.getByTestId(`sidebar-group-${label}`)).toBeInTheDocument();
    }
    expect(screen.queryByTestId("tenant-selector")).toBeNull();
    expect(screen.getByTestId("sidebar-server")).toHaveTextContent(
      "nimbus.example:9000",
    );
  });

  it("carries no count badge on any row", () => {
    render(<Sidebar />);
    expect(document.querySelector("[data-testid$=-count]")).toBeNull();
    expect(screen.getByTestId("nav-services")).toHaveTextContent(/^Services$/);
  });

  it("marks only the current row, exactly for the home page and by prefix elsewhere", () => {
    setPathname("/developer");
    const { rerender } = render(<Sidebar />);
    expect(screen.getByTestId("nav-overview")).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByTestId("nav-compute")).not.toHaveAttribute(
      "aria-current",
    );

    setPathname("/developer/compute/some-function");
    rerender(<Sidebar />);
    expect(screen.getByTestId("nav-compute")).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByTestId("nav-overview")).not.toHaveAttribute(
      "aria-current",
    );
  });

  it("collapses to the rail and persists the choice on desktop", () => {
    render(<Sidebar />);
    const aside = screen.getByTestId("sidebar");
    const toggle = screen.getByTestId("sidebar-toggle");
    expect(aside).toHaveAttribute("data-collapsed", "false");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(toggle).toHaveAttribute("aria-controls", aside.id);
    expect(toggle).toHaveAccessibleName("Collapse sidebar");

    fireEvent.click(toggle);
    expect(aside).toHaveAttribute("data-collapsed", "true");
    expect(aside.className).toContain("w-16");
    expect(screen.getByTestId("sidebar-toggle")).toHaveAccessibleName(
      "Expand sidebar",
    );
    expect(window.localStorage.getItem("nimbus-ui:sidebar-collapsed")).toBe(
      "true",
    );
    // The rail keeps every row reachable by name, and drops the labels.
    expect(screen.getByTestId("nav-compute")).toHaveAccessibleName("Compute");
    expect(screen.queryByTestId("sidebar-group-build")).toBeNull();
    expect(screen.getByTestId("view-switcher-developer")).toHaveAttribute(
      "aria-checked",
      "true",
    );

    fireEvent.click(screen.getByTestId("sidebar-toggle"));
    expect(aside).toHaveAttribute("data-collapsed", "false");
    expect(window.localStorage.getItem("nimbus-ui:sidebar-collapsed")).toBe(
      "false",
    );
  });

  it("expands the rail from the scope button without a second click", () => {
    useUiStore.setState({ sidebarCollapsed: true });
    render(<Sidebar />);
    const button = screen.getByTestId("sidebar-scope-button");
    expect(button).toHaveAccessibleName(/Tenant · acme/);
    fireEvent.click(button);
    expect(screen.getByTestId("sidebar")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    expect(screen.getByTestId("tenant-selector")).toBeInTheDocument();
  });

  it("defaults to the rail below the desktop tier and keeps the stored preference", () => {
    stubTier("tablet");
    render(<Sidebar />);
    const aside = screen.getByTestId("sidebar");
    expect(aside).toHaveAttribute("data-tier", "tablet");
    expect(aside).toHaveAttribute("data-collapsed", "true");

    fireEvent.click(screen.getByTestId("sidebar-toggle"));
    expect(aside).toHaveAttribute("data-collapsed", "false");
    expect(window.localStorage.getItem("nimbus-ui:sidebar-collapsed")).toBe(
      null,
    );
    expect(useUiStore.getState().sidebarCollapsed).toBe(false);
  });

  it("names the current theme on the toggle and flips it", () => {
    render(<Sidebar />);
    const toggle = screen.getByTestId("sidebar-theme-toggle");
    expect(toggle).toHaveTextContent("Light theme");
    expect(toggle).toHaveAccessibleName("Light theme. Switch to dark theme");

    act(() => {
      fireEvent.click(toggle);
    });
    expect(screen.getByTestId("sidebar-theme-toggle")).toHaveTextContent(
      "Dark theme",
    );
    expect(useUiStore.getState().themeMode).toBe("dark");
    expect(window.localStorage.getItem("nimbus-ui:theme")).toBe("dark");
  });

  it("shows the connection state and the server version in the footer", () => {
    render(<Sidebar />);
    const status = screen.getByTestId("sidebar-status");
    expect(status).toHaveAttribute("data-state", "connected");
    expect(status).toHaveTextContent("Connected · v0.1.0");
  });
});
