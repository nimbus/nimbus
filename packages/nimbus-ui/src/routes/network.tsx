import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useMemo, useState } from "react";

import { api } from "../../convex/_generated/api";
import { RelativeTime } from "../components/time";
import { cn } from "../lib/cn";

export const Route = createFileRoute("/network")({
  component: NetworkPage,
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
  DELETE: "text-danger",
  OPTIONS: "text-muted",
  HEAD: "text-muted",
};

function NetworkPage() {
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
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Network
          </h1>
          <p className="text-sm text-muted">
            HTTP routes, listeners, and published ports. Routes are sourced from
            the live registry — adapters appear as they register.
          </p>
        </div>
        <div
          className="font-mono text-xs text-muted"
          data-testid="network-total"
        >
          {routes === undefined
            ? "loading…"
            : `${filtered?.length ?? 0} of ${routes.length} routes`}
        </div>
      </header>

      <div
        className="flex flex-wrap items-center gap-2 rounded-md border border-app bg-surface-2 px-3 py-2"
        data-testid="network-filters"
      >
        <label className="flex items-center gap-2">
          <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
            filter
          </span>
          <input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            data-inline-search
            placeholder="method, path, handler"
            data-testid="network-filter-input"
            className="w-72 rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default placeholder:text-muted/70"
          />
        </label>
        <div
          className="flex items-center gap-1"
          role="tablist"
          aria-label="Filter by adapter"
        >
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
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {routes === undefined ? (
          <div className="flex h-32 items-center justify-center text-xs text-muted">
            Loading routes…
          </div>
        ) : filtered && filtered.length > 0 ? (
          <RoutesTable routes={filtered} />
        ) : (
          <div className="flex h-32 flex-col items-center justify-center gap-1 text-center">
            <span className="font-mono text-sm text-default">
              No matching routes
            </span>
            <span className="max-w-md text-xs text-muted">
              {routes.length === 0
                ? "Adapters register HTTP routes here as they start."
                : "Clear the filter or pick a different adapter."}
            </span>
          </div>
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
      role="tab"
      aria-selected={active}
      onClick={onClick}
      data-testid={`network-adapter-${label}`}
      className={cn(
        "rounded border px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide",
        active
          ? "border-strong bg-surface text-default"
          : "border-app text-muted hover:bg-surface hover:text-default",
      )}
    >
      {label}
    </button>
  );
}

function RoutesTable({ routes }: { routes: RouteDoc[] }) {
  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="network-routes-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Method</Th>
            <Th>Path</Th>
            <Th>Adapter</Th>
            <Th>Handler</Th>
            <Th>Auth</Th>
            <Th>Last request</Th>
          </tr>
        </thead>
        <tbody>
          {routes.map((route) => {
            const method = (route.method ?? "").toUpperCase();
            const tone = METHOD_TONE[method] ?? "text-default";
            return (
              <tr
                key={route._id}
                className="border-t border-app hover:bg-surface-2"
                data-testid={`network-route-${method}-${route.path ?? route._id}`}
              >
                <Td>
                  <span
                    className={cn(
                      "font-mono text-[11px] uppercase tracking-wide",
                      tone,
                    )}
                  >
                    {method || "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-default">
                    {route.path ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-default">
                    {route.adapter ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-xs text-muted">
                    {route.handler ?? "—"}
                  </span>
                </Td>
                <Td>
                  <span className="font-mono text-[11px] uppercase tracking-wide text-muted">
                    {route.authRequired ? "required" : "public"}
                  </span>
                </Td>
                <Td>
                  {typeof route.lastRequestAt === "number" ? (
                    <RelativeTime epochMs={route.lastRequestAt} />
                  ) : (
                    <span className="tabular text-muted">never</span>
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
