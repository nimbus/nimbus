import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useEffect } from "react";

import { useUiStore } from "../store/ui-store";

export function useTenantBootstrap() {
  const { pathname, search } = useRouterState({
    select: (s) => ({
      pathname: s.location.pathname,
      search: s.location.search as Record<string, unknown> | undefined,
    }),
  });
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
}
