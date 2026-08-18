import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, navigateMock } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer/compute" },
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
  pathnameRef.current = "/developer/compute";
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
    expect(screen.getByTestId("tenant-selector-option-acme")).toHaveAttribute(
      "data-active",
      "true",
    );
  });

  it("falls back to Create tenant when developer mode has zero tenants", async () => {
    mockTenants([]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    await waitFor(() => {
      expect(screen.getByTestId("tenant-selector-create")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTestId("tenant-selector-create"));
    expect(navigateMock).toHaveBeenCalledWith(
      expect.objectContaining({ to: "/operator/tenants" }),
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
      <TenantSelector
        mode={{
          kind: "operator-filter",
          currentFilter: null,
          unavailable: false,
        }}
      />,
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
        to: "/operator/observability",
        search: { tenant: "beta" },
      }),
    );
  });

  it("moves focus to the menu when it opens", async () => {
    mockTenants(["acme", "beta"]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    const menu = await screen.findByTestId("tenant-selector-menu");
    // The menu is a sibling of the trigger, so without this focus move its
    // keydown handler never receives a key event and the whole menu is
    // keyboard-dead.
    await waitFor(() => {
      expect(document.activeElement).toBe(menu);
    });
  });

  it("opens from the trigger on ArrowDown without closing again", async () => {
    mockTenants(["acme", "beta"]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    const trigger = screen.getByTestId("tenant-selector-trigger");
    trigger.focus();
    // preventDefault matters: the browser synthesizes a click from an
    // unprevented Enter/Space on a button, which would re-run the toggle and
    // close the menu in the same tick.
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(
      await screen.findByTestId("tenant-selector-menu"),
    ).toBeInTheDocument();
  });

  it("tracks the focused option with aria-activedescendant", async () => {
    mockTenants(["acme", "beta"]);
    render(<TenantSelector mode={{ kind: "developer" }} />);
    fireEvent.click(screen.getByTestId("tenant-selector-trigger"));
    const menu = await screen.findByTestId("tenant-selector-menu");
    await waitFor(() => {
      expect(
        screen.getByTestId("tenant-selector-option-acme"),
      ).toBeInTheDocument();
    });
    const first = menu.getAttribute("aria-activedescendant");
    expect(first).toBeTruthy();
    expect(document.getElementById(first as string)).toBe(
      screen.getByTestId("tenant-selector-option-acme"),
    );
    fireEvent.keyDown(menu, { key: "End" });
    const last = menu.getAttribute("aria-activedescendant");
    expect(document.getElementById(last as string)).toBe(
      screen.getByTestId("tenant-selector-option-beta"),
    );
    fireEvent.keyDown(menu, { key: "Home" });
    expect(menu.getAttribute("aria-activedescendant")).toBe(first);
  });

  it("renders an inert trigger when the filter is unavailable", async () => {
    mockTenants(["acme", "beta"]);
    render(
      <TenantSelector
        mode={{
          kind: "operator-filter",
          currentFilter: null,
          unavailable: true,
        }}
      />,
    );
    const trigger = screen.getByTestId("tenant-selector-trigger");
    expect(trigger).toBeDisabled();
    expect(trigger).toHaveAttribute("data-unavailable", "true");
    expect(
      screen.getByTestId("tenant-selector-coming-soon"),
    ).toBeInTheDocument();
    fireEvent.click(trigger);
    await act(async () => {});
    expect(
      screen.queryByTestId("tenant-selector-menu"),
    ).not.toBeInTheDocument();
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
    expect(screen.getByTestId("tenant-selector-option-acme")).toHaveAttribute(
      "data-focused",
      "true",
    );
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(screen.getByTestId("tenant-selector-option-beta")).toHaveAttribute(
      "data-focused",
      "true",
    );
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
      expect(screen.getByTestId("tenant-selector-error")).toHaveTextContent(
        "boom",
      );
    });
  });

  it("operator-filter reflects currentFilter on the trigger", async () => {
    mockTenants(["acme", "beta"]);
    render(
      <TenantSelector
        mode={{
          kind: "operator-filter",
          currentFilter: "beta",
          unavailable: false,
        }}
      />,
    );
    await act(async () => {});
    expect(screen.getByTestId("tenant-selector-trigger")).toHaveTextContent(
      "beta",
    );
  });
});
