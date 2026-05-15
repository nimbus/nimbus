import { useQuery } from "nimbus/react";
import { useEffect, useRef } from "react";
import { useRouterState } from "@tanstack/react-router";
import { X } from "lucide-react";

import { api } from "../../convex/_generated/api";
import { useUiStore } from "../store/ui-store";
import { Kbd } from "../components/kbd";
import { metaGlyph } from "../lib/platform";

export function SystemTenantLens() {
  const open = useUiStore((s) => s.lensOpen);
  const setLensOpen = useUiStore((s) => s.setLensOpen);
  const routerState = useRouterState();
  const pathname = routerState.location.pathname;
  const panelRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (open) {
      panelRef.current?.focus();
    }
  }, [open]);

  if (!open) return null;

  const view = resolveLensView(pathname);

  return (
    <aside
      ref={(node) => {
        panelRef.current = node;
      }}
      tabIndex={-1}
      role="region"
      aria-label="System tenant lens"
      data-testid="system-tenant-lens"
      className="fixed inset-y-0 right-0 z-40 flex w-[min(560px,50vw)] flex-col border-l shadow-2xl bg-surface border-app animate-in slide-in-from-right-4 duration-150"
    >
      <header className="flex items-center justify-between border-b border-app px-3 py-2">
        <div>
          <div className="text-xs uppercase tracking-wider text-muted">
            System tenant lens
          </div>
          <div className="font-mono text-sm text-default">
            _nimbus · {view.label}
          </div>
        </div>
        <button
          type="button"
          aria-label="Close lens"
          onClick={() => setLensOpen(false)}
          className="rounded p-1 text-muted hover:bg-surface-2 hover:text-default"
          data-testid="lens-close"
        >
          <X size={16} aria-hidden />
        </button>
      </header>
      <LensBody view={view} />
      <footer className="flex items-center gap-2 border-t border-app px-3 py-1.5 text-xs text-muted">
        <Kbd>{metaGlyph}</Kbd>
        <Kbd>\</Kbd>
        <span>toggle</span>
        <span className="ml-auto">read-only</span>
      </footer>
    </aside>
  );
}

type LensView =
  | { kind: "machines"; label: string }
  | { kind: "listeners"; label: string }
  | { kind: "system"; label: string }
  | { kind: "tables"; label: string }
  | { kind: "routes"; label: string }
  | { kind: "runs"; label: string }
  | { kind: "functions"; label: string };

function resolveLensView(pathname: string): LensView {
  if (pathname.startsWith("/machines")) return { kind: "machines", label: "machines" };
  if (pathname.startsWith("/network")) return { kind: "listeners", label: "listeners" };
  if (pathname.startsWith("/storage")) return { kind: "tables", label: "tables" };
  if (pathname.startsWith("/compute")) return { kind: "functions", label: "functions" };
  if (pathname.startsWith("/observability")) return { kind: "runs", label: "runs" };
  return { kind: "system", label: "system.status" };
}

function LensBody({ view }: { view: LensView }) {
  const docs = useLensDocuments(view);
  return (
    <div className="flex-1 overflow-auto px-3 py-2">
      <pre
        className="font-mono text-xs leading-relaxed text-default"
        data-testid="lens-json"
      >
        {docs === undefined
          ? "Loading…"
          : docs === null
            ? `Not in _nimbus. View "${view.label}" has no system-tenant document.`
            : JSON.stringify(docs, null, 2)}
      </pre>
    </div>
  );
}

function useLensDocuments(view: LensView) {
  // Each branch must be called unconditionally to keep hook order stable.
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 50,
  });
  const listeners = useQuery(api.listeners.list, {
    adapter: null,
    state: null,
    limit: 50,
  });
  const status = useQuery(api.system.status, {});
  const tables = useQuery(api.tables.list, { tenantId: null, limit: 50 });
  const routes = useQuery(api.routes.list, { adapter: null, limit: 50 });
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: null,
    status: null,
    limit: 50,
  });
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 50,
  });

  switch (view.kind) {
    case "machines":
      return machines;
    case "listeners":
      return listeners;
    case "system":
      return status;
    case "tables":
      return tables;
    case "routes":
      return routes;
    case "runs":
      return runs;
    case "functions":
      return functions;
  }
}
