import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, navigateMock } = vi.hoisted(() => ({
  pathnameRef: { current: "/app/compute" },
  navigateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigateMock,
  useRouterState: ({
    select,
  }: {
    select: (s: { location: { pathname: string } }) => unknown;
  }) => select({ location: { pathname: pathnameRef.current } }),
}));

import { useUiStore } from "../store/ui-store";
import { TenantSelector } from "./tenant-selector";

function mockTenants(tenants: Array<string | Record<string, string>>) {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ tenants }),
    }),
  );
}

beforeEach(() => {
  pathnameRef.current = "/app/compute";
  navigateMock.mockReset();
  window.localStorage.clear();
  useUiStore.setState({ activeTenant: null });
});

describe("TenantSelector", () => {
  it("loads tenants and renders the trigger label from activeTenant", async () => {
    mockTenants(["acme", "beta"]);
    useUiStore.setState({ activeTenant: "acme" });
    render(<TenantSelector mode={{ kind: "developer" }} />);
    expect(screen.getByTestId("tenant-selector-trigger")).toHaveTextContent(
      "acme",
    );
    await waitFor(() => {
      fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
      expect(
        screen.getByTestId("tenant-selector-option-acme"),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByTestId("tenant-selector-option-acme"),
    ).toHaveAttribute("data-active", "true");
  });

  it("falls back to Create tenant when developer mode has zero tenants", async () => {
    mockTenants([]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    await waitFor(() => {
      expect(
        screen.getByTestId("tenant-selector-create"),
      ).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId("tenant-selector-create"));
    expect(navigateMock).toHaveBeenCalledWith(
      expect.objectContaining({ to: "/admin/tenants" }),
    );
  });

  it("setActiveTenant fires when a developer-mode option is clicked", async () => {
    mockTenants(["acme", "beta"]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    await waitFor(() => {
      expect(
        screen.getByTestId("tenant-selector-option-beta"),
      ).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId("tenant-selector-option-beta"));
    expect(useUiStore.getState().activeTenant).toBe("beta");
    expect(window.localStorage.getItem("nimbus-ui:active-tenant")).toBe("beta");
  });

  it("operator-filter mode prepends 'All tenants' and navigates with ?tenant=", async () => {
    mockTenants(["acme", "beta"]);
    render(
      <TenantSelector mode={{ kind: "operator-filter", currentFilter: null }} />,
    );
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    await waitFor(() => {
      expect(
        screen.getByTestId("tenant-selector-option-all"),
      ).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId("tenant-selector-option-beta"));
    expect(navigateMock).toHaveBeenCalledWith(
      expect.objectContaining({
        to: "/admin/observability",
        search: { tenant: "beta" },
      }),
    );
  });

  it("arrow-key navigation cycles option focus and Enter selects", async () => {
    mockTenants(["acme", "beta"]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    const menu = await screen.findByTestId("tenant-selector-menu");
    await waitFor(() => {
      expect(
        screen.getByTestId("tenant-selector-option-acme"),
      ).toBeInTheDocument();
    });
    expect(
      screen.getByTestId("tenant-selector-option-acme"),
    ).toHaveAttribute("data-focused", "true");
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(
      screen.getByTestId("tenant-selector-option-beta"),
    ).toHaveAttribute("data-focused", "true");
    fireEvent.keyDown(menu, { key: "Enter" });
    expect(useUiStore.getState().activeTenant).toBe("beta");
  });

  it("Escape closes the menu without changing selection", async () => {
    mockTenants(["acme"]);
    useUiStore.setState({ activeTenant: "acme" });
    render(<TenantSelector mode={{ kind: "developer" }} />);
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    const menu = await screen.findByTestId("tenant-selector-menu");
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => {
      expect(
        screen.queryByTestId("tenant-selector-menu"),
      ).not.toBeInTheDocument();
    });
    expect(useUiStore.getState().activeTenant).toBe("acme");
  });

  it("shows an error message when /api/tenants fails", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: false,
        json: async () => ({ error: { message: "boom" } }),
      }),
    );
    render(<TenantSelector mode={{ kind: "developer" }} />);
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    await waitFor(() => {
      expect(
        screen.getByTestId("tenant-selector-error"),
      ).toHaveTextContent("boom");
    });
  });

  it("operator-filter reflects currentFilter on the trigger", async () => {
    mockTenants(["acme", "beta"]);
    render(
      <TenantSelector
        mode={{ kind: "operator-filter", currentFilter: "beta" }}
      />,
    );
    await act(async () => {});
    expect(screen.getByTestId("tenant-selector-trigger")).toHaveTextContent(
      "beta",
    );
  });
});
