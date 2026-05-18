import {
  createFileRoute,
  Link,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { StateChip } from "../../components/state-chip";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
  useSubDrawerSearch,
} from "../../shell/sub-drawer";
import type { ServiceDoc } from "../app/services";

export type DetailTab = "placement";

export const TABS: ReadonlyArray<{ id: DetailTab; label: string }> = [
  { id: "placement", label: "Placement" },
];

type DetailSearch = {
  tab?: DetailTab;
};

type BundleDoc = {
  _id: string;
  sha256?: string;
  status?: string;
  sourceRef?: string;
  _creationTime?: number;
};

type MachineDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  hostname?: string;
  state?: string;
};

export const Route = createFileRoute("/admin/services_/$service")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
  }),
  component: AdminServiceDetailPage,
});

export function isTab(value: unknown): value is DetailTab {
  return value === "placement";
}

function AdminServiceDetailPage() {
  const { service: serviceId } = Route.useParams();
  const search = useSearch({ from: "/admin/services_/$service" });
  const navigate = useNavigate();
  const tab: DetailTab = search.tab ?? "placement";

  const service = useQuery(api.services.byId, {
    id: serviceId as never,
  }) as ServiceDoc | null | undefined;

  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as ServiceDoc[] | undefined;

  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;
  const bundle = useMemo<BundleDoc | null>(() => {
    if (!service?.bundleId || !bundles) return null;
    return bundles.find((b) => b._id === service.bundleId) ?? null;
  }, [service, bundles]);

  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services" },
      children: (
        <AdminDetailSubDrawer
          services={services}
          activeServiceId={serviceId}
        />
      ),
    }),
    [services, serviceId],
  );
  useContributeSubDrawer(spec);

  const setTab = (next: DetailTab) =>
    navigate({
      to: "/admin/services/$service",
      params: { service: serviceId },
      search: { tab: next },
      replace: true,
    });

  const displayName = service?.name ?? shortId(serviceId, 12);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-admin-service-detail"
    >
      <div className="flex shrink-0 flex-col gap-2 border-b border-app px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Services", href: "/admin/services" },
            { label: displayName, active: true },
          ]}
        />
        <header className="flex flex-wrap items-baseline gap-3">
          <h1
            className="font-mono text-default"
            style={{ fontSize: "var(--text-lg)" }}
          >
            {displayName}
          </h1>
          {service?.kind ? (
            <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
              {service.kind}
            </span>
          ) : null}
          {service?.state ? <StateChip state={service.state} /> : null}
          {service?.tenantId ? (
            <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
              {service.tenantId}
            </span>
          ) : null}
          {bundle?.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="admin-service-detail-bundle"
            >
              {shortId(bundle.sha256, 12)}
            </CopyChip>
          ) : null}
        </header>
      </div>

      <nav
        aria-label="Admin service detail sections"
        className="flex shrink-0 gap-px border-b border-app bg-surface-2 px-6"
        data-testid="admin-service-detail-tabs"
      >
        {TABS.map((t) => {
          const isActive = tab === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`admin-service-detail-tab-${t.id}`}
              className={cn(
                "flex items-center px-3 py-2 font-mono text-xs uppercase tracking-wide",
                isActive
                  ? "border-b-2 border-[color:var(--color-brand)] text-default"
                  : "text-muted hover:text-default",
              )}
            >
              {t.label}
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden">
        {service === undefined ? (
          <Loading label="Loading service…" />
        ) : service === null ? (
          <NotFound id={serviceId} />
        ) : (
          <PlacementTab service={service} />
        )}
      </div>
    </section>
  );
}

function PlacementTab({ service }: { service: ServiceDoc }) {
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as MachineDoc[] | undefined;
  const machine = useMemo<MachineDoc | null>(() => {
    if (!service.machineId || !machines) return null;
    return machines.find((m) => m._id === service.machineId) ?? null;
  }, [service.machineId, machines]);

  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-default"
      data-testid="admin-service-tab-placement"
    >
      <Stat label="Tenant" value={service.tenantId ?? "—"} />
      <Stat
        label="Machine"
        value={
          service.machineId ? (
            <Link
              to="/admin/machines"
              className="font-mono text-default hover:underline"
            >
              {machine?.name ?? shortId(service.machineId, 12)}
            </Link>
          ) : (
            "—"
          )
        }
      />
      <Stat label="Host" value={machine?.hostname ?? "—"} />
      <Stat
        label="Machine state"
        value={machine?.state ? <StateChip state={machine.state} /> : "—"}
      />
      <div className="rounded border border-app bg-surface-2 px-3 py-3 text-xs text-muted">
        Placement cost, region affinity, and scheduler hints are not yet
        exposed by the system tenant. Follow-up plans will surface these once
        the placement controller is wired.
      </div>
    </div>
  );
}

function Stat({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
        {label}
      </span>
      <span className="font-mono text-xs text-default">{value}</span>
    </div>
  );
}

function AdminDetailSubDrawer({
  services,
  activeServiceId,
}: {
  services: ServiceDoc[] | undefined;
  activeServiceId: string;
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
          (s.tenantId ?? "").toLowerCase().includes(filter) ||
          (s.kind ?? "").toLowerCase().includes(filter),
      )
    : services;
  if (services.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-muted">No services registered.</div>
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
      {filtered.map((svc) => {
        const isActive = svc._id === activeServiceId;
        return (
          <li key={svc._id}>
            <Link
              to="/admin/services/$service"
              params={{ service: svc._id }}
              data-testid={`sub-drawer-item-op-service-${svc.name ?? svc._id}`}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md px-2 text-sm",
                isActive
                  ? "bg-surface-2 text-default"
                  : "text-muted hover:bg-surface-2 hover:text-default",
              )}
            >
              <span className="flex-1 truncate font-mono text-xs">
                {svc.name ?? shortId(svc._id, 12)}
              </span>
              {svc.tenantId ? (
                <span className="tabular font-mono text-[10px] text-muted">
                  {svc.tenantId}
                </span>
              ) : null}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center text-xs text-muted">
      {label}
    </div>
  );
}

function NotFound({ id }: { id: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
      <span className="font-mono text-sm text-default">Service not found</span>
      <span className="max-w-md text-xs text-muted">
        No service matches the id{" "}
        <code className="font-mono text-default">{shortId(id, 12)}</code>.
      </span>
      <Link
        to="/admin/services"
        className="rounded border border-app px-3 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        ← back to services
      </Link>
    </div>
  );
}

