import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useEffect } from "react";

import { useUiStore } from "../store/ui-store";

type TenantListResponse = {
  tenants?: Array<
    string | { id?: string; tenantId?: string; name?: string }
  >;
};

async function fetchFirstTenantId(
  signal: AbortSignal,
): Promise<string | null> {
  const response = await fetch("/api/tenants", {
    credentials: "include",
    signal,
  });
  if (!response.ok) return null;
  const body = (await response.json()) as TenantListResponse;
  const ids = (body.tenants ?? [])
    .map((entry) =>
      typeof entry === "string"
        ? entry
        : (entry.tenantId ?? entry.id ?? entry.name ?? null),
    )
    .filter((id): id is string => typeof id === "string" && id.length > 0)
    .sort((a, b) => a.localeCompare(b));
  return ids[0] ?? null;
}

export function useTenantBootstrap() {
  const { pathname, search } = useRouterState({
    select: (s) => ({
      pathname: s.location.pathname,
      search: s.location.search as Record<string, unknown> | undefined,
    }),
  });
  const activeTenant = useUiStore((s) => s.activeTenant);
  const setActiveTenant = useUiStore((s) => s.setActiveTenant);
  const navigate = useNavigate();

  useEffect(() => {
    if (!pathname.startsWith("/app")) return;
    const as = search?.as;
    if (typeof as !== "string" || as.length === 0) return;
    setActiveTenant(as);
    const { as: _stripped, ...rest } = search ?? {};
    void navigate({
      to: pathname,
      search: rest as Record<string, unknown>,
      replace: true,
    });
  }, [pathname, search, setActiveTenant, navigate]);

  useEffect(() => {
    if (!pathname.startsWith("/app")) return;
    if (activeTenant !== null) return;
    const as = search?.as;
    if (typeof as === "string" && as.length > 0) return;
    const controller = new AbortController();
    fetchFirstTenantId(controller.signal)
      .then((firstId) => {
        if (controller.signal.aborted) return;
        if (firstId === null) return;
        if (useUiStore.getState().activeTenant !== null) return;
        setActiveTenant(firstId);
      })
      .catch(() => {
        /* ignore — TenantSelector surfaces tenant-load errors */
      });
    return () => controller.abort();
  }, [pathname, search, activeTenant, setActiveTenant]);
}
