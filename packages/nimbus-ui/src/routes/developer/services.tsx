import { createFileRoute, Link } from "@tanstack/react-router";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { Td, Th } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { ServicesLoaderError } from "../../components/service-loader-errors";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { ServiceDoc } from "../../lib/types/service";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { useUiStore } from "../../store/ui-store";

export const Route = createFileRoute("/developer/services")({
  loaderDeps: () => ({
    activeTenant: useUiStore.getState().activeTenant,
  }),
  loader: async ({ deps }) => {
    const services = await getNimbusClient().query(api.services.list, {
      tenantId: deps.activeTenant,
      machineId: null,
      state: null,
      limit: 200,
    });
    return { services, activeTenant: deps.activeTenant };
  },
  component: ServicesPage,
  errorComponent: ServicesLoaderError,
});

function ServicesPage() {
  const { services } = Route.useLoaderData();
  const activeTenant = useUiStore((s) => s.activeTenant);

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services" },
      children: (
        <ServicesSubDrawer services={services} activeTenant={activeTenant} />
      ),
    }),
    [services, activeTenant],
  );
  useContributeSubDrawer(spec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-services"
    >
      <PageHeader
        title="Services"
        subtitle={
          <>
            Services this tenant declares in <code>compose.yaml</code> —
            microVMs on Linux, containers in a macOS machine VM.
          </>
        }
        trailing={<ScopeChip activeTenant={activeTenant} />}
      />

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        <ServicesTable
          services={services}
          activeTenant={activeTenant}
          showTenantColumn={false}
        />
      </div>
    </section>
  );
}

function ScopeChip({ activeTenant }: { activeTenant: string | null }) {
  if (activeTenant === null) return null;
  return (
    <span
      className="inline-flex items-center gap-1 rounded border border-app px-2 py-0.5 font-mono text-xs text-muted"
      data-testid="services-scope"
    >
      <span className="uppercase tracking-wide">tenant</span>
      <span className="font-mono text-default">{activeTenant}</span>
    </span>
  );
}

function ServicesSubDrawer({
  services,
  activeTenant,
}: {
  services: ServiceDoc[];
  activeTenant: string | null;
}) {
  const filter = useSubDrawerSearch().trim().toLowerCase();
  const filtered = filter
    ? services.filter(
        (s) =>
          (s.name ?? "").toLowerCase().includes(filter) ||
          (s.state ?? "").toLowerCase().includes(filter) ||
          (s.kind ?? "").toLowerCase().includes(filter),
      )
    : services;
  if (services.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        <p>No services declared.</p>
        <p className="mt-2">
          Author a <code>compose.yaml</code> and run{" "}
          <code className="whitespace-nowrap">nimbus compose up</code> to
          register services for{" "}
          {activeTenant ? `tenant ${activeTenant}` : "this tenant"}.
        </p>
      </div>
    );
  }
  if (filtered.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">
        No services match the filter.
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {filtered.map((svc) => (
        <li key={svc._id}>
          <Link
            to="/developer/services/$service"
            params={{ service: svc._id }}
            data-testid={`sub-drawer-item-dev-service-${svc.name ?? svc._id}`}
            className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default"
          >
            <span className="flex-1 truncate font-mono text-xs">
              {svc.name ?? shortId(svc._id, 12)}
            </span>
            {svc.state ? (
              <span className="tabular font-mono text-xs uppercase tracking-[0.18em] text-muted">
                {svc.state}
              </span>
            ) : null}
          </Link>
        </li>
      ))}
    </ul>
  );
}

export function ServicesTable({
  services,
  activeTenant,
  showTenantColumn,
}: {
  services: ServiceDoc[];
  activeTenant: string | null;
  showTenantColumn: boolean;
}) {
  if (services.length === 0) {
    return (
      <EmptyState
        title="No services"
        body={
          activeTenant ? (
            <>
              This tenant has no declared services. Add them to{" "}
              <code>compose.yaml</code> and run{" "}
              <code className="whitespace-nowrap">nimbus compose up</code>.
            </>
          ) : (
            "No services declared across any tenant."
          )
        }
        testid="services-empty"
      />
    );
  }
  return (
    <div className="h-full overflow-auto">
      <table
        className="w-full border-collapse text-base"
        data-testid="services-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Name</Th>
            <Th>Kind</Th>
            <Th>State</Th>
            {showTenantColumn ? <Th>Tenant</Th> : null}
            <Th>Machine</Th>
            <Th>Endpoints</Th>
            <Th>Updated</Th>
          </tr>
        </thead>
        <tbody>
          {services.map((svc) => {
            const endpoints = Array.isArray(svc.endpoints) ? svc.endpoints : [];
            return (
              <tr
                key={svc._id}
                className="border-t border-app hover:bg-surface-2"
                data-testid={`services-row-${svc.name ?? svc._id}`}
              >
                <Td>
                  <Link
                    to={
                      showTenantColumn
                        ? "/operator/services/$service"
                        : "/developer/services/$service"
                    }
                    params={{ service: svc._id }}
                    className="font-mono text-default hover:underline"
                  >
                    {svc.name ?? shortId(svc._id, 12)}
                  </Link>
                </Td>
                <Td>
                  <span className="font-mono uppercase tracking-wide text-muted">
                    {svc.kind ?? "—"}
                  </span>
                </Td>
                <Td>
                  <StateChip state={svc.state} />
                </Td>
                {showTenantColumn ? <Td mono>{svc.tenantId ?? "—"}</Td> : null}
                <Td mono>{svc.machineId ?? "—"}</Td>
                <Td mono className="text-muted">
                  {endpoints.length}
                </Td>
                <Td>
                  {typeof svc._updateTime === "number" ? (
                    <RelativeTime epochMs={svc._updateTime} />
                  ) : (
                    <span className="tabular text-muted">—</span>
                  )}
                </Td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
