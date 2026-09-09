import {
  createFileRoute,
  Link,
  notFound,
  useNavigate,
  useSearch,
} from "@tanstack/react-router";
import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { api } from "../../../convex/_generated/api";
import type { Doc, Id } from "../../../convex/_generated/dataModel";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { EmptyState } from "../../components/empty-state";
import { StatePill } from "../../components/pill";
import { ServiceDetailLoaderError } from "../../components/service-loader-errors";
import { RelativeTime } from "../../components/time";
import { shortHash, shortId } from "../../lib/format";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { ServiceDoc } from "../../lib/types/service";
import {
  type SubPanelSpec,
  useContributeSubPanel,
  useSubPanelSearch,
} from "../../shell/sub-panel";
import { useUiStore } from "../../store/ui-store";

type DetailTab = "overview" | "endpoints" | "health" | "bundle";

const TABS: Array<{ id: DetailTab; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "endpoints", label: "Endpoints" },
  { id: "health", label: "Health" },
  { id: "bundle", label: "Bundle" },
];

type DetailSearch = {
  tab?: DetailTab;
};

export const Route = createFileRoute("/developer/services_/$service")({
  validateSearch: (search: Record<string, unknown>): DetailSearch => ({
    tab: isTab(search.tab) ? search.tab : undefined,
  }),
  loaderDeps: () => ({
    activeTenant: useUiStore.getState().activeTenant,
  }),
  loader: async ({ params, deps }) => {
    const client = getNimbusClient();
    const [service, services, bundles] = await Promise.all([
      client.query(api.services.byId, {
        id: params.service as Id<"services">,
      }),
      client.query(api.services.list, {
        tenantId: deps.activeTenant,
        machineId: null,
        state: null,
        limit: 200,
      }),
      client.query(api.bundles.list, { status: null, limit: 50 }),
    ]);
    if (!service) throw notFound();
    return { service, services, bundles, activeTenant: deps.activeTenant };
  },
  notFoundComponent: ServiceNotFound,
  errorComponent: ServiceDetailLoaderError,
  component: ServiceDetailPage,
});

function isTab(value: unknown): value is DetailTab {
  return (
    value === "overview" ||
    value === "endpoints" ||
    value === "health" ||
    value === "bundle"
  );
}

function ServiceDetailPage() {
  const { service: serviceId } = Route.useParams();
  const { service, services, bundles } = Route.useLoaderData();
  const search = useSearch({ from: "/developer/services_/$service" });
  const navigate = useNavigate();
  const tab: DetailTab = search.tab ?? "overview";

  const bundle = useMemo<Doc<"bundles"> | null>(() => {
    if (!service.bundleId) return null;
    return bundles.find((b) => b._id === service.bundleId) ?? null;
  }, [service, bundles]);

  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Services",
      search: { placeholder: "Filter services", rows: services.length },
      children: (
        <DetailSubPanel services={services} activeServiceId={serviceId} />
      ),
    }),
    [services, serviceId],
  );
  useContributeSubPanel(spec);

  const setTab = (next: DetailTab) =>
    navigate({
      to: "/developer/services/$service",
      params: { service: serviceId },
      search: { tab: next },
      replace: true,
    });

  const displayName = service.name ?? shortId(serviceId, 12);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-service-detail"
    >
      <div className="flex shrink-0 flex-col gap-2 border-b border-border-2 px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Services", href: "/developer/services" },
            { label: displayName, active: true },
          ]}
        />
        <header className="flex flex-wrap items-baseline gap-3">
          <h1
            className="font-mono text-text-1"
            style={{ fontSize: "var(--text-lg)" }}
          >
            {displayName}
          </h1>
          {service.kind ? (
            <span className="rounded-xs border border-border-2 px-1.5 py-0.5 text-xs font-medium text-text-3">
              {service.kind}
            </span>
          ) : null}
          {service.state ? <StatePill state={service.state} /> : null}
          {bundle?.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="service-detail-bundle"
            >
              {shortHash(bundle.sha256, 12)}
            </CopyChip>
          ) : null}
        </header>
      </div>

      <nav
        aria-label="Service detail sections"
        className="flex shrink-0 gap-px border-b border-border-2 bg-bg-raised px-6"
        data-testid="service-detail-tabs"
      >
        {TABS.map((t) => {
          const isActive = tab === t.id;
          return (
            <button
              key={t.id}
              type="button"
              onClick={() => setTab(t.id)}
              aria-current={isActive ? "page" : undefined}
              data-testid={`service-detail-tab-${t.id}`}
              className={cn(
                "flex items-center px-3 py-2 text-xs font-medium",
                isActive
                  ? "border-b-2 border-[color:var(--accent)] text-text-1"
                  : "text-text-3 hover:text-text-1",
              )}
            >
              {t.label}
            </button>
          );
        })}
      </nav>

      <div className="min-h-0 flex-1 overflow-hidden">
        <TabBody tab={tab} service={service} bundle={bundle} />
      </div>
    </section>
  );
}

function TabBody({
  tab,
  service,
  bundle,
}: {
  tab: DetailTab;
  service: ServiceDoc;
  bundle: Doc<"bundles"> | null;
}) {
  if (tab === "overview") return <OverviewTab service={service} />;
  if (tab === "endpoints") return <EndpointsTab service={service} />;
  if (tab === "health") return <HealthTab service={service} />;
  return <BundleTab service={service} bundle={bundle} />;
}

