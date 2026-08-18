import { fireEvent, render, screen } from "@testing-library/react";
import { Box } from "lucide-react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, searchRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer/settings" },
  searchRef: { current: {} as Record<string, unknown> },
}));

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    "aria-current": ariaCurrent,
    "data-testid": testId,
    "data-active": dataActive,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "aria-current"?: "page" | undefined;
    "data-testid"?: string;
    "data-active"?: string;
    className?: string;
  }) => (
    <a
      href={to}
      aria-current={ariaCurrent}
      data-testid={testId}
      data-active={dataActive}
      className={className}
    >
      {children}
    </a>
  ),
  useRouterState: ({
    select,
  }: {
    select: (s: {
      location: { pathname: string; search: Record<string, unknown> };
    }) => unknown;
  }) =>
    select({
      location: {
        pathname: pathnameRef.current,
        search: searchRef.current,
      },
    }),
}));

import { useUiStore } from "../store/ui-store";
import {
  SubDrawer,
  SubDrawerProvider,
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "./sub-drawer";

function setPathname(path: string) {
  pathnameRef.current = path;
}

function Contributor({ spec }: { spec: SubDrawerSpec | null }) {
  useContributeSubDrawer(spec);
  return null;
}

beforeEach(() => {
  setPathname("/developer/settings");
  searchRef.current = {};
  window.localStorage.clear();
  useUiStore.setState({ subDrawerOpen: true });
});

afterEach(() => {
  window.localStorage.clear();
  vi.unstubAllGlobals();
});

// The global setup stubs matchMedia to always miss, so every other test in this
// file runs at the desktop tier. This narrows the viewport for the tier tests.
function stubTablet() {
  vi.stubGlobal(
    "matchMedia",
    (query: string) =>
      ({
        matches: query.includes("1023px"),
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
      }) as unknown as MediaQueryList,
  );
}

const DYNAMIC_SPEC: SubDrawerSpec = {
  kind: "dynamic",
  title: "Tenants",
  search: { placeholder: "Filter tenants" },
  children: <div data-testid="dynamic-body" />,
};

function renderSubDrawer(spec: SubDrawerSpec = DYNAMIC_SPEC) {
  return render(
    <SubDrawerProvider>
      <Contributor spec={spec} />
      <SubDrawer />
    </SubDrawerProvider>,
  );
}

describe("SubDrawer", () => {
  it("renders nothing when no contributor mounts", () => {
    render(
      <SubDrawerProvider>
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.queryByTestId("sub-drawer")).toBeNull();
  });

  it("renders a static contributor with items + active-state highlight", () => {
    setPathname("/developer/settings/secrets");
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Settings",
      items: [
        {
          id: "environment",
          label: "Environment",
          to: "/developer/settings/environment",
        },
        { id: "secrets", label: "Secrets", to: "/developer/settings/secrets" },
        { id: "schema", label: "Schema", to: "/developer/settings/schema" },
      ],
    };
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-kind",
      "static",
    );
    expect(screen.getByTestId("sub-drawer-item-environment")).toHaveAttribute(
      "data-active",
      "false",
    );
    const secrets = screen.getByTestId("sub-drawer-item-secrets");
    expect(secrets).toHaveAttribute("data-active", "true");
    expect(secrets).toHaveAttribute("aria-current", "page");
  });

  it("renders dynamic contributor children and an optional search input", () => {
    const spec: SubDrawerSpec = {
      kind: "dynamic",
      title: "Tenants",
      search: { placeholder: "Filter tenants" },
      children: (
        <div data-testid="dynamic-body">
          <a href="/operator/tenants/alpha">alpha</a>
        </div>
      ),
    };
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-kind",
      "dynamic",
    );
    const search = screen.getByTestId("sub-drawer-search");
    expect(search).toHaveAttribute("placeholder", "Filter tenants");
    expect(screen.getByTestId("dynamic-body")).toBeInTheDocument();
  });

  it("collapse toggle rails the drawer (still reachable) and persists subDrawerOpen=false", () => {
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Network",
      items: [
        { id: "routes", label: "Routes", to: "/operator/network/routes" },
      ],
    };
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    fireEvent.click(screen.getByTestId("sub-drawer-toggle"));
    // Collapsed to a rail — still present and reachable, not removed.
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(window.localStorage.getItem("nimbus-ui:sub-drawer-open")).toBe(
      "false",
    );
    // And it re-expands from the rail.
    fireEvent.click(screen.getByTestId("sub-drawer-toggle"));
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
  });

  it("hydrates from persisted subDrawerOpen=false (drawer renders collapsed)", async () => {
    window.localStorage.setItem("nimbus-ui:sub-drawer-open", "false");
    vi.resetModules();
    const mod = await import("./sub-drawer");
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Settings",
      items: [
        { id: "general", label: "General", to: "/operator/settings/general" },
      ],
    };
    function FreshContributor() {
      mod.useContributeSubDrawer(spec);
      return null;
    }
    render(
      <mod.SubDrawerProvider>
        <FreshContributor />
        <mod.SubDrawer />
      </mod.SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(screen.getByTestId("sub-drawer-toggle")).toBeInTheDocument();
  });

  it("shows all rail items in the collapsed rail and switches on click without expanding", () => {
    const onFunctions = vi.fn();
    const onSandboxes = vi.fn();
    const spec: SubDrawerSpec = {
      kind: "dynamic",
      title: "Compute",
      children: <div data-testid="dynamic-body" />,
      railItems: [
        {
          id: "functions",
          label: "Functions",
          icon: Box,
          active: true,
          onSelect: onFunctions,
        },
        {
          id: "sandboxes",
          label: "Sandboxes",
          icon: Box,
          active: false,
          onSelect: onSandboxes,
        },
      ],
    };
    useUiStore.setState({ subDrawerOpen: false });
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(
      screen.getByTestId("sub-drawer-rail-item-functions"),
    ).toHaveAttribute("data-active", "true");
    expect(
      screen.getByTestId("sub-drawer-rail-item-sandboxes"),
    ).toHaveAttribute("data-active", "false");
    fireEvent.click(screen.getByTestId("sub-drawer-rail-item-sandboxes"));
    expect(onSandboxes).toHaveBeenCalledTimes(1);
    // switching sub-view from the rail does not expand the drawer
    expect(useUiStore.getState().subDrawerOpen).toBe(false);
  });

  it("toggles collapse on background double-click", () => {
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Network",
      items: [{ id: "routes", label: "Routes", to: "/operator/network" }],
    };
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    fireEvent.doubleClick(screen.getByTestId("sub-drawer"));
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    fireEvent.doubleClick(screen.getByTestId("sub-drawer"));
    expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
  });

  it("uses search-param match for active state when items declare search", () => {
    setPathname("/developer/settings");
    searchRef.current = { section: "secrets" };
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Settings",
      items: [
        {
          id: "environment",
          label: "Environment",
          to: "/developer/settings",
          search: { section: "environment" },
        },
        {
          id: "secrets",
          label: "Secrets",
          to: "/developer/settings",
          search: { section: "secrets" },
        },
      ],
    };
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer-item-environment")).toHaveAttribute(
      "data-active",
      "false",
    );
    expect(screen.getByTestId("sub-drawer-item-secrets")).toHaveAttribute(
      "data-active",
      "true",
    );
  });

  it("clears the spec when the contributor unmounts", () => {
    function Host() {
      const [mounted, setMounted] = useState(true);
      const spec: SubDrawerSpec = {
        kind: "static",
        title: "Schedules",
        items: [
          {
            id: "scheduled",
            label: "Scheduled",
            to: "/developer/schedules/scheduled",
          },
        ],
      };
      return (
        <SubDrawerProvider>
          {mounted ? <Contributor spec={spec} /> : null}
          <SubDrawer />
          <button
            type="button"
            data-testid="toggle"
            onClick={() => setMounted(false)}
          >
            unmount
          </button>
        </SubDrawerProvider>
      );
    }
    render(<Host />);
    expect(screen.getByTestId("sub-drawer")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("toggle"));
    expect(screen.queryByTestId("sub-drawer")).toBeNull();
  });
  describe("below the desktop tier", () => {
    it("shows the rail without a sheet even when the stored state is open", () => {
      stubTablet();
      useUiStore.setState({ subDrawerOpen: true });
      renderSubDrawer();
      // The sheet is modal. Honoring a stored desktop preference here would
      // drop a scrim over the content the moment the viewport narrows.
      expect(screen.getByTestId("sub-drawer")).toHaveAttribute(
        "data-collapsed",
        "true",
      );
      expect(screen.queryByTestId("sub-drawer-overlay")).toBeNull();
      expect(screen.queryByTestId("sub-drawer-scrim")).toBeNull();
    });

    it("opens an overlay sheet on an explicit expand", () => {
      stubTablet();
      useUiStore.setState({ subDrawerOpen: true });
      renderSubDrawer();
      fireEvent.click(screen.getByTestId("sub-drawer-toggle"));
      expect(screen.getByTestId("sub-drawer-overlay")).toBeInTheDocument();
      expect(screen.getByTestId("sub-drawer-scrim")).toBeInTheDocument();
      // The rail stays in flow beneath the sheet, so dismissing it still
      // leaves a visible way back.
      expect(screen.getByTestId("sub-drawer")).toBeInTheDocument();
    });

    it("never writes the desktop preference when expanding at tablet width", () => {
      stubTablet();
      useUiStore.setState({ subDrawerOpen: true });
      renderSubDrawer();
      fireEvent.click(screen.getByTestId("sub-drawer-toggle"));
      expect(useUiStore.getState().subDrawerOpen).toBe(true);
      expect(
        window.localStorage.getItem("nimbus-ui:sub-drawer-open"),
      ).toBeNull();
    });
  });
});
