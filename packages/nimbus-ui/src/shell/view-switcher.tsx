import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useCallback } from "react";

import { SegmentedControl } from "../components/segmented-control";
import {
  persistLastRouteForView,
  readLastRouteForView,
  useUiStore,
} from "../store/ui-store";
import { type NavView, viewFromPathname } from "./nav-entries";

export const VIEW_SEGMENTS: ReadonlyArray<{ value: NavView; label: string }> = [
  { value: "developer", label: "Developer" },
  { value: "operator", label: "Operator" },
];

// useViewSwitch owns the one rule of a view change: the route the user left
// is remembered for that view, and the route they land on is the one they
// last had open in the target view, or its home page. The expanded sidebar
// draws it as a segmented control and the rail as two stacked icons; both
// call the same `switchTo`.
export function useViewSwitch(): {
  activeView: NavView;
  switchTo: (target: NavView) => void;
} {
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const activeView = viewFromPathname(pathname);
  const setLastView = useUiStore((s) => s.setLastView);

  const switchTo = useCallback(
    (target: NavView) => {
      if (target === activeView) return;
      persistLastRouteForView(activeView, pathname);
      setLastView(target);
      const restored = readLastRouteForView(target);
      void navigate({ to: restored ?? `/${target}` });
    },
    [activeView, pathname, navigate, setLastView],
  );

  return { activeView, switchTo };
}

export function ViewSwitcher() {
  const { activeView, switchTo } = useViewSwitch();
  return (
    <SegmentedControl<NavView>
      label="Console view"
      value={activeView}
      options={VIEW_SEGMENTS}
      onChange={switchTo}
      testid="view-switcher"
      className="flex h-7 w-full text-xs"
      segmentClassName="h-6 flex-1 justify-center px-2 py-0 font-medium"
    />
  );
}
