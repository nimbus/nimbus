import { useNavigate } from "@tanstack/react-router";
import { useCallback, useMemo } from "react";

import type { ObservabilityTabProps } from "./-facets";
import { type ObservabilitySearch, parseObservabilitySearch } from "./-types";

type ObservabilityRoute =
  | "/developer/observability"
  | "/operator/observability";

// The router hands the reducer the union of every route's search, so the
// previous value is re-parsed through the shared parser before the patch
// lands on it; the result is one ObservabilitySearch either route accepts.
function merge(
  prev: Record<string, unknown>,
  patch: Partial<ObservabilitySearch>,
): ObservabilitySearch {
  return { ...parseObservabilitySearch(prev), ...patch };
}

// The two navigation flavours a tab receives. A facet change replaces the
// history entry, so Back leaves the page rather than unwinding every
// keystroke; opening a run or switching a tab pushes one, so Back closes
// it. Both routes hand the same pair to the same tabs.
export function useObservabilityNavigation(
  to: ObservabilityRoute,
): Pick<ObservabilityTabProps, "setSearch" | "setSearchAction"> {
  const navigate = useNavigate();
  const setSearch = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to,
        search: (prev) => merge(prev, patch),
        replace: true,
      });
    },
    [navigate, to],
  );
  const setSearchAction = useCallback(
    (patch: Partial<ObservabilitySearch>) => {
      void navigate({
        to,
        search: (prev) => merge(prev, patch),
      });
    },
    [navigate, to],
  );
  return useMemo(
    () => ({ setSearch, setSearchAction }),
    [setSearch, setSearchAction],
  );
}
