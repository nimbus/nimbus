import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useCallback } from "react";

import { SegmentedControl } from "../components/segmented-control";
import {
  persistLastRouteForView,
  readLastRouteForView,
  useUiStore,
} from "../store/ui-store";
import { type NavView, viewFromPathname } from "./nav-entries";

const SEGMENTS: ReadonlyArray<{ value: NavView; label: string }> = [
  { value: "developer", label: "Developer" },
  { value: "operator", label: "Operator" },
];

export function ViewSwitcher() {
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

  return (
    <SegmentedControl<NavView>
      label="Console view"
      value={activeView}
      options={SEGMENTS}
      onChange={switchTo}
      testid="view-switcher"
      className="h-7 text-xs"
      segmentClassName="h-7 px-3 py-0 font-mono uppercase tracking-[0.12em]"
    />
  );
}
