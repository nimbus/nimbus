import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import { useUiStore } from "../../store/ui-store";

export const Route = createFileRoute("/app/services")({
  component: ServicesPage,
});

export type ServiceDoc = {
  _id: string;
  _updateTime?: number;
  tenantId?: string;
  name?: string;
  machineId?: string;
  bundleId?: string;
  kind?: string;
  state?: string;
  endpoints?: unknown[];
  health?: unknown;
};

function ServicesPage() {
  const activeTenant = useUiStore((s) => s.activeTenant);
  const services = useQuery(api.services.list, {
    tenantId: activeTenant,
    machineId: null,
    state: null,
    limit: 200,
  }) as ServiceDoc[] | undefined;

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
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Services
          </h1>
          <p className="text-sm text-muted">
            Services this tenant declares in <code>compose.yaml</code>. They run
            as microVMs on Linux and as containers inside the developer machine
            VM on macOS.
          </p>
        </div>
        <ScopeChip activeTenant={activeTenant} />
      </header>

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
  return (
    <span
      className="rounded border border-app px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted"
      data-testid="services-scope"
    >
      tenant: {activeTenant ?? "all"}
    </span>
  );
}

function ServicesSubDrawer({
  services,
  activeTenant,
}: {
  services: ServiceDoc[] | undefined;
  activeTenant: string | null;
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
          <code className="font-mono">nimbus compose up</code> to register
          services for this tenant.
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
            to="/app/services/$service"
            params={{ service: svc._id }}
            data-testid={`sub-drawer-item-dev-service-${svc.name ?? svc._id}`}
            className="flex h-8 items-center gap-2 rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default"
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
  services: ServiceDoc[] | undefined;
  activeTenant: string | null;
  showTenantColumn: boolean;
}) {
  if (services === undefined) {
    return (
      <div className="flex h-32 items-center justify-center text-xs text-muted">
        Loading services…
      </div>
    );
  }
  if (services.length === 0) {
    return (
      <div className="flex h-32 flex-col items-center justify-center gap-1 text-center">
        <span className="font-mono text-sm text-default">No services</span>
        <span className="max-w-md text-xs text-muted">
          {activeTenant
            ? "This tenant has no declared services. Add them to compose.yaml and run `nimbus compose up`."
            : "No services declared across any tenant."}
        </span>
      </div>
    );
  }
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="services-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
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
            const endpoints = Array.isArray(svc.endpoints)
              ? svc.endpoints
              : [];
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
                        ? "/admin/services/$service"
                        : "/app/services/$service"
                    }
                    params={{ service: svc._id }}
                    className="font-mono text-default hover:underline"
                  >
                    {svc.name ?? shortId(svc._id, 12)}
                  </Link>
                </Td>
                <Td>
                  <span className="font-mono text-xs uppercase tracking-wide text-muted">
                    {svc.kind ?? "—"}
                  </span>
                </Td>
                <Td>
                  <StateChip state={svc.state} />
                </Td>
                {showTenantColumn ? (
                  <Td>
                    <span className="font-mono text-xs text-default">
                      {svc.tenantId ?? "—"}
                    </span>
                  </Td>
                ) : null}
                <Td>
                  <span className="font-mono text-xs text-default">
                    {svc.machineId ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-muted">
                    {endpoints.length}
                  </span>
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

function Th({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <th
      className={cn(
        "border-b border-app px-3 py-2 text-left font-normal",
        className,
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <td className={cn("px-3 py-2 align-middle", className)}>{children}</td>
  );
}
