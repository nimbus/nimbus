import { Dialog } from "@base-ui/react/dialog";
import { Link, useRouterState } from "@tanstack/react-router";
import { ChevronsLeft, ChevronsRight, type LucideIcon } from "lucide-react";
import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/ui-store";
import { useViewportTier, type ViewportTier } from "./use-viewport-tier";

// Background double-click toggles collapse; ignore double-clicks that land on an
// interactive element (links, buttons, inputs) so they keep their own behavior.
function isInteractiveTarget(target: EventTarget | null): boolean {
  return Boolean(
    (target as HTMLElement | null)?.closest("a,button,input,textarea,select"),
  );
}

export type SubDrawerItem<TId extends string = string> = {
  readonly id: TId;
  readonly label: string;
  readonly to: string;
  readonly search?: Record<string, unknown>;
  readonly description?: string;
  readonly disabled?: boolean;
  readonly count?: number | null;
};

// Items shown in the collapsed rail: each sub-view as an icon with the active
// one indicated. Clicking one switches sub-view without expanding the drawer.
export type SubDrawerRailItem = {
  readonly id: string;
  readonly label: string;
  readonly icon: LucideIcon;
  readonly active: boolean;
  readonly onSelect: () => void;
};

export type StaticSubDrawerSpec<TId extends string = string> = {
  readonly kind: "static";
  readonly title: string;
  readonly items: ReadonlyArray<SubDrawerItem<TId>>;
  readonly railItems?: ReadonlyArray<SubDrawerRailItem>;
};

export type DynamicSubDrawerSpec = {
  readonly kind: "dynamic";
  readonly title: string;
  readonly search?: { placeholder: string };
  readonly children: ReactNode;
  readonly railItems?: ReadonlyArray<SubDrawerRailItem>;
};

export type SubDrawerSpec<TId extends string = string> =
  | StaticSubDrawerSpec<TId>
  | DynamicSubDrawerSpec;

type SubDrawerContextValue = {
  spec: SubDrawerSpec | null;
  setSpec: (spec: SubDrawerSpec | null) => void;
  search: string;
  setSearch: (next: string) => void;
};

const SubDrawerContext = createContext<SubDrawerContextValue | null>(null);

export function SubDrawerProvider({ children }: { children: ReactNode }) {
  const [spec, setSpec] = useState<SubDrawerSpec | null>(null);
  const [search, setSearch] = useState<string>("");
  const value = useMemo(
    () => ({ spec, setSpec, search, setSearch }),
    [spec, search],
  );
  return (
    <SubDrawerContext.Provider value={value}>
      {children}
    </SubDrawerContext.Provider>
  );
}

export function useSubDrawerSearch(): string {
  const ctx = useContext(SubDrawerContext);
  if (!ctx) {
    throw new Error("useSubDrawerSearch must be used within SubDrawerProvider");
  }
  return ctx.search;
}

export function useContributeSubDrawer(spec: SubDrawerSpec | null) {
  const ctx = useContext(SubDrawerContext);
  if (!ctx) {
    throw new Error(
      "useContributeSubDrawer must be used within a SubDrawerProvider",
    );
  }
  const { setSpec, setSearch } = ctx;
  useEffect(() => {
    setSpec(spec);
    setSearch("");
    return () => {
      setSpec(null);
      setSearch("");
    };
  }, [spec, setSpec, setSearch]);
}

export function SubDrawer() {
  const ctx = useContext(SubDrawerContext);
  const { open, overlay, setOpen, toggle } = useSubDrawerDisplay();
  const spec = ctx?.spec ?? null;
  const search = ctx?.search ?? "";
  const setSearch = ctx?.setSearch ?? (() => {});
  if (!spec) return null;

  if (!open || overlay) {
    return (
      <>
        <SubDrawerRail
          spec={spec}
          onExpand={() => setOpen(true)}
          onToggle={toggle}
        />
        {overlay && open ? (
          <Dialog.Root open onOpenChange={(next) => setOpen(next)} modal>
            <Dialog.Portal>
              <Dialog.Backdrop
                data-testid="sub-drawer-scrim"
                className="fixed inset-0 z-40 bg-black/50"
              />
              <Dialog.Popup
                aria-label={spec.title}
                data-testid="sub-drawer-overlay"
                data-kind={spec.kind}
                className="fixed bottom-[var(--statusbar-height)] left-8 top-10 z-50 flex w-64 flex-col border-r border-app bg-surface shadow-lg outline-none"
              >
                <SubDrawerPanelBody
                  spec={spec}
                  search={search}
                  setSearch={setSearch}
                  onCollapse={() => setOpen(false)}
                />
              </Dialog.Popup>
            </Dialog.Portal>
          </Dialog.Root>
        ) : null}
      </>
    );
  }

  return (
    <aside
      aria-label={spec.title}
      data-testid="sub-drawer"
      data-kind={spec.kind}
      data-collapsed="false"
      onDoubleClick={(e) => {
        if (!isInteractiveTarget(e.target)) toggle();
      }}
      className="flex h-full w-64 shrink-0 flex-col border-r border-app bg-surface"
    >
      <SubDrawerPanelBody
        spec={spec}
        search={search}
        setSearch={setSearch}
        onCollapse={() => setOpen(false)}
      />
    </aside>
  );
}

