import {
  createFileRoute,
  Link,
  notFound,
  useSearch,
} from "@tanstack/react-router";
import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { api } from "../../../convex/_generated/api";
import type { Doc, Id } from "../../../convex/_generated/dataModel";
import { Breadcrumb } from "../../components/breadcrumb";
import { CopyChip } from "../../components/copy-chip";
import { EmptyState } from "../../components/empty-state";
import { PageTabs } from "../../components/page-tabs";
import { CategoryPill, StatePill } from "../../components/pill";
import { ServiceDetailLoaderError } from "../../components/service-loader-errors";
import { StateDot } from "../../components/state-dot";
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
import { LifecycleButtons, shownStateOf } from "./services/-lifecycle-buttons";
import { useServiceActions } from "./services/-service-lifecycle";
import { ServiceLogs } from "./services/-service-logs";

export type DetailTab = "overview" | "logs" | "config";

export const TABS: ReadonlyArray<{ id: DetailTab; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "logs", label: "Logs" },
  { id: "config", label: "Config" },
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

export function isTab(value: unknown): value is DetailTab {
  return value === "overview" || value === "logs" || value === "config";
}

function ServiceDetailPage() {
  const { service: serviceId } = Route.useParams();
  const { service, services, bundles } = Route.useLoaderData();
  const search = useSearch({ from: "/developer/services_/$service" });
  const tab: DetailTab = search.tab ?? "overview";
  const actions = useServiceActions();

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

  const displayName = service.name ?? shortId(serviceId, 12);

  return (
    <section
      className="flex h-full flex-col overflow-hidden"
      data-testid="page-service-detail"
    >
      <div className="flex shrink-0 flex-col gap-3 border-b border-border-2 px-6 pb-3 pt-4">
        <Breadcrumb
          segments={[
            { label: "Services", href: "/developer/services" },
            { label: displayName, active: true },
          ]}
        />
        <header className="flex flex-wrap items-start justify-between gap-3">
          <div className="flex flex-wrap items-baseline gap-3">
            <h1
              className="font-mono text-text-1"
              style={{ fontSize: "var(--text-lg)" }}
            >
              {displayName}
            </h1>
            {service.kind ? <CategoryPill value={service.kind} /> : null}
            <StatePill
              state={shownStateOf(service, actions)}
              data-testid="service-detail-state"
            />
            {bundle?.sha256 ? (
              <CopyChip
                label="bundle sha256"
                value={bundle.sha256}
                testid="service-detail-bundle"
              >
                {shortHash(bundle.sha256, 12)}
              </CopyChip>
            ) : null}
          </div>
          <LifecycleButtons
            service={service}
            actions={actions}
            testid="service-detail-action"
          />
        </header>
        <PageTabs
          label="Service detail sections"
          tabs={TABS}
          active={tab}
          testid="service-detail-tabs"
          itemTestid="service-detail-tab"
        />
      </div>

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
  if (tab === "logs") {
    return (
      <ServiceLogs
        tenantId={service.tenantId}
        serviceName={service.name ?? service._id}
        testid="service-tab-logs"
      />
    );
  }
  if (tab === "config") return <ConfigTab service={service} bundle={bundle} />;
  return <OverviewTab service={service} />;
}

function OverviewTab({ service }: { service: ServiceDoc }) {
  const endpoints = Array.isArray(service.endpoints) ? service.endpoints : [];
  return (
    <div
      className="flex h-full flex-col gap-5 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="service-tab-overview"
    >
      <div className="flex flex-col gap-2">
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
      </div>

      <Panel title="Endpoints" testid="service-overview-endpoints">
        {endpoints.length === 0 ? (
          <p className="text-xs text-text-3">
            No endpoints declared. A service exposes endpoints once its{" "}
            <code>compose.yaml</code> binds host ports or a sidecar registers a
            route.
          </p>
        ) : (
          <pre className="m-0 font-mono text-xs text-text-1">
            {JSON.stringify(endpoints, null, 2)}
          </pre>
        )}
      </Panel>

      <Panel title="Health" testid="service-overview-health">
        {service.health ? (
          <pre className="m-0 font-mono text-xs text-text-1">
            {JSON.stringify(service.health, null, 2)}
          </pre>
        ) : (
          <p className="text-xs text-text-3">
            No health snapshot yet. The service manager records one after the
            first readiness check.
          </p>
        )}
      </Panel>
    </div>
  );
}

// Config is what the operator declared and what the manager attached to
// it: the runtime bundle, the endpoint bindings, and where the rest lives.
function ConfigTab({
  service,
  bundle,
}: {
  service: ServiceDoc;
  bundle: Doc<"bundles"> | null;
}) {
  const endpoints = Array.isArray(service.endpoints) ? service.endpoints : [];
  return (
    <div
      className="flex h-full flex-col gap-5 overflow-auto px-6 py-4 text-sm text-text-1"
      data-testid="service-tab-config"
    >
      <Panel title="Bundle" testid="service-config-bundle">
        <BundleTab service={service} bundle={bundle} />
      </Panel>
      <Panel title="Endpoint bindings" testid="service-config-endpoints">
        {endpoints.length === 0 ? (
          <p className="text-xs text-text-3">No endpoint bindings declared.</p>
        ) : (
          <pre className="m-0 font-mono text-xs text-text-1">
            {JSON.stringify(endpoints, null, 2)}
          </pre>
        )}
      </Panel>
      <p className="text-xs text-text-3" data-testid="service-config-note">
        Environment, ports, and the code ref live in <code>compose.yaml</code>{" "}
        beside the source. The console shows what the service manager has
        applied; edit the file and run{" "}
        <code className="whitespace-nowrap">nimbus compose up</code> to change
        it.
      </p>
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
        className="py-6"
      />
    );
  }
  if (bundle === null) {
    return (
      <EmptyState
        title="Bundle not found"
        body={`Service references bundleId ${shortId(service.bundleId, 12)} but no matching bundle is registered.`}
        className="py-6"
      />
    );
  }
  return (
    <div
      className="flex flex-col gap-2 text-sm text-text-1"
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

function Panel({
  title,
  testid,
  children,
}: {
  title: string;
  testid: string;
  children: React.ReactNode;
}) {
  return (
    <section
      className="flex flex-col gap-2 rounded-md border border-border-2 bg-bg-panel px-4 py-3"
      data-testid={testid}
    >
      <h2 className="text-xs font-medium text-text-3">{title}</h2>
      {children}
    </section>
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
              <StateDot state={svc.state} />
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
