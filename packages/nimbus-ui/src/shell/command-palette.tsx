import { useQuery } from "@nimbus/nimbus/react";
import { useNavigate, useRouter, useRouterState } from "@tanstack/react-router";
import type { LucideIcon } from "lucide-react";
import {
  ArrowLeftRight,
  Boxes,
  Building2,
  Command as CommandIcon,
  Cpu,
  Database,
  MonitorCog,
  Moon,
  Network,
  RotateCw,
  ScanEye,
  Sun,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import { api } from "../../convex/_generated/api";
import { useTenantList } from "../hooks/use-tenant-list";
import { metaGlyph } from "../lib/platform";
import { useUiStore } from "../store/ui-store";
import {
  DEVELOPER_NAV_ENTRIES,
  type NavEntry,
  type NavView,
  OPERATOR_NAV_ENTRIES,
  viewFromPathname,
} from "./nav-entries";
import { useViewSwitch } from "./view-switcher";

// The palette is one list in three groups: Routes are the console pages,
// Tenants are the scopes the developer view can stand in, Actions are the
// verbs that have no page of their own. Resources (tables, functions,
// services, machines, HTTP routes) join the list once the operator types, so
// the palette holds no server subscriptions while it only shows what it
// always shows. The footer is the one place the shell's keyboard contract is
// written down.

export const RECENT_KEY = "nimbus-ui:commands:recent";
const RECENT_LIMIT = 5;

// Resource kinds the palette can jump to, plus "section" for the console
// pages. The kind is persisted with each recent so a stored resource can be
// re-rendered (icon + label + target) without re-querying the server.
type TargetKind =
  | "section"
  | "table"
  | "function"
  | "tenant"
  | "machine"
  | "service"
  | "route";

const TARGET_ICONS: Record<TargetKind, LucideIcon> = {
  section: CommandIcon,
  table: Database,
  function: Cpu,
  tenant: Building2,
  machine: MonitorCog,
  service: Boxes,
  route: Network,
};

// One selectable jump target. `href` is a resolved path (never a route id with
// params) so a target read back out of localStorage stays navigable.
type PaletteTarget = {
  kind: TargetKind;
  key: string;
  label: string;
  detail?: string;
  href: string;
};

function sectionTarget(entry: NavEntry): PaletteTarget {
  return {
    kind: "section",
    key: `${entry.view}:${entry.id}`,
    label: entry.label,
    detail: entry.view === "developer" ? "Developer" : "Operator",
    href: entry.to,
  };
}

type PaletteAction = {
  id: string;
  label: string;
  icon: LucideIcon;
  keys?: string[];
  perform: () => void;
};

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const setLensOpen = useUiStore((s) => s.setLensOpen);
  const theme = useUiStore((s) => s.theme);
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  const navigate = useNavigate();
  const router = useRouter();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  const { switchTo } = useViewSwitch();
  const [search, setSearch] = useState("");
  const [recent, setRecent] = useState<PaletteTarget[]>(loadRecent);

  useEffect(() => {
    if (open) setSearch("");
  }, [open]);

  const otherView: NavView = view === "developer" ? "operator" : "developer";
  const actions: PaletteAction[] = [
    {
      id: "switch-view",
      label: `Switch to ${otherView === "developer" ? "Developer" : "Operator"} console`,
      icon: ArrowLeftRight,
      perform: () => {
        setOpen(false);
        switchTo(otherView);
      },
    },
    ...(view === "developer"
      ? [
          {
            id: "open-system-tenant-lens",
            label: "Open system tenant lens",
            icon: ScanEye,
            keys: [metaGlyph, "\\"],
            perform: () => {
              setOpen(false);
              queueMicrotask(() => setLensOpen(true));
            },
          },
        ]
      : []),
    // Refetches in place — the same `router.invalidate()` the retry buttons
    // and the per-page reload controls use. A `window.location.reload()`
    // here read as "refresh" but tore the SPA down: the socket dropped and
    // reconnected, and every piece of view state went with it.
    {
      id: "refresh-current-view",
      label: "Refresh current view",
      icon: RotateCw,
      perform: () => {
        setOpen(false);
        void router.invalidate();
      },
    },
    {
      id: "toggle-theme",
      label:
        theme === "dark" ? "Switch to light theme" : "Switch to dark theme",
      icon: theme === "dark" ? Sun : Moon,
      perform: () => {
        setOpen(false);
        setThemeMode(theme === "dark" ? "light" : "dark");
      },
    },
  ];

  // Jump to a target and push it onto the recents list. Recents hold the whole
  // target record (not a bare key), so a table or function selected here still
  // resolves after a reload — resolving keys against the nav list alone would
  // render every stored resource as `null` forever.
  const pickTarget = useCallback(
    (target: PaletteTarget) => {
      setRecent((current) => {
        const next = [
          target,
          ...current.filter((existing) => existing.key !== target.key),
        ].slice(0, RECENT_LIMIT);
        persistRecent(next);
        return next;
      });
      setOpen(false);
      navigate({ to: target.href });
    },
    [navigate, setOpen],
  );

  const query = search.trim();
  // Pages of the current view first: the palette opens on the console the
  // operator is already in, so its own pages are the likeliest jump.
  const routeEntries =
    view === "developer"
      ? [...DEVELOPER_NAV_ENTRIES, ...OPERATOR_NAV_ENTRIES]
      : [...OPERATOR_NAV_ENTRIES, ...DEVELOPER_NAV_ENTRIES];

  return (
    <CommandDialog
      open={open}
      onOpenChange={(next) => setOpen(next)}
      title="Command palette"
      description="Jump to a page, switch tenant, or run an action."
      className="top-[12vh] sm:max-w-[640px]"
    >
      <Command
        loop
        aria-label="Command palette"
        className="max-h-[min(70vh,560px)] bg-bg-raised"
        data-testid="command-palette"
      >
        <CommandInput
          value={search}
          onValueChange={setSearch}
          autoFocus
          placeholder="Jump to a page, tenant, table, function, service…"
          data-testid="command-palette-input"
        />
        <CommandList
          className="max-h-none min-h-0 flex-1"
          data-testid="command-palette-list"
        >
          <CommandEmpty className="text-text-3">No matches.</CommandEmpty>

          {recent.length > 0 && query === "" ? (
            <CommandGroup heading="Recent" data-testid="palette-group-recent">
              {recent.map((target) => (
                <PaletteItem
                  key={`recent-${target.key}`}
                  target={target}
                  onSelect={() => pickTarget(target)}
                />
              ))}
            </CommandGroup>
          ) : null}

          <CommandGroup heading="Routes" data-testid="palette-group-routes">
            {routeEntries.map((entry) => {
              const target = sectionTarget(entry);
              return (
                <PaletteItem
                  key={`nav-${target.key}`}
                  target={target}
                  icon={entry.icon}
                  onSelect={() => pickTarget(target)}
                />
              );
            })}
          </CommandGroup>

          {open ? <TenantGroup onPick={pickTarget} /> : null}

          <CommandGroup heading="Actions" data-testid="palette-group-actions">
            {actions.map((action) => (
              <CommandItem
                key={action.id}
                value={action.label}
                onSelect={action.perform}
                className={ITEM_CLASS}
                data-testid={`palette-action-${action.label}`}
              >
                <action.icon className="text-text-3" />
                <span className="flex-1 truncate">{action.label}</span>
                {action.keys ? <Keys keys={action.keys} /> : null}
              </CommandItem>
            ))}
          </CommandGroup>

          {/* Resource groups mount only once the operator types: the palette
              can hold several hundred rows, and rendering them all on open
              would cost more than it buys when nothing has been searched for
              yet. The queries live in PaletteResults, which is mounted only
              inside this branch, so the palette holds no subscriptions while
              it is closed or idle. */}
          {open && query !== "" ? <PaletteResults onPick={pickTarget} /> : null}
        </CommandList>
        <PaletteFooter view={view} />
      </Command>
    </CommandDialog>
  );
}

