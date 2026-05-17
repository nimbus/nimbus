import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useCallback, useRef } from "react";
import { cn } from "../lib/cn";
import {
  persistLastRouteForView,
  readLastRouteForView,
  useUiStore,
} from "../store/ui-store";
import { type NavView, viewFromPathname } from "./nav-entries";

const SEGMENTS: ReadonlyArray<{ view: NavView; label: string }> = [
  { view: "developer", label: "Developer" },
  { view: "operator", label: "Operator" },
];

const VIEW_DEFAULT: Record<NavView, string> = {
  developer: "/app",
  operator: "/admin",
};

export function ViewSwitcher() {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const activeView = viewFromPathname(pathname);
  const setLastView = useUiStore((s) => s.setLastView);
  const refs = useRef<Record<NavView, HTMLButtonElement | null>>({
    developer: null,
    operator: null,
  });

  const switchTo = useCallback(
    (target: NavView) => {
      if (target === activeView) return;
      persistLastRouteForView(activeView, pathname);
      setLastView(target);
      const restored = readLastRouteForView(target);
      void navigate({ to: restored ?? VIEW_DEFAULT[target] });
    },
    [activeView, pathname, navigate, setLastView],
  );

  const onKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLButtonElement>, current: NavView) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      const other: NavView = current === "developer" ? "operator" : "developer";
      refs.current[other]?.focus();
    },
    [],
  );

  return (
    <fieldset
      className="inline-flex overflow-hidden rounded-md border text-xs border-app"
      data-testid="view-switcher"
      aria-label="Console view"
    >
      <legend className="sr-only">Console view</legend>
      {SEGMENTS.map((segment) => {
        const active = segment.view === activeView;
        return (
          <button
            key={segment.view}
            ref={(node) => {
              refs.current[segment.view] = node;
            }}
            type="button"
            aria-pressed={active}
            onClick={() => switchTo(segment.view)}
            onKeyDown={(e) => onKeyDown(e, segment.view)}
            tabIndex={active ? 0 : -1}
            className={cn(
              "h-7 px-3 font-mono uppercase tracking-[0.12em] transition-colors",
              active
                ? "bg-surface-2 text-default"
                : "text-muted hover:bg-surface-2 hover:text-default",
            )}
            data-testid={`view-switcher-${segment.view}`}
          >
            {segment.label}
          </button>
        );
      })}
    </fieldset>
  );
}
