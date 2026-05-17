import { useNavigate, useRouterState } from "@tanstack/react-router";
import { ChevronDown, Plus } from "lucide-react";
import {
  type KeyboardEvent,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { cn } from "../lib/cn";
import { useUiStore } from "../store/ui-store";

type TenantListResponse = {
  tenants?: Array<
    string | { id?: string; tenantId?: string; name?: string; backend?: string }
  >;
};

type TenantEntry = {
  id: string;
  backend?: string;
};

type TenantsState =
  | { kind: "loading" }
  | { kind: "loaded"; tenants: TenantEntry[] }
  | { kind: "error"; message: string };

export type TenantSelectorMode =
  | { kind: "developer" }
  | { kind: "operator-filter"; currentFilter: string | null };

async function loadTenants(signal: AbortSignal): Promise<TenantEntry[]> {
  const response = await fetch("/api/tenants", {
    credentials: "include",
    signal,
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      error?: { message?: string };
    } | null;
    throw new Error(
      body?.error?.message ?? `Request failed: ${response.status}`,
    );
  }
  const body = (await response.json()) as TenantListResponse;
  return (body.tenants ?? [])
    .map<TenantEntry | null>((entry) => {
      if (typeof entry === "string") return { id: entry };
      const id = entry.tenantId ?? entry.id ?? entry.name;
      if (!id) return null;
      return { id, backend: entry.backend };
    })
    .filter((entry): entry is TenantEntry => entry !== null)
    .sort((a, b) => a.id.localeCompare(b.id));
}

export function TenantSelector({ mode }: { mode: TenantSelectorMode }) {
  const activeTenant = useUiStore((s) => s.activeTenant);
  const setActiveTenant = useUiStore((s) => s.setActiveTenant);
  const [state, setState] = useState<TenantsState>({ kind: "loading" });
  const [open, setOpen] = useState(false);
  const [focusIndex, setFocusIndex] = useState(0);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const menuId = useId();
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });

  useEffect(() => {
    const controller = new AbortController();
    setState({ kind: "loading" });
    loadTenants(controller.signal)
      .then((tenants) => {
        if (controller.signal.aborted) return;
        setState({ kind: "loaded", tenants });
      })
      .catch((err) => {
        if (controller.signal.aborted) return;
        setState({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      });
    return () => controller.abort();
  }, []);

  const tenants = state.kind === "loaded" ? state.tenants : [];
  const entries = useMemo<
    Array<{ id: string | null; label: string; backend?: string }>
  >(() => {
    if (mode.kind === "operator-filter") {
      return [
        { id: null, label: "All tenants" },
        ...tenants.map((t) => ({ id: t.id, label: t.id, backend: t.backend })),
      ];
    }
    return tenants.map((t) => ({ id: t.id, label: t.id, backend: t.backend }));
  }, [mode.kind, tenants]);

  const currentValue =
    mode.kind === "operator-filter" ? mode.currentFilter : activeTenant;

  const currentIndex = entries.findIndex((e) => e.id === currentValue);

  useEffect(() => {
    if (open) setFocusIndex(currentIndex >= 0 ? currentIndex : 0);
  }, [open, currentIndex]);

  useEffect(() => {
    if (!open) return;
    const onClickOutside = (event: MouseEvent) => {
      if (
        menuRef.current &&
        !menuRef.current.contains(event.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(event.target as Node)
      ) {
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onClickOutside);
    return () => window.removeEventListener("mousedown", onClickOutside);
  }, [open]);

  const applySelection = useCallback(
    (next: string | null) => {
      if (mode.kind === "operator-filter") {
        navigate({
          to: "/admin/observability",
          search: next ? { tenant: next } : {},
          replace: true,
        });
      } else {
        setActiveTenant(next);
      }
      setOpen(false);
      queueMicrotask(() => buttonRef.current?.focus());
    },
    [mode.kind, navigate, setActiveTenant],
  );

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      if (entries.length === 0) {
        if (e.key === "Escape") setOpen(false);
        return;
      }
      switch (e.key) {
        case "ArrowDown":
          e.preventDefault();
          setFocusIndex((i) => (i + 1) % entries.length);
          break;
        case "ArrowUp":
          e.preventDefault();
          setFocusIndex((i) => (i - 1 + entries.length) % entries.length);
          break;
        case "Home":
          e.preventDefault();
          setFocusIndex(0);
          break;
        case "End":
          e.preventDefault();
          setFocusIndex(entries.length - 1);
          break;
        case "Enter":
        case " ":
          e.preventDefault();
          applySelection(entries[focusIndex]?.id ?? null);
          break;
        case "Escape":
          e.preventDefault();
          setOpen(false);
          queueMicrotask(() => buttonRef.current?.focus());
          break;
      }
    },
    [applySelection, entries, focusIndex],
  );

  if (
    mode.kind === "developer" &&
    state.kind === "loaded" &&
    tenants.length === 0
  ) {
    return (
      <button
        type="button"
        data-testid="tenant-selector-create"
        onClick={() =>
          navigate({ to: "/admin/tenants", search: { create: 1 } as never })
        }
        className="flex h-7 items-center gap-1 rounded-md border border-app bg-surface px-2 font-mono text-[10px] uppercase tracking-[0.18em] text-muted hover:bg-surface-2 hover:text-default"
      >
        <Plus size={12} aria-hidden />
        Create tenant
      </button>
    );
  }

  const triggerLabel =
    mode.kind === "operator-filter"
      ? (mode.currentFilter ?? "All tenants")
      : (activeTenant ?? "Select tenant");

  return (
    <div className="relative" data-testid="tenant-selector">
      <button
        ref={buttonRef}
        type="button"
        data-testid="tenant-selector-trigger"
        data-mode={mode.kind}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          "flex h-7 max-w-[14rem] items-center gap-2 rounded-md border border-app bg-surface px-2 font-mono text-[11px] text-default",
          "hover:bg-surface-2",
          open && "bg-surface-2",
        )}
      >
        <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-muted">
          {mode.kind === "operator-filter" ? "Filter" : "Tenant"}
        </span>
        <span className="truncate">{triggerLabel}</span>
        <ChevronDown
          size={12}
          aria-hidden
          className={cn(
            "shrink-0 text-muted transition-transform",
            open && "rotate-180",
          )}
        />
      </button>
      {open ? (
        <div
          ref={menuRef}
          id={menuId}
          role="listbox"
          tabIndex={-1}
          aria-label="Tenants"
          data-testid="tenant-selector-menu"
          onKeyDown={onKeyDown}
          className="absolute right-0 top-full z-10 mt-1 max-h-72 w-[16rem] overflow-auto rounded-md border border-app bg-surface shadow-lg focus:outline-none"
        >
          {state.kind === "loading" ? (
            <p
              className="px-3 py-2 font-mono text-[11px] text-muted"
              data-testid="tenant-selector-loading"
            >
              loading…
            </p>
          ) : state.kind === "error" ? (
            <p
              className="px-3 py-2 font-mono text-[11px] text-rose-400"
              data-testid="tenant-selector-error"
            >
              {state.message}
            </p>
          ) : entries.length === 0 ? (
            <p
              className="px-3 py-2 font-mono text-[11px] text-muted"
              data-testid="tenant-selector-empty"
            >
              No tenants yet.
            </p>
          ) : (
            <ul className="flex flex-col gap-px py-1">
              {entries.map((entry, idx) => {
                const isActive = entry.id === currentValue;
                const isFocused = idx === focusIndex;
                return (
                  <li key={entry.id ?? "__all__"}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={isActive}
                      data-testid={`tenant-selector-option-${entry.id ?? "all"}`}
                      data-active={isActive ? "true" : "false"}
                      data-focused={isFocused ? "true" : "false"}
                      onMouseEnter={() => setFocusIndex(idx)}
                      onClick={() => applySelection(entry.id)}
                      className={cn(
                        "flex h-8 w-full items-center justify-between gap-2 px-3 text-left font-mono text-xs",
                        isFocused
                          ? "bg-surface-2 text-default"
                          : "text-muted hover:bg-surface-2 hover:text-default",
                        isActive && "text-default",
                      )}
                    >
                      <span className="flex-1 truncate">{entry.label}</span>
                      {entry.backend ? (
                        <span className="rounded border border-app px-1 py-px text-[9px] uppercase tracking-wide text-muted">
                          {entry.backend}
                        </span>
                      ) : null}
                      {isActive ? (
                        <span
                          aria-hidden
                          className="font-mono text-[10px] text-brand"
                        >
                          ●
                        </span>
                      ) : null}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      ) : null}
      <span className="sr-only" aria-live="polite">
        {pathname}
      </span>
    </div>
  );
}