function OverviewTab({ service }: { service: ServiceDoc }) {
  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="service-tab-overview"
    >
      <Stat label="Name" value={service.name ?? "—"} />
      <Stat label="Kind" value={service.kind ?? "—"} />
      <Stat label="State" value={<StatePill state={service.state} />} />
      <Stat label="Tenant" value={service.tenantId ?? "—"} />
      <Stat label="Machine" value={service.machineId ?? "—"} />
      <Stat
        label="Updated"
        value={
          typeof service._updateTime === "number" ? (
            <RelativeTime epochMs={service._updateTime} />
          ) : (
            "—"
          )
        }
      />
      <div className="rounded-xs border border-border-2 bg-bg-raised px-3 py-3 text-xs text-text-3">
        Logs, environment variables, ports, and code-ref details are not yet
        surfaced by the system tenant for the services index. A follow-up plan
        will wire these dimensions through the ServiceManager and compose.yaml
        metadata.
      </div>
    </div>
  );
}

function EndpointsTab({ service }: { service: ServiceDoc }) {
  const endpoints = Array.isArray(service.endpoints) ? service.endpoints : [];
  if (endpoints.length === 0) {
    return (
      <EmptyState
        title="No endpoints declared"
        body="Services expose endpoints once their compose.yaml binds host ports or a sidecar registers a route."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4"
      data-testid="service-tab-endpoints"
    >
      <pre className="m-0 rounded-xs border border-border-2 bg-bg-raised p-3 font-mono text-xs text-text-1">
        {JSON.stringify(endpoints, null, 2)}
      </pre>
    </div>
  );
}

function HealthTab({ service }: { service: ServiceDoc }) {
  const health = service.health;
  if (!health) {
    return (
      <EmptyState
        title="No health snapshot"
        body="Health probes populate this panel once the service manager records its first readiness check."
      />
    );
  }
  return (
    <div
      className="h-full overflow-auto px-6 py-4"
      data-testid="service-tab-health"
    >
      <pre className="m-0 rounded-xs border border-border-2 bg-bg-raised p-3 font-mono text-xs text-text-1">
        {JSON.stringify(health, null, 2)}
      </pre>
    </div>
  );
}

export function BundleTab({
  service,
  bundle,
}: {
  service: ServiceDoc;
  bundle: Doc<"bundles"> | null;
}) {
  if (!service.bundleId) {
    return (
      <EmptyState
        title="No bundle attached"
        body={
          <>
            This service has not been associated with a runtime bundle. Run{" "}
            <code className="whitespace-nowrap">nimbus compose up</code> to
            register one.
          </>
        }
      />
    );
  }
  if (bundle === null) {
    return (
      <EmptyState
        title="Bundle not found"
        body={`Service references bundleId ${shortId(service.bundleId, 12)} but no matching bundle is registered.`}
      />
    );
  }
  return (
    <div
      className="flex h-full flex-col gap-3 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="service-tab-bundle"
    >
      <Stat
        label="SHA-256"
        value={
          bundle.sha256 ? (
            <CopyChip
              label="bundle sha256"
              value={bundle.sha256}
              testid="service-bundle-sha"
            >
              {shortHash(bundle.sha256, 16)}
            </CopyChip>
          ) : (
            "—"
          )
        }
      />
      <Stat label="Status" value={bundle.status ?? "—"} />
      <Stat label="Source ref" value={bundle.sourceRef ?? "—"} />
      <Stat
        label="Registered"
        value={
          typeof bundle._creationTime === "number" ? (
            <RelativeTime epochMs={bundle._creationTime} />
          ) : (
            "—"
          )
        }
      />
    </div>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="w-32 text-xs font-medium text-text-3">{label}</span>
      <span className="font-mono text-xs text-text-1">{value}</span>
    </div>
  );
}

function DetailSubPanel({
  services,
  activeServiceId,
}: {
  services: ServiceDoc[];
  activeServiceId: string;
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
      <div className="px-3 py-6 text-xs text-text-3">No services declared.</div>
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
      {filtered.map((svc) => {
        const isActive = svc._id === activeServiceId;
        return (
          <li key={svc._id}>
            <Link
              to="/developer/services/$service"
              params={{ service: svc._id }}
              data-testid={`sub-panel-item-dev-service-${svc.name ?? svc._id}`}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md px-2 text-sm",
                isActive
                  ? "bg-bg-raised text-text-1"
                  : "text-text-3 hover:bg-bg-raised hover:text-text-1",
              )}
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
        );
      })}
    </ul>
  );
}

function ServiceNotFound() {
  const { service: serviceId } = Route.useParams();
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-2 text-center"
      data-testid="service-not-found"
    >
      <span className="font-mono text-sm text-text-1">Service not found</span>
      <span className="max-w-md text-xs text-text-3">
        No service matches the id{" "}
        <code className="font-mono text-text-1">{shortId(serviceId, 12)}</code>.
        It may have been stopped or never registered.
      </span>
      <Link
        to="/developer/services"
        className="rounded-xs border border-border-2 px-3 py-1 text-xs font-medium text-text-3 hover:bg-bg-panel hover:text-text-1"
      >
        ← back to services
      </Link>
    </div>
  );
}
