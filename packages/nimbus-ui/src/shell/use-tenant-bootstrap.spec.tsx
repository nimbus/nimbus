import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, searchRef, navigateMock } = vi.hoisted(() => ({
  pathnameRef: { current: "/app/compute" },
  searchRef: { current: {} as Record<string, unknown> },
  navigateMock: vi.fn(),
}));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigateMock,
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

import { useUiStore } from "../store/ui-store";
import { useTenantBootstrap } from "./use-tenant-bootstrap";

function mockTenantsResponse(tenants: unknown) {
  const fetchMock = vi.fn(async () =>
    new Response(JSON.stringify({ tenants }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
  );
  globalThis.fetch = fetchMock as unknown as typeof fetch;
  return fetchMock;
}

function mockTenantsError(status = 500) {
  const fetchMock = vi.fn(async () =>
    new Response(JSON.stringify({}), { status }),
  );
  globalThis.fetch = fetchMock as unknown as typeof fetch;
  return fetchMock;
}

beforeEach(() => {
  pathnameRef.current = "/app/compute";
  searchRef.current = {};
  navigateMock.mockReset();
  window.localStorage.clear();
  useUiStore.setState({ activeTenant: null });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useTenantBootstrap — ?as= override", () => {
  it("does nothing on non-/app routes", () => {
    pathnameRef.current = "/admin/machines";
    searchRef.current = { as: "acme" };
    mockTenantsResponse([]);
    renderHook(() => useTenantBootstrap());
    expect(useUiStore.getState().activeTenant).toBeNull();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("writes ?as= into the store and strips it from the URL", () => {
    pathnameRef.current = "/app/compute";
    searchRef.current = { as: "acme", other: "1" };
    mockTenantsResponse([]);
    renderHook(() => useTenantBootstrap());
    expect(useUiStore.getState().activeTenant).toBe("acme");
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/app/compute",
      search: { other: "1" },
      replace: true,
    });
  });

  it("ignores empty-string ?as=", () => {
    pathnameRef.current = "/app/compute";
    searchRef.current = { as: "" };
    mockTenantsResponse([]);
    renderHook(() => useTenantBootstrap());
    expect(navigateMock).not.toHaveBeenCalled();
  });
});

describe("useTenantBootstrap — auto-default (DR4 / F5)", () => {
  it("defaults activeTenant to the first tenant id on /app when null", async () => {
    mockTenantsResponse(["beta", "alpha"]);
    renderHook(() => useTenantBootstrap());
    await waitFor(() =>
      expect(useUiStore.getState().activeTenant).toBe("alpha"),
    );
  });

  it("handles tenant objects with tenantId/id/name and sorts deterministically", async () => {
    mockTenantsResponse([
      { tenantId: "delta" },
      { id: "alpha" },
      { name: "charlie" },
    ]);
    renderHook(() => useTenantBootstrap());
    await waitFor(() =>
      expect(useUiStore.getState().activeTenant).toBe("alpha"),
    );
  });

  it("does not auto-default on operator routes", async () => {
    pathnameRef.current = "/admin/machines";
    const fetchMock = mockTenantsResponse(["alpha"]);
    renderHook(() => useTenantBootstrap());
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(useUiStore.getState().activeTenant).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("does not override an existing activeTenant", async () => {
    useUiStore.setState({ activeTenant: "acme" });
    const fetchMock = mockTenantsResponse(["alpha"]);
    renderHook(() => useTenantBootstrap());
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(useUiStore.getState().activeTenant).toBe("acme");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("leaves activeTenant null when the tenant list is empty", async () => {
    mockTenantsResponse([]);
    renderHook(() => useTenantBootstrap());
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(useUiStore.getState().activeTenant).toBeNull();
  });

  it("swallows fetch errors instead of throwing", async () => {
    mockTenantsError(500);
    renderHook(() => useTenantBootstrap());
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(useUiStore.getState().activeTenant).toBeNull();
  });

  it("?as= takes priority over auto-default and the fetch is skipped", async () => {
    searchRef.current = { as: "explicit" };
    const fetchMock = mockTenantsResponse(["alpha", "beta"]);
    renderHook(() => useTenantBootstrap());
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(useUiStore.getState().activeTenant).toBe("explicit");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