// The footer is where the shell's keyboard contract is written down. The
// three chords are the ones keyboard-contract.tsx binds; the `/` filter only
// exists on a page with an inline search, so it reads as a page shortcut.
function PaletteFooter({ view }: { view: NavView }) {
  return (
    <div
      className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-t border-border-1 px-3 py-2 text-xs text-text-3"
      data-testid="command-palette-footer"
    >
      <Hint keys={["↑", "↓"]} label="move" />
      <Hint keys={["⏎"]} label="open" />
      <Hint keys={["⎋"]} label="close" />
      <span className="ml-auto inline-flex flex-wrap items-center gap-x-4 gap-y-1">
        <Hint keys={[metaGlyph, "K"]} label="palette" />
        {view === "developer" ? (
          <Hint keys={[metaGlyph, "\\"]} label="tenant lens" />
        ) : null}
        <Hint keys={["/"]} label="filter page" />
      </span>
    </div>
  );
}

function Hint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <Keys keys={keys} />
      <span>{label}</span>
    </span>
  );
}

function Keys({ keys }: { keys: string[] }) {
  return (
    <KbdGroup className="shrink-0">
      {keys.map((key) => (
        <Kbd key={key}>{key}</Kbd>
      ))}
    </KbdGroup>
  );
}

// The tenant group is the developer scope: picking a tenant makes it the
// active tenant and lands on the developer console, where the scope applies.
// The list comes from the same `/api/tenants` read the sidebar selector
// already holds, so mounting it here adds no request.
function TenantGroup({ onPick }: { onPick: (t: PaletteTarget) => void }) {
  const tenantList = useTenantList();
  const activeTenant = useUiStore((s) => s.activeTenant);
  const setActiveTenant = useUiStore((s) => s.setActiveTenant);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const inDeveloper = viewFromPathname(pathname) === "developer";

  if (tenantList.kind === "error") {
    return (
      <CommandGroup
        heading="Tenants"
        data-testid="palette-group-tenants"
        forceMount
      >
        <UnavailableRow heading="Tenants" error={tenantList.message} />
      </CommandGroup>
    );
  }
  if (tenantList.kind !== "loaded" || tenantList.tenants.length === 0) {
    return null;
  }
  return (
    <CommandGroup heading="Tenants" data-testid="palette-group-tenants">
      {tenantList.tenants.map((entry) => {
        const active = entry.id === activeTenant;
        return (
          <CommandItem
            key={`tenant:${entry.id}`}
            value={`tenant ${entry.id} ${entry.backend ?? ""}`}
            onSelect={() => {
              setActiveTenant(entry.id);
              if (inDeveloper) {
                setOpen(false);
                return;
              }
              onPick({
                kind: "tenant",
                key: `tenant:${entry.id}`,
                label: entry.id,
                detail: entry.backend,
                href: "/developer",
              });
            }}
            className={ITEM_CLASS}
            data-checked={active ? "true" : undefined}
            data-testid={`palette-item-tenant:${entry.id}`}
          >
            <Building2 className="text-text-3" />
            <span className="flex-1 truncate font-mono">{entry.id}</span>
            {entry.backend ? <Detail>{entry.backend}</Detail> : null}
          </CommandItem>
        );
      })}
    </CommandGroup>
  );
}

