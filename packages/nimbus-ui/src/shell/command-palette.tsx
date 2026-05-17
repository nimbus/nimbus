import { useNavigate } from "@tanstack/react-router";
import { Command } from "cmdk";
import {
  Command as CommandIcon,
  Filter,
  PlayCircle,
  RotateCw,
  Search,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Kbd } from "../components/kbd";
import { cn } from "../lib/cn";
import { metaGlyph } from "../lib/platform";
import { useUiStore } from "../store/ui-store";
import {
  DEVELOPER_NAV_ENTRIES,
  type NavEntry,
  OPERATOR_NAV_ENTRIES,
} from "./nav-entries";

type Mode = "navigate" | "run" | "filter";

const RECENT_KEY = "nimbus-ui:palette:recent";
const RECENT_LIMIT = 5;

function recentKey(entry: NavEntry): string {
  return `${entry.view}:${entry.id}`;
}

type RunAction = {
  id: string;
  label: string;
  hint?: string;
  perform: () => void;
};

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const setLensOpen = useUiStore((s) => s.setLensOpen);
  const navigate = useNavigate();
  const [mode, setMode] = useState<Mode>("navigate");
  const [search, setSearch] = useState("");
  const [recent, setRecent] = useState<string[]>(loadRecent);

  const allEntries = useMemo<NavEntry[]>(
    () => [...DEVELOPER_NAV_ENTRIES, ...OPERATOR_NAV_ENTRIES],
    [],
  );

  useEffect(() => {
    if (open) {
      setSearch("");
      setMode("navigate");
    }
  }, [open]);

  const runActions: RunAction[] = [
    {
      id: "open-system-tenant-lens",
      label: "Open system tenant lens",
      hint: `${metaGlyph} \\`,
      perform: () => {
        setOpen(false);
        queueMicrotask(() => setLensOpen(true));
      },
    },
    {
      id: "refresh-current-view",
      label: "Refresh current view",
      hint: `${metaGlyph} R`,
      perform: () => {
        setOpen(false);
        window.location.reload();
      },
    },
  ];

  function rememberAndClose(id: string, action: () => void) {
    setRecent((current) => {
      const next = [id, ...current.filter((existing) => existing !== id)].slice(
        0,
        RECENT_LIMIT,
      );
      persistRecent(next);
      return next;
    });
    action();
  }

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 backdrop-blur-[1px] pt-[12vh] animate-in fade-in-0">
      <button
        type="button"
        aria-label="Close command palette"
        className="absolute inset-0 cursor-default"
        onClick={() => setOpen(false)}
      />
      <Command
        loop
        role="dialog"
        aria-label="Command palette"
        className="relative w-[min(640px,90vw)] overflow-hidden rounded-lg border bg-surface shadow-2xl border-strong animate-in zoom-in-95 fade-in-0"
        data-testid="command-palette"
      >
        <div className="flex items-center gap-2 border-b border-app px-3 py-2">
          <Search size={14} className="text-muted" aria-hidden />
          <Command.Input
            value={search}
            onValueChange={setSearch}
            autoFocus
            placeholder={
              mode === "navigate"
                ? "Jump to a resource…"
                : mode === "run"
                  ? "Run an action…"
                  : "Filter the current view…"
            }
            className="h-7 flex-1 bg-transparent text-sm outline-none placeholder:text-muted text-default"
            data-testid="command-palette-input"
          />
          <ModeToggle current={mode} onChange={setMode} />
        </div>
        <Command.List
          className="max-h-[420px] overflow-auto px-1 py-1"
          data-testid="command-palette-list"
        >
          <Command.Empty className="px-3 py-6 text-center text-sm text-muted">
            No matches.
          </Command.Empty>

          {recent.length > 0 && search === "" ? (
            <Command.Group heading="Recent">
              {recent.map((key) => {
                const entry = allEntries.find((e) => recentKey(e) === key);
                if (!entry) return null;
                return (
                  <PaletteItem
                    key={`recent-${key}`}
                    icon={entry.icon}
                    label={`${entry.label} · ${entry.view}`}
                    hint={`${metaGlyph} ⏎`}
                    onSelect={() => {
                      rememberAndClose(key, () => {
                        setOpen(false);
                        navigate({ to: entry.to });
                      });
                    }}
                  />
                );
              })}
            </Command.Group>
          ) : null}

          {mode === "navigate" ? (
            <>
              <Command.Group heading="Developer console">
                {DEVELOPER_NAV_ENTRIES.map((entry) => (
                  <PaletteItem
                    key={`nav-${recentKey(entry)}`}
                    icon={entry.icon}
                    label={entry.label}
                    hint="⏎ open"
                    onSelect={() =>
                      rememberAndClose(recentKey(entry), () => {
                        setOpen(false);
                        navigate({ to: entry.to });
                      })
                    }
                  />
                ))}
              </Command.Group>
              <Command.Group heading="Operator console">
                {OPERATOR_NAV_ENTRIES.map((entry) => (
                  <PaletteItem
                    key={`nav-${recentKey(entry)}`}
                    icon={entry.icon}
                    label={entry.label}
                    hint="⏎ open"
                    onSelect={() =>
                      rememberAndClose(recentKey(entry), () => {
                        setOpen(false);
                        navigate({ to: entry.to });
                      })
                    }
                  />
                ))}
              </Command.Group>
            </>
          ) : null}

          {mode === "run" ? (
            <Command.Group heading="Run">
              {runActions.map((action) => (
                <PaletteItem
                  key={action.id}
                  icon={
                    action.id === "refresh-current-view" ? RotateCw : PlayCircle
                  }
                  label={action.label}
                  hint={action.hint}
                  onSelect={() => rememberAndClose(action.id, action.perform)}
                />
              ))}
            </Command.Group>
          ) : null}

          {mode === "filter" ? (
            <Command.Group heading="Filter">
              <PaletteItem
                icon={Filter}
                label="Apply text filter to current view"
                hint="⏎"
                onSelect={() => {
                  setOpen(false);
                  window.dispatchEvent(
                    new CustomEvent("nimbus:filter", { detail: search }),
                  );
                }}
              />
            </Command.Group>
          ) : null}
        </Command.List>
        <div className="flex items-center gap-3 border-t border-app px-3 py-1.5 text-xs text-muted">
          <span className="inline-flex items-center gap-1">
            <Kbd>↑</Kbd>
            <Kbd>↓</Kbd>
            <span>move</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd>⏎</Kbd>
            <span>run</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd>⎋</Kbd>
            <span>close</span>
          </span>
          <span className="ml-auto inline-flex items-center gap-1">
            <CommandIcon size={12} aria-hidden />
            <span>{mode}</span>
          </span>
        </div>
      </Command>
    </div>
  );
}

