import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/app/settings" },
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
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
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
  setPathname("/app/settings");
  window.localStorage.clear();
  useUiStore.setState({ subDrawerOpen: true });
});

afterEach(() => {
  window.localStorage.clear();
});

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
    setPathname("/app/settings/secrets");
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Settings",
      items: [
        {
          id: "environment",
          label: "Environment",
          to: "/app/settings/environment",
        },
        { id: "secrets", label: "Secrets", to: "/app/settings/secrets" },
        { id: "schema", label: "Schema", to: "/app/settings/schema" },
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
          <a href="/admin/tenants/alpha">alpha</a>
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

  it("close button hides the drawer and persists subDrawerOpen=false", () => {
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Network",
      items: [{ id: "routes", label: "Routes", to: "/admin/network/routes" }],
    };
    render(
      <SubDrawerProvider>
        <Contributor spec={spec} />
        <SubDrawer />
      </SubDrawerProvider>,
    );
    expect(screen.getByTestId("sub-drawer")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("sub-drawer-close"));
    expect(screen.queryByTestId("sub-drawer")).toBeNull();
    expect(window.localStorage.getItem("nimbus-ui:sub-drawer-open")).toBe(
      "false",
    );
  });

  it("hydrates from persisted subDrawerOpen=false (drawer stays hidden)", async () => {
    window.localStorage.setItem("nimbus-ui:sub-drawer-open", "false");
    vi.resetModules();
    const mod = await import("./sub-drawer");
    const spec: SubDrawerSpec = {
      kind: "static",
      title: "Settings",
      items: [
        { id: "general", label: "General", to: "/admin/settings/general" },
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
    expect(screen.queryByTestId("sub-drawer")).toBeNull();
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
            to: "/app/schedules/scheduled",
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
});
