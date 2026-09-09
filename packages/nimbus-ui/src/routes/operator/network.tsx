import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, redirect } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { api } from "../../../convex/_generated/api";
import { EmptyState } from "../../components/empty-state";
import { SkeletonRows } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { ScrollRegion } from "../../components/scroll-region";
import { Td, Th } from "../../components/table-cells";
import { RelativeTime } from "../../components/time";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { NetworkInventorySection } from "./-network-inventory";

const SECTIONS = ["routes", "ws", "ports", "listeners", "security"] as const;
type NetworkSection = (typeof SECTIONS)[number];

type NetworkSearch = { section: NetworkSection };

function parseSection(value: unknown): NetworkSection | undefined {
  return typeof value === "string" &&
    (SECTIONS as readonly string[]).includes(value)
    ? (value as NetworkSection)
    : undefined;
}

export const Route = createFileRoute("/operator/network")({
  component: NetworkPage,
  validateSearch: (search: Record<string, unknown>): NetworkSearch => ({
    section: parseSection(search.section) ?? "routes",
  }),
  beforeLoad: ({ search }) => {
    if (
      parseSection((search as Record<string, unknown>).section) === undefined
    ) {
      throw redirect({
        to: "/operator/network",
        search: { section: "routes" },
        replace: true,
      });
    }
  },
});

type RouteDoc = {
  _id: string;
  _updateTime?: number;
  method?: string;
  path?: string;
  adapter?: string;
  handler?: string;
  authRequired?: boolean;
  lastRequestAt?: number;
};

// HTTP method tone. `--link` is reserved for <a> elements per DESIGN.md;
// POST uses the product accent (teal) instead, matching its "create" verb
// being a primary action.
const METHOD_TONE: Record<string, string> = {
  GET: "text-success",
  POST: "text-accent",
  PUT: "text-warning",
  PATCH: "text-warning",
  DELETE: "text-error",
  OPTIONS: "text-text-3",
  HEAD: "text-text-3",
};

const NETWORK_SUB_PANEL: SubPanelSpec = {
  kind: "static",
  title: "Network",
  items: [
    {
      id: "routes",
      label: "Routes",
      to: "/operator/network",
      search: { section: "routes" },
    },
    {
      id: "ws",
      label: "WS",
      to: "/operator/network",
      search: { section: "ws" },
    },
    {
      id: "ports",
      label: "Ports",
      to: "/operator/network",
      search: { section: "ports" },
    },
    {
      id: "listeners",
      label: "Listeners",
      to: "/operator/network",
      search: { section: "listeners" },
    },
    {
      id: "security",
      label: "Security",
      to: "/operator/network",
      search: { section: "security" },
    },
  ],
};

function NetworkPage() {
  useContributeSubPanel(NETWORK_SUB_PANEL);
  const section = Route.useSearch().section;
  if (section !== "routes") {
    return <NetworkInventorySection section={section} />;
  }
  return <RoutesSection />;
}

function RoutesSection() {
  const routes = useQuery(api.routes.list, {
    adapter: null,
    limit: 500,
  }) as RouteDoc[] | undefined;

  const [filter, setFilter] = useState("");
  const [adapterFilter, setAdapterFilter] = useState<string | null>(null);

  const adapters = useMemo(() => {
    if (!routes) return [];
    const set = new Set<string>();
    for (const r of routes) {
      if (r.adapter) set.add(r.adapter);
    }
    return Array.from(set).sort();
  }, [routes]);

  const filtered = useMemo(() => {
    if (!routes) return undefined;
    const needle = filter.trim().toLowerCase();
    return routes.filter((r) => {
      if (adapterFilter && r.adapter !== adapterFilter) return false;
      if (!needle) return true;
      const hay =
        `${r.method ?? ""} ${r.path ?? ""} ${r.handler ?? ""} ${r.adapter ?? ""}`.toLowerCase();
      return hay.includes(needle);
    });
  }, [routes, filter, adapterFilter]);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-network"
      data-section="routes"
    >
      <PageHeader
        title="Network"
        subtitle="HTTP routes, listeners, published ports from the live registry — adapters appear as they register."
        trailing={
          <span
            className="font-mono text-xs text-text-3"
            data-testid="network-total"
          >
            {routes === undefined
              ? "loading…"
              : `${filtered?.length ?? 0} of ${routes.length} routes`}
          </span>
        }
      />

      <div
        className="flex flex-wrap items-center gap-2 rounded-md border border-border-2 bg-bg-raised px-3 py-2"
        data-testid="network-filters"
      >
        <label className="flex items-center gap-2">
          <span className="text-xs font-medium text-text-3">filter</span>
          <input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            data-inline-search
            placeholder="method, path, handler"
            data-testid="network-filter-input"
            className="w-72 rounded-xs border border-border-2 bg-bg-panel px-2 py-1 font-mono text-xs text-text-1 placeholder:text-text-3/70"
          />
        </label>
        {/* Toggle buttons, not tabs: they filter the table in place and do not
            own a tabpanel, so a labelled group of `aria-pressed` buttons is the
            honest contract, and native Tab/Space/Enter is its complete keyboard
            behavior. */}
        <fieldset className="flex min-w-0 items-center gap-1">
          <legend className="sr-only">Filter by adapter</legend>
          <FilterChip
            label="all"
            active={adapterFilter === null}
            onClick={() => setAdapterFilter(null)}
          />
          {adapters.map((a) => (
            <FilterChip
              key={a}
              label={a}
              active={adapterFilter === a}
              onClick={() => setAdapterFilter(a)}
            />
          ))}
        </fieldset>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        {routes === undefined ? (
          // No `rowContentHeight`: `Td`'s 40px row floor already sizes the real
          // and the placeholder rows alike (measured 40.00px in both states at
          // 1440px), so the default content box is the matching one.
          <SkeletonRows
            columns={6}
            head={<RoutesTableHead />}
            label="Loading routes…"
            testid="network-routes-loading"
          />
        ) : filtered && filtered.length > 0 ? (
          <RoutesTable routes={filtered} />
        ) : (
          <EmptyState
            title="No matching routes"
            body={
              routes.length === 0
                ? "Adapters register HTTP routes here as they start."
                : "Clear the filter or pick a different adapter."
            }
            testid="network-routes-empty"
          />
        )}
      </div>
    </section>
  );
}

