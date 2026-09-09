import { useNavigate } from "@tanstack/react-router";
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
import { cn } from "@/lib/utils";
import { useTenantList } from "../hooks/use-tenant-list";
import { useUiStore } from "../store/ui-store";

export type TenantSelectorMode =
  | { kind: "developer" }
  // `unavailable` keeps the affordance on screen while the backend cannot honor
  // the filter (see EVENTS_TABLE_HAS_TENANT_COLUMN). Removing the control
  // instead would strand a bookmarked `?tenant=` with nothing to clear it.
  | {
      kind: "operator-filter";
      currentFilter: string | null;
      unavailable: boolean;
    };

export function TenantSelector({ mode }: { mode: TenantSelectorMode }) {
  const activeTenant = useUiStore((s) => s.activeTenant);
  const setActiveTenant = useUiStore((s) => s.setActiveTenant);
  const state = useTenantList();
  const [open, setOpen] = useState(false);
  const [focusIndex, setFocusIndex] = useState(0);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const menuId = useId();
  const navigate = useNavigate();

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

  // Focusing the menu is what routes ArrowUp/Down/Home/End/Enter/Escape to
  // `onKeyDown` below: the menu is a sibling of the trigger, not an ancestor,
  // so without this the keys keep going to the button and the whole menu is
  // keyboard-dead.
  useEffect(() => {
    if (open) {
      setFocusIndex(currentIndex >= 0 ? currentIndex : 0);
      menuRef.current?.focus();
    }
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
          to: "/operator/observability",
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

  // Opening from the keyboard must preventDefault: without it the browser
  // synthesizes a click on keyup, which re-runs the trigger's onClick toggle
  // and closes the menu the moment it opened.
  const onTriggerKeyDown = useCallback(
    (e: KeyboardEvent<HTMLButtonElement>) => {
      if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        setOpen(true);
      }
    },
    [],
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
          navigate({ to: "/operator/tenants", search: { create: 1 } })
        }
        className="flex h-7 w-full items-center gap-1 rounded-md border border-border-2 bg-bg-panel px-2 text-xs font-medium text-text-3 hover:bg-bg-raised hover:text-text-1"
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

  // The backend cannot honor the filter yet, so the control stays on screen but
  // cannot open and cannot write `?tenant=` — the same "coming soon" treatment
  // the observability page gives its own unbuilt tabs.
  if (mode.kind === "operator-filter" && mode.unavailable) {
    return (
      <div className="relative" data-testid="tenant-selector">
        <button
          ref={buttonRef}
          type="button"
          disabled
          aria-disabled="true"
          data-testid="tenant-selector-trigger"
          data-mode={mode.kind}
          data-unavailable="true"
          title="Tenant filtering is unavailable until the events table exposes a tenant column"
          className="flex h-7 w-full min-w-0 cursor-not-allowed items-center gap-2 rounded-md border border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1 opacity-60"
        >
          <span className="text-xs font-medium text-text-3">Filter</span>
          <span className="truncate">All tenants</span>
          <span
            aria-hidden
            className="rounded-xs bg-bg-raised px-1 text-xs font-medium text-text-3"
            data-testid="tenant-selector-coming-soon"
          >
            coming soon
          </span>
        </button>
      </div>
    );
  }

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
        onKeyDown={onTriggerKeyDown}
        className={cn(
          "flex h-7 w-full min-w-0 items-center gap-2 rounded-md border border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1",
          "hover:bg-bg-raised",
          open && "bg-bg-raised",
        )}
      >
        <span className="text-xs font-medium text-text-3">
          {mode.kind === "operator-filter" ? "Filter" : "Tenant"}
        </span>
        <span className="flex-1 truncate text-left">{triggerLabel}</span>
        <ChevronDown
          size={12}
          aria-hidden
          className={cn(
            "shrink-0 text-text-3 transition-transform",
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
          // Guarded on entries: the loading, error and empty branches below
          // render no options at all, and pointing at a nonexistent id would
          // leave assistive tech announcing a row that is not there.
          aria-activedescendant={
            entries.length > 0 ? `${menuId}-opt-${focusIndex}` : undefined
          }
          data-testid="tenant-selector-menu"
          onKeyDown={onKeyDown}
          className="absolute left-0 right-0 top-full z-10 mt-1 max-h-72 overflow-auto rounded-md border border-border-2 bg-bg-panel shadow-lg focus:outline-none"
        >
          {state.kind === "loading" ? (
            <p
              className="px-3 py-2 font-mono text-xs text-text-3"
              data-testid="tenant-selector-loading"
            >
              loading…
            </p>
          ) : state.kind === "error" ? (
            <p
              className="px-3 py-2 font-mono text-xs text-error"
              data-testid="tenant-selector-error"
            >
              {state.message}
            </p>
          ) : entries.length === 0 ? (
            <p
              className="px-3 py-2 font-mono text-xs text-text-3"
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
                      id={`${menuId}-opt-${idx}`}
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
                          ? "bg-bg-raised text-text-1"
                          : "text-text-3 hover:bg-bg-raised hover:text-text-1",
                        isActive && "text-text-1",
                      )}
                    >
                      <span className="flex-1 truncate">{entry.label}</span>
                      {entry.backend ? (
                        <span className="rounded-xs border border-border-2 px-1 py-px text-xs font-medium text-text-3">
                          {entry.backend}
                        </span>
                      ) : null}
                      {isActive ? (
                        // Matches `Select`: the marker takes the active row's
                        // tone. `text-accent` measured 2.48:1 on --surface in
                        // the warm palette, failing at 11px.
                        <span
                          aria-hidden
                          className="font-mono text-xs text-text-1"
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
    </div>
  );
}