// Below the desktop tier the expanded panel is an overlay sheet rather than a
// second in-flow column, so the content area keeps its width. The rail stays in
// flow underneath it: an overlay whose only dismissal is the scrim would leave
// no visible way back once closed.
//
// The sheet is modal, so entering the tier must not open it — a stored desktop
// preference would otherwise drop a scrim over the content on arrival. Below
// desktop the panel starts closed and only an explicit expand opens it, and
// that choice is tagged with the tier it was made in so returning to desktop
// restores the operator's real preference untouched.
function useSubDrawerDisplay(): {
  open: boolean;
  overlay: boolean;
  setOpen: (next: boolean) => void;
  toggle: () => void;
} {
  const tier = useViewportTier();
  const stored = useUiStore((s) => s.subDrawerOpen);
  const setSubDrawerOpen = useUiStore((s) => s.setSubDrawerOpen);
  const toggleSubDrawer = useUiStore((s) => s.toggleSubDrawer);
  const [override, setOverride] = useState<{
    tier: ViewportTier;
    value: boolean;
  } | null>(null);
  const overlay = tier !== "desktop";
  const active = override?.tier === tier ? override.value : null;
  const open = overlay ? (active ?? false) : stored;
  const setOpen = (next: boolean) => {
    if (overlay) {
      setOverride({ tier, value: next });
      return;
    }
    setOverride(null);
    setSubDrawerOpen(next);
  };
  return {
    open,
    overlay,
    setOpen,
    toggle: () => {
      if (overlay) {
        setOverride({ tier, value: !open });
        return;
      }
      setOverride(null);
      toggleSubDrawer();
    },
  };
}

// Collapsed: a thin rail with an expand toggle (mirrors the primary drawer).
// The panel is never fully removed, so it is always reachable again.
function SubDrawerRail({
  spec,
  onExpand,
  onToggle,
}: {
  spec: SubDrawerSpec;
  onExpand: () => void;
  onToggle: () => void;
}) {
  return (
    <aside
      aria-label={spec.title}
      data-testid="sub-drawer"
      data-kind={spec.kind}
      data-collapsed="true"
      onDoubleClick={(e) => {
        if (!isInteractiveTarget(e.target)) onToggle();
      }}
      className="flex h-full w-8 shrink-0 flex-col gap-1 border-r border-app bg-surface py-2"
    >
      <button
        type="button"
        onClick={onExpand}
        aria-label="Expand sub-drawer"
        title="Expand sub-drawer"
        data-testid="sub-drawer-toggle"
        className="flex h-7 w-full items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-default"
      >
        <ChevronsRight size={12} aria-hidden />
      </button>
      {spec.railItems?.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            type="button"
            onClick={item.onSelect}
            aria-label={item.label}
            aria-current={item.active ? "page" : undefined}
            title={item.label}
            data-testid={`sub-drawer-rail-item-${item.id}`}
            data-active={item.active ? "true" : "false"}
            className={cn(
              "flex h-8 w-full items-center justify-center border-l-2 border-transparent text-muted transition-colors hover:bg-surface-2 hover:text-default",
              item.active && "bg-surface-2 text-default",
            )}
            style={
              item.active
                ? { borderLeftColor: "var(--nimbus-brand)" }
                : undefined
            }
          >
            <Icon size={14} aria-hidden />
          </button>
        );
      })}
    </aside>
  );
}