function FilterChip({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      data-testid={`network-adapter-${label}`}
      className={cn(
        "rounded-xs border px-2 py-0.5 text-xs font-medium",
        active
          ? "border-border-3 bg-bg-panel text-text-1"
          : "border-border-2 text-text-3 hover:bg-bg-panel hover:text-text-1",
      )}
    >
      {label}
    </button>
  );
}

function RoutesTableHead() {
  return (
    <thead className="sticky top-0 bg-bg-raised text-xs font-medium text-text-3">
      <tr>
        <Th>Method</Th>
        <Th>Path</Th>
        <Th>Adapter</Th>
        <Th>Handler</Th>
        <Th>Auth</Th>
        <Th>Last request</Th>
      </tr>
    </thead>
  );
}

/**
 * Tracks whether a horizontal scroller still has content to the right, so the
 * panel can show a real overflow cue. Clipped text at a hard panel border is
 * indistinguishable from corrupted data; the fade says "there is more", and the
 * per-column `max-w-*ch` truncation says "this value was cut".
 */
function useHorizontalOverflow<T extends HTMLElement>() {
  const [overflowing, setOverflowing] = useState(false);
  const cleanup = useRef<(() => void) | null>(null);
  const ref = useCallback((node: T | null) => {
    cleanup.current?.();
    cleanup.current = null;
    if (!node) return;
    const measure = () => {
      setOverflowing(node.scrollWidth - node.clientWidth - node.scrollLeft > 1);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    for (const child of Array.from(node.children)) observer.observe(child);
    node.addEventListener("scroll", measure, { passive: true });
    cleanup.current = () => {
      observer.disconnect();
      node.removeEventListener("scroll", measure);
    };
  }, []);
  useEffect(() => () => cleanup.current?.(), []);
  return { ref, overflowing };
}

function RoutesTable({ routes }: { routes: RouteDoc[] }) {
  const { ref, overflowing } = useHorizontalOverflow<HTMLDivElement>();
  return (
    <div className="relative h-full">
      <ScrollRegion ref={ref} label="Routes" className="h-full">
        <table
          className="w-full border-collapse text-base"
          data-testid="network-routes-table"
          data-overflowing={overflowing ? "true" : "false"}
        >
          <RoutesTableHead />
          <tbody>
            {routes.map((route) => {
              const method = (route.method ?? "").toUpperCase();
              const tone = METHOD_TONE[method] ?? "text-text-1";
              return (
                <tr
                  key={route._id}
                  className="border-t border-border-2 hover:bg-bg-raised"
                  data-testid={`network-route-${method}-${route.path ?? route._id}`}
                >
                  <Td className="whitespace-nowrap">
                    <span className={cn("font-medium", tone)}>
                      {method || "—"}
                    </span>
                  </Td>
                  <Td>
                    {/* `block` is required: `truncate` is inert on an inline
                      span. PATH is the column to sacrifice — it is the widest
                      and the `title` keeps the full value recoverable. 42ch is
                      what lands the whole table inside the panel at 1440px;
                      the fade covers the narrower viewports. */}
                    <span
                      className="block max-w-[42ch] truncate font-mono text-text-1"
                      title={route.path ?? undefined}
                    >
                      {route.path ?? "—"}
                    </span>
                  </Td>
                  <Td className="whitespace-nowrap">
                    <span className="font-mono text-text-1">
                      {route.adapter ?? "—"}
                    </span>
                  </Td>
                  <Td>
                    <span
                      className="block max-w-[28ch] truncate font-mono text-text-3"
                      title={route.handler ?? undefined}
                    >
                      {route.handler ?? "—"}
                    </span>
                  </Td>
                  <Td className="whitespace-nowrap">
                    <span className="font-medium text-text-3">
                      {route.authRequired ? "required" : "public"}
                    </span>
                  </Td>
                  <Td className="whitespace-nowrap">
                    {typeof route.lastRequestAt === "number" ? (
                      <RelativeTime epochMs={route.lastRequestAt} />
                    ) : (
                      <span className="tabular text-text-3">never</span>
                    )}
                  </Td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </ScrollRegion>
      {overflowing ? (
        <div
          aria-hidden
          data-testid="network-routes-overflow-cue"
          className="pointer-events-none absolute inset-y-0 right-0 w-10 bg-gradient-to-l from-bg-panel to-transparent"
        />
      ) : null}
    </div>
  );
}
