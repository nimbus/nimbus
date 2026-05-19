import { useNavigate, useRouter, useRouterState } from "@tanstack/react-router";
import { useEffect, useMemo } from "react";

import { useUiStore } from "../store/ui-store";
import { viewFromPathname } from "./nav-entries";
import { fetchTenants } from "./tenants-fetch";

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

  const view = viewFromPathname(pathname);
  const asParam = useMemo(() => {
    const as = search?.as;
    return typeof as === "string" && as.length > 0 ? as : null;
  }, [search]);

  useEffect(() => {
    if (view !== "developer") return;
    if (asParam === null) return;
    setActiveTenant(asParam);
    const { as: _stripped, ...rest } = search ?? {};
    void navigate({
      to: pathname,
      search: rest as Record<string, unknown>,
      replace: true,
    });
  }, [view, asParam, pathname, search, setActiveTenant, navigate]);

  useEffect(() => {
    if (view !== "developer") return;
    if (activeTenant !== null) return;
    if (asParam !== null) return;
    const controller = new AbortController();
    fetchTenants(controller.signal)
      .then((ids) => {
        if (controller.signal.aborted) return;
        if (ids === null || ids.length === 0) return;
        if (useUiStore.getState().activeTenant !== null) return;
        setActiveTenant(ids[0]);
      })
      .catch(() => {
        /* ignore — TenantSelector surfaces tenant-load errors */
      });
    return () => controller.abort();
  }, [view, asParam, activeTenant, setActiveTenant]);
}

export function useTenantSwitchInvalidation() {
  const router = useRouter();
  useEffect(() => {
    const unsubscribe = useUiStore.subscribe((state, prevState) => {
      if (state.activeTenant !== prevState.activeTenant) {
        void router.invalidate();
      }
    });
    return unsubscribe;
  }, [router]);
}