type ResourceRow = {
  _id: string;
  name?: string;
  path?: string;
  method?: string;
  adapter?: string;
  kind?: string;
  state?: string;
  tenantId?: string;
};

// The resource half of the list. Kept in its own component so the five
// resource reads mount only while the operator is typing — hooks placed on
// CommandPalette itself would hold live subscriptions for the whole session.
function PaletteResults({ onPick }: { onPick: (t: PaletteTarget) => void }) {
  const tenant = useUiStore((s) => s.activeTenant);

  const tables = useQuery(
    api.tables.list,
    tenant ? { tenantId: tenant, limit: 200 } : "skip",
  ) as ResourceRow[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 200,
  }) as ResourceRow[] | undefined;
  const services = useQuery(api.services.list, {
    tenantId: null,
    machineId: null,
    state: null,
    limit: 200,
  }) as ResourceRow[] | undefined;
  const machines = useQuery(api.machines.list, {
    state: null,
    provider: null,
    limit: 200,
  }) as ResourceRow[] | undefined;
  const routes = useQuery(api.routes.list, {
    adapter: null,
    limit: 200,
  }) as ResourceRow[] | undefined;

  const tableTargets = useMemo<PaletteTarget[]>(
    () =>
      (tables ?? [])
        .filter((row) => Boolean(row.name))
        .map((row) => ({
          kind: "table" as const,
          key: `table:${row._id}`,
          label: row.name ?? row._id,
          detail: tenant ?? undefined,
          href: `/developer/storage/${encodeURIComponent(row.name ?? "")}`,
        })),
    [tables, tenant],
  );

  const functionTargets = useMemo<PaletteTarget[]>(
    () =>
      (functions ?? [])
        .filter((row) => Boolean(row.path))
        .map((row) => ({
          kind: "function" as const,
          key: `function:${row._id}`,
          label: row.path ?? row._id,
          detail: row.kind,
          href: `/developer/compute/${encodeURIComponent(row.path ?? "")}`,
        })),
    [functions],
  );

  const serviceTargets = useMemo<PaletteTarget[]>(
    () =>
      (services ?? []).map((row) => ({
        kind: "service" as const,
        key: `service:${row._id}`,
        label: row.name ?? row._id,
        detail: row.state,
        href: `/developer/services/${encodeURIComponent(row._id)}`,
      })),
    [services],
  );

  // machine-detail.tsx has no createFileRoute, so a machine has no detail URL
  // to target; these items land on the machines list.
  const machineTargets = useMemo<PaletteTarget[]>(
    () =>
      (machines ?? []).map((row) => ({
        kind: "machine" as const,
        key: `machine:${row._id}`,
        label: row.name ?? row._id,
        detail: row.state,
        href: "/operator/machines",
      })),
    [machines],
  );

  const routeTargets = useMemo<PaletteTarget[]>(
    () =>
      (routes ?? [])
        .filter((row) => Boolean(row.path))
        .map((row) => ({
          kind: "route" as const,
          key: `route:${row._id}`,
          label: `${row.method ?? "ANY"} ${row.path ?? ""}`,
          detail: row.adapter,
          href: "/operator/network",
        })),
    [routes],
  );

  return (
    <>
      <ResourceGroup heading="Tables" targets={tableTargets} onPick={onPick} />
      <ResourceGroup
        heading="Functions"
        targets={functionTargets}
        onPick={onPick}
      />
      <ResourceGroup
        heading="Services"
        targets={serviceTargets}
        onPick={onPick}
      />
      <ResourceGroup
        heading="Machines"
        targets={machineTargets}
        onPick={onPick}
      />
      <ResourceGroup
        heading="HTTP routes"
        targets={routeTargets}
        onPick={onPick}
      />
    </>
  );
}