function SubDrawerPanelBody({
  spec,
  search,
  setSearch,
  onCollapse,
}: {
  spec: SubDrawerSpec;
  search: string;
  setSearch: (next: string) => void;
  onCollapse: () => void;
}) {
  return (
    <>
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-app px-3">
        <span className="font-mono text-xs uppercase tracking-[0.18em] text-muted">
          {spec.title}
        </span>
        <button
          type="button"
          onClick={onCollapse}
          aria-label="Collapse sub-drawer"
          title="Collapse sub-drawer"
          data-testid="sub-drawer-toggle"
          className="flex h-6 w-6 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-default"
        >
          <ChevronsLeft size={12} aria-hidden />
        </button>
      </header>
      {spec.kind === "dynamic" && spec.search ? (
        <div className="border-b border-app px-3 py-2">
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={spec.search.placeholder}
            data-testid="sub-drawer-search"
            // `drawer` marks this as the fallback target for the `/` shortcut;
            // a page-level filter claims `primary` and wins. No focus styling
            // of its own — `--brand` is identity, `--accent` is focus, and the
            // global :focus-visible rule already paints the accent ring.
            data-inline-search="drawer"
            className="h-7 w-full rounded-md border border-app bg-canvas px-2 text-xs text-default placeholder:text-muted"
          />
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-auto">
        {spec.kind === "static" ? (
          <SubDrawerStaticList items={spec.items} />
        ) : (
          spec.children
        )}
      </div>
    </>
  );
}

function isItemActive(
  location: { pathname: string; search?: Record<string, unknown> },
  item: SubDrawerItem<string>,
): boolean {
  const pathMatches =
    location.pathname === item.to ||
    location.pathname.startsWith(`${item.to}/`);
  if (!pathMatches) return false;
  if (!item.search) return true;
  const current = location.search ?? {};
  for (const [key, value] of Object.entries(item.search)) {
    if (current[key] !== value) return false;
  }
  return true;
}

function SubDrawerStaticList({
  items,
}: {
  items: ReadonlyArray<SubDrawerItem<string>>;
}) {
  const location = useRouterState({
    select: (s) => ({
      pathname: s.location.pathname,
      search: s.location.search as Record<string, unknown> | undefined,
    }),
  });
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {items.map((item) => {
        const active = isItemActive(location, item);
        const row = cn(
          "flex h-8 items-center gap-2 rounded-md border-l-2 border-transparent px-2 text-sm",
          item.disabled
            ? "text-muted"
            : active
              ? "bg-surface-2 text-default"
              : "text-muted hover:bg-surface-2 hover:text-default",
        );
        const label = <span className="flex-1 truncate">{item.label}</span>;
        /* A sub-view that does not exist yet is not a link. Rendering it as
           one and blocking the mouse with `pointer-events-none` left it in the
           tab order and still activatable by Enter -- and because the view is
           unbuilt, the router then dropped the `tab` search param and landed
           the user on a different view than the one they chose. A span with
           `aria-disabled` cannot be reached or fired at all, and announces the
           state rather than only looking dim.

           Same treatment as `DisabledTab` on the observability tab strip and
           the operator tenant filter: the chip carries the state, not opacity.
           `opacity-60` composited over `--muted` measures 2.42:1 in warm
           light and 3.17:1 at its best, so in all six combinations the one
           label explaining what is coming was the hardest text in the drawer
           to read. */
        if (item.disabled) {
          return (
            <li key={item.id}>
              <span
                aria-disabled="true"
                data-testid={`sub-drawer-item-${item.id}`}
                data-active="false"
                title={`${item.label} — coming soon`}
                className={cn(row, "cursor-not-allowed")}
              >
                {label}
                <span
                  aria-hidden
                  className="inline-flex items-center rounded border border-app bg-surface-2 px-1.5 py-0.5 font-mono text-xs leading-none uppercase tracking-wide text-muted"
                  data-testid={`sub-drawer-item-${item.id}-coming-soon`}
                >
                  coming soon
                </span>
              </span>
            </li>
          );
        }
        return (
          <li key={item.id}>
            <Link
              to={item.to}
              search={item.search ?? undefined}
              aria-current={active ? "page" : undefined}
              data-testid={`sub-drawer-item-${item.id}`}
              data-active={active ? "true" : "false"}
              className={row}
              style={
                active ? { borderLeftColor: "var(--nimbus-brand)" } : undefined
              }
            >
              {label}
              {typeof item.count === "number" ? (
                <span className="tabular font-mono text-xs text-muted">
                  {item.count}
                </span>
              ) : null}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
