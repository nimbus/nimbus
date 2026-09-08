import { useMemo } from "react";

import { FacetButton } from "../../../components/facet-bar";
import { Select } from "../../../components/select";
import { useTenantList } from "../../../hooks/use-tenant-list";
import { useUiStore } from "../../../store/ui-store";
import type { ObservabilitySearch } from "./-types";

// The props every observability tab receives from its route. The route owns
// the tenant resolution (active tenant on the developer page, every tenant
// on the operator page) and the two navigation flavours; the tabs own the
// facets and the reads.
export type ObservabilityTabProps = {
  search: ObservabilitySearch;
  // The tenant the reads are scoped to, or null for every tenant.
  tenantId: string | null;
  // The operator surface offers "all tenants"; the developer surface never
  // reads across tenants.
  allowAllTenants: boolean;
  // Replace navigation: a facet change is not a history entry.
  setSearch: (patch: Partial<ObservabilitySearch>) => void;
  // Push navigation: opening a run or clearing every facet is one.
  setSearchAction: (patch: Partial<ObservabilitySearch>) => void;
};

// Base UI treats an empty string as "no value" and shows the placeholder,
// so the every-tenant option carries a sentinel instead.
export const ALL_OPTION = "*";

// TenantFacet is the tenant scope of the tab. The options are the server's
// tenant list; the current value is always among them so the select never
// shows a blank for a tenant the list has not loaded yet.
export function TenantFacet({
  tenantId,
  allowAllTenants,
  setSearch,
}: Pick<ObservabilityTabProps, "tenantId" | "allowAllTenants" | "setSearch">) {
  const list = useTenantList();
  const options = useMemo(() => {
    const ids = new Set<string>();
    if (tenantId) ids.add(tenantId);
    if (list.kind === "loaded") {
      for (const tenant of list.tenants) ids.add(tenant.id);
    }
    const entries = [...ids].sort().map((id) => ({ value: id, label: id }));
    return allowAllTenants
      ? [{ value: ALL_OPTION, label: "all tenants" }, ...entries]
      : entries;
  }, [allowAllTenants, list, tenantId]);
  return (
    <Select
      label="Tenant"
      value={tenantId ?? ALL_OPTION}
      options={options}
      placeholder="no tenant"
      onChange={(next) =>
        setSearch({ tenant: next === ALL_OPTION ? undefined : next })
      }
      testid="observability-filter-tenant"
    />
  );
}

// SystemLensButton opens the `_nimbus` system tenant lens, the console's
// view of the server's own tables, from the facet bar. The lens is the
// cross-tenant view; the tab never is.
export function SystemLensButton() {
  const setLensOpen = useUiStore((s) => s.setLensOpen);
  return (
    <FacetButton
      onClick={(e) => setLensOpen(true, e.currentTarget)}
      title="Open the _nimbus system tenant lens (⌘\\)"
      testid="observability-open-lens"
    >
      System ⌘\
    </FacetButton>
  );
}