function ModeToggle({
  current,
  onChange,
}: {
  current: Mode;
  onChange: (m: Mode) => void;
}) {
  const modes: Array<{ id: Mode; label: string }> = [
    { id: "navigate", label: "Navigate" },
    { id: "run", label: "Run" },
    { id: "filter", label: "Filter" },
  ];
  return (
    <fieldset className="inline-flex overflow-hidden rounded-md border text-xs border-app">
      <legend className="sr-only">Palette mode</legend>
      {modes.map((m) => (
        <button
          key={m.id}
          type="button"
          aria-pressed={current === m.id}
          onClick={() => onChange(m.id)}
          className={cn(
            "px-2 py-1 transition-colors",
            current === m.id
              ? "bg-surface-2 text-default"
              : "text-muted hover:bg-surface-2",
          )}
          data-testid={`palette-mode-${m.id}`}
        >
          {m.label}
        </button>
      ))}
    </fieldset>
  );
}

function PaletteItem({
  icon: Icon,
  label,
  hint,
  onSelect,
}: {
  icon: React.ComponentType<{ size?: number; className?: string }>;
  label: string;
  hint?: string;
  onSelect: () => void;
}) {
  return (
    <Command.Item
      onSelect={onSelect}
      className="flex h-9 cursor-default items-center gap-2 rounded px-2 text-sm aria-selected:bg-surface-2 aria-selected:text-default text-muted"
    >
      <Icon size={14} className="shrink-0" />
      <span className="flex-1 text-default">{label}</span>
      {hint ? (
        <span className="text-xs font-mono text-muted">{hint}</span>
      ) : null}
    </Command.Item>
  );
}

function loadRecent(): string[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((v) => typeof v === "string")
      : [];
  } catch {
    return [];
  }
}

function persistRecent(list: string[]) {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* ignore */
  }
}
