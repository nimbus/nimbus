import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { type ServiceDoc, ServicesTable } from "../app/services";

export const Route = createFileRoute("/admin/services")({
  component: AdminServicesPage,
});

function AdminServicesPage() {
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as ServiceDoc[] | undefined;

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services" },
      children: <AdminServicesSubDrawer services={services} />,
    }),
    [services],
  );
  useContributeSubDrawer(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-admin-services"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Services
          </h1>
          <p className="text-sm text-muted">
            Every service running on this Nimbus cluster, grouped by tenant.
            Operator-only view: inspect placement, restarts, density, and
            bundle drift across all tenants.
          </p>
        </div>
        <SummaryChip services={services} />
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        <ServicesTable
          services={services}
          activeTenant={null}
          showTenantColumn
        />
      </div>
    </section>
  );
}

function SummaryChip({ services }: { services: ServiceDoc[] | undefined }) {
  if (services === undefined) {
    return (
      <span
        className="font-mono text-[11px] text-muted"
        data-testid="admin-services-summary-loading"
      >
        services: loading…
      </span>
    );
  }
  const tenants = new Set<string>();
  for (const svc of services) {
    if (svc.tenantId) tenants.add(svc.tenantId);
  }
  return (
    <span
      className="font-mono text-[11px] text-muted"
      data-testid="admin-services-summary"
    >
      {services.length} service{services.length === 1 ? "" : "s"} · {tenants.size}{" "}
      tenant{tenants.size === 1 ? "" : "s"}
    </span>
  );
}

function AdminServicesSubDrawer({
  services,
}: {
  services: ServiceDoc[] | undefined;
}) {
  const filter = useSubDrawerSearch().trim().toLowerCase();
  if (services === undefined) {
    return (
      <div className="px-3 py-3 text-xs text-muted">
        <span aria-hidden>·</span>
        <span className="sr-only">loading</span>
      </div>
    );
  }
  if (services.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        <p>No services registered.</p>
        <p className="mt-2">
          Services appear here once a tenant deploys a runtime bundle.
        </p>
      </div>
    );
  }
  const filtered = filter
    ? services.filter(
        (s) =>
          (s.name ?? "").toLowerCase().includes(filter) ||
          (s.state ?? "").toLowerCase().includes(filter) ||
          (s.tenantId ?? "").toLowerCase().includes(filter) ||
          (s.kind ?? "").toLowerCase().includes(filter),
      )
    : services;
  if (filtered.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        No services match the filter.
      </div>
    );
  }
  const grouped = groupByTenant(filtered);
  return (
    <ul className="flex flex-col gap-2 px-2 py-2">
      {grouped.map(([tenant, items]) => (
        <li key={tenant} className="flex flex-col gap-px">
          <div className="px-2 pb-1 pt-2 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
            {tenant}
          </div>
          {items.map((svc) => (
            <Link
              key={svc._id}
              to="/admin/services/$service"
              params={{ service: svc._id }}
              data-testid={`sub-drawer-item-op-service-${svc.name ?? svc._id}`}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default",
              )}
            >
              <span className="flex-1 truncate font-mono text-xs">
                {svc.name ?? shortId(svc._id, 12)}
              </span>
              {svc.state ? (
                <span className="tabular font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
                  {svc.state}
                </span>
              ) : null}
            </Link>
          ))}
        </li>
      ))}
    </ul>
  );
}

function groupByTenant(
  services: ServiceDoc[],
): Array<[string, ServiceDoc[]]> {
  const map = new Map<string, ServiceDoc[]>();
  for (const svc of services) {
    const tenant = svc.tenantId ?? "(none)";
    const existing = map.get(tenant);
    if (existing) existing.push(svc);
    else map.set(tenant, [svc]);
  }
  return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
}
