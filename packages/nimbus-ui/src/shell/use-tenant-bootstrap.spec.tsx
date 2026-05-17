import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

beforeEach(() => {
  pathnameRef.current = "/app/compute";
  searchRef.current = {};
  navigateMock.mockReset();
  window.localStorage.clear();
  useUiStore.setState({ activeTenant: null });
});

describe("useTenantBootstrap", () => {
  it("does nothing on non-/app routes", () => {
    pathnameRef.current = "/admin/machines";
    searchRef.current = { as: "acme" };
    renderHook(() => useTenantBootstrap());
    expect(useUiStore.getState().activeTenant).toBeNull();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("does nothing on /app when no ?as= is present", () => {
    pathnameRef.current = "/app/compute";
    searchRef.current = {};
    renderHook(() => useTenantBootstrap());
    expect(useUiStore.getState().activeTenant).toBeNull();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("writes ?as= into the store and strips it from the URL", () => {
    pathnameRef.current = "/app/compute";
    searchRef.current = { as: "acme", other: "1" };
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
    renderHook(() => useTenantBootstrap());
    expect(useUiStore.getState().activeTenant).toBeNull();
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
