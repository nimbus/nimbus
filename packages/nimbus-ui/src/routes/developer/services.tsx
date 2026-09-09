import { createFileRoute, Link } from "@tanstack/react-router";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { StatePill } from "../../components/pill";
import { ServicesLoaderError } from "../../components/service-loader-errors";
import { Td, Th } from "../../components/table-cells";
import { RelativeTime } from "../../components/time";
import { shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { ServiceDoc } from "../../lib/types/service";
import {
  type SubPanelSpec,
  useContributeSubPanel,
  useSubPanelSearch,
} from "../../shell/sub-panel";
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

  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services", rows: services.length },
      children: (
        <ServicesSubPanel services={services} activeTenant={activeTenant} />
      ),
    }),
    [services, activeTenant],
  );
  useContributeSubPanel(spec);

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

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
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
      className="inline-flex items-center gap-1 rounded-xs border border-border-2 px-2 py-0.5 font-mono text-xs text-text-3"
      data-testid="services-scope"
    >
      <span className="font-medium">tenant</span>
      <span className="font-mono text-text-1">{activeTenant}</span>
    </span>
  );
}

function ServicesSubPanel({
  services,
  activeTenant,
}: {
  services: ServiceDoc[];
  activeTenant: string | null;
}) {
  const filter = useSubPanelSearch().trim().toLowerCase();
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
      <div className="px-3 py-6 text-xs text-text-3">
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
      <div className="px-3 py-6 text-xs text-text-3">
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
            data-testid={`sub-panel-item-dev-service-${svc.name ?? svc._id}`}
            className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-text-3 hover:bg-bg-raised hover:text-text-1"
          >
            <span className="flex-1 truncate font-mono text-xs">
              {svc.name ?? shortId(svc._id, 12)}
            </span>
            {svc.state ? (
              <span className="tabular text-xs font-medium text-text-3">
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
        <thead className="sticky top-0 bg-bg-raised text-xs font-medium text-text-3">
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
                className="border-t border-border-2 hover:bg-bg-raised"
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
                    className="font-mono text-text-1 hover:underline"
                  >
                    {svc.name ?? shortId(svc._id, 12)}
                  </Link>
                </Td>
                <Td>
                  <span className="font-medium text-text-3">
                    {svc.kind ?? "—"}
                  </span>
                </Td>
                <Td>
                  <StatePill state={svc.state} />
                </Td>
                {showTenantColumn ? <Td mono>{svc.tenantId ?? "—"}</Td> : null}
                <Td mono>{svc.machineId ?? "—"}</Td>
                <Td mono className="text-text-3">
                  {endpoints.length}
                </Td>
                <Td>
                  {typeof svc._updateTime === "number" ? (
                    <RelativeTime epochMs={svc._updateTime} />
                  ) : (
                    <span className="tabular text-text-3">—</span>
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
