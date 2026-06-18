import { createFileRoute, Link, useRouter } from "@tanstack/react-router";
import { useCallback, useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { ServiceDoc } from "../../lib/types/service";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { ServicesTable } from "../developer/services";

export const Route = createFileRoute("/operator/services")({
  loader: async () => {
    const services = await getNimbusClient().query(api.services.list, {
      tenantId: null,
      machineId: null,
      state: null,
      limit: 200,
    });
    return { services };
  },
  component: AdminServicesPage,
  errorComponent: AdminServicesLoaderError,
});

export function AdminServicesLoaderError({ error }: { error: Error }) {
  const router = useRouter();
  const reload = useCallback(() => {
    void router.invalidate();
  }, [router]);
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-admin-services"
    >
      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        <EmptyState
          title="Services endpoint unavailable"
          body={
            <>
              The operator services query failed:{" "}
              <span
                className="font-mono text-default"
                data-testid="storage-server-error"
              >
                {error.message}
              </span>
              . Retry once the backend is reachable.
            </>
          }
          cta={{ label: "Retry", onClick: reload }}
          testid="storage-server-error-envelope"
        />
      </div>
    </section>
  );
}

function AdminServicesPage() {
  const { services } = Route.useLoaderData();

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
      <PageHeader
        title="Services"
        subtitle="Every service running on this Nimbus cluster, grouped by tenant. Operator-only view: inspect placement, restarts, density, and bundle drift across all tenants."
        trailing={<SummaryChip services={services} />}
      />

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

function SummaryChip({ services }: { services: ServiceDoc[] }) {
  const tenants = new Set<string>();
  for (const svc of services) {
    if (svc.tenantId) tenants.add(svc.tenantId);
  }
  return (
    <span
      className="whitespace-nowrap font-mono text-[11px] text-muted"
      data-testid="admin-services-summary"
    >
      {services.length} service{services.length === 1 ? "" : "s"} ·{" "}
      {tenants.size} tenant{tenants.size === 1 ? "" : "s"}
    </span>
  );
}

function AdminServicesSubDrawer({ services }: { services: ServiceDoc[] }) {
  const filter = useSubDrawerSearch().trim().toLowerCase();
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
              to="/operator/services/$service"
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

export function groupByTenant(
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