function ResourceGroup({
  heading,
  targets,
  onPick,
}: {
  heading: string;
  targets: PaletteTarget[];
  onPick: (t: PaletteTarget) => void;
}) {
  if (targets.length === 0) return null;
  return (
    <CommandGroup heading={heading}>
      {targets.map((target) => (
        <PaletteItem
          key={target.key}
          target={target}
          onSelect={() => onPick(target)}
        />
      ))}
    </CommandGroup>
  );
}

// A status row, not a match candidate: forceMount on both parts so the filter
// cannot score it 0 against a tenant name and hide the one warning the
// operator most needs to see.
function UnavailableRow({
  heading,
  error,
}: {
  heading: string;
  error: string;
}) {
  return (
    <CommandItem
      disabled
      forceMount
      value={`${heading} unavailable`}
      className="text-text-3"
      data-testid={`palette-group-${heading.toLowerCase()}-error`}
    >
      {heading} unavailable — {error}
    </CommandItem>
  );
}

// Rows are the registry item at 36px with the console's selection wash. The
// selected row is what Enter runs, so it takes the hover surface plus a 2px
// accent bar — the ~5% surface step alone was indistinguishable from the
// empty background.
const ITEM_CLASS =
  "h-9 text-text-1 data-selected:bg-bg-hover data-selected:shadow-[inset_2px_0_0_var(--accent)]";

function Detail({ children }: { children: string }) {
  return (
    <span className="shrink-0 text-xs font-medium text-text-3">{children}</span>
  );
}

function PaletteItem({
  target,
  icon,
  onSelect,
}: {
  target: PaletteTarget;
  icon?: LucideIcon;
  onSelect: () => void;
}) {
  const Icon = icon ?? TARGET_ICONS[target.kind];
  return (
    <CommandItem
      // The key is part of the match value so two resources that share a name
      // stay distinct rows (cmdk keys selection by value), and so an operator
      // can match on the id as well as the name.
      value={[target.label, target.detail, target.key]
        .filter(Boolean)
        .join(" ")}
      onSelect={onSelect}
      className={ITEM_CLASS}
      data-testid={`palette-item-${target.key}`}
    >
      <Icon className="text-text-3" />
      <span
        className={
          target.kind === "section"
            ? "flex-1 truncate"
            : "flex-1 truncate font-mono"
        }
      >
        {target.label}
      </span>
      {target.detail ? <Detail>{target.detail}</Detail> : null}
    </CommandItem>
  );
}

function isPaletteTarget(value: unknown): value is PaletteTarget {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.key === "string" &&
    typeof candidate.label === "string" &&
    typeof candidate.href === "string" &&
    typeof candidate.kind === "string" &&
    candidate.kind in TARGET_ICONS
  );
}

function loadRecent(): PaletteTarget[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isPaletteTarget) : [];
  } catch {
    return [];
  }
}

function persistRecent(list: PaletteTarget[]) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* ignore */
  }
}
