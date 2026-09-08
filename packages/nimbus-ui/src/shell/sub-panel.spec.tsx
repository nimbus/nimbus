import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Box } from "lucide-react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { pathnameRef, searchRef } = vi.hoisted(() => ({
  pathnameRef: { current: "/developer/settings" },
  searchRef: { current: {} as Record<string, unknown> },
}));

vi.mock("@tanstack/react-router", () => ({
  Link: ({
    to,
    children,
    "aria-current": ariaCurrent,
    "data-testid": testId,
    "data-active": dataActive,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "aria-current"?: "page" | undefined;
    "data-testid"?: string;
    "data-active"?: string;
    className?: string;
  }) => (
    <a
      href={to}
      aria-current={ariaCurrent}
      data-testid={testId}
      data-active={dataActive}
      className={className}
    >
      {children}
    </a>
  ),
  useRouterState: ({
    select,
  }: {
    select: (s: {
      location: { pathname: string; search: Record<string, unknown> };
    }) => unknown;
  }) =>
    select({
      location: {
        pathname: pathnameRef.current,
        search: searchRef.current,
      },
    }),
}));

import {
  SUB_PANEL_DEFAULT_WIDTH,
  SUB_PANEL_MIN_WIDTH,
  SUB_PANEL_RAIL_WIDTH,
  SubPanelLayout,
  type SubPanelPrefs,
  SubPanelProvider,
  type SubPanelSpec,
  subPanelStorageKey,
  useContributeSubPanel,
} from "./sub-panel";

// ---------------------------------------------------------------------------
// Geometry
//
// happy-dom performs no layout, and react-resizable-panels sizes its group
// from the panels' `offsetWidth`. This shim answers that measurement from the
// inline styles the library writes: a panel with a pixel `flex-basis` is that
// wide (a first render from `defaultSize`), a panel whose siblings carry
// percentage `flex-grow` values that sum to 100 is its share of the group
// (every render after layout), and anything else splits the group evenly,
// which is what a browser does with the `flex-grow: 1` the library writes
// before a panel is in the layout. The library only ever sums these, so any
// split that adds up to the group width measures the same. One arrow key on
// the separator moves the layout 5%, so the group is 1200px wide to keep
// every step on a whole pixel: 5% = 60px.
//
// happy-dom also lacks the `ariaDisabled` reflection the library reads on
// each separator; without it every separator reads as disabled and no key
// press resizes anything.

const GROUP_WIDTH = 1200;
const ARROW_STEP = GROUP_WIDTH / 20;

function inlinePx(el: HTMLElement, prop: string): number | null {
  const raw = el.style.getPropertyValue(prop);
  if (!raw.endsWith("px")) return null;
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) && value > 0 ? value : null;
}

function panelWidth(el: HTMLElement): number {
  const basis = inlinePx(el, "flex-basis");
  if (basis !== null) return basis;
  const siblings = Array.from(el.parentElement?.children ?? []).filter(
    (node): node is HTMLElement =>
      node instanceof HTMLElement && node.hasAttribute("data-panel"),
  );
  const grows = siblings.map((p) =>
    Number.parseFloat(p.style.getPropertyValue("flex-grow")),
  );
  const total = grows.reduce((sum, g) => sum + (Number.isFinite(g) ? g : 0), 0);
  if (Math.abs(total - 100) < 0.01) {
    const own = Number.parseFloat(el.style.getPropertyValue("flex-grow"));
    return Math.round((own / 100) * GROUP_WIDTH);
  }
  const fixed = siblings
    .map((p) => inlinePx(p, "flex-basis") ?? 0)
    .reduce((sum, w) => sum + w, 0);
  const flexible = siblings.filter((p) => inlinePx(p, "flex-basis") === null);
  return flexible.length > 0 ? (GROUP_WIDTH - fixed) / flexible.length : 0;
}

function installAriaDisabled() {
  if ("ariaDisabled" in HTMLElement.prototype) return () => {};
  Object.defineProperty(HTMLElement.prototype, "ariaDisabled", {
    configurable: true,
    get(this: HTMLElement) {
      return this.getAttribute("aria-disabled");
    },
  });
  return () => {
    Reflect.deleteProperty(HTMLElement.prototype, "ariaDisabled");
  };
}

function elementWidth(el: HTMLElement): number {
  if (el.hasAttribute("data-group")) return GROUP_WIDTH;
  if (el.hasAttribute("data-panel")) return panelWidth(el);
  if (el.hasAttribute("data-separator")) return 1;
  return 0;
}

// The library orders a group's children by `offsetLeft`, so each child sits
// after the widths of the children before it.
function elementLeft(el: HTMLElement): number {
  let left = 0;
  for (const sibling of Array.from(el.parentElement?.children ?? [])) {
    if (sibling === el) break;
    if (sibling instanceof HTMLElement) left += elementWidth(sibling);
  }
  return left;
}

let restoreGeometry: () => void = () => {};

function installGeometry() {
  const originals = {
    offsetWidth: Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "offsetWidth",
    ),
    offsetLeft: Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "offsetLeft",
    ),
  };
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return elementWidth(this);
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetLeft", {
    configurable: true,
    get(this: HTMLElement) {
      return elementLeft(this);
    },
  });
  const restoreAria = installAriaDisabled();
  restoreGeometry = () => {
    restoreAria();
    for (const [name, original] of Object.entries(originals)) {
      if (original) {
        Object.defineProperty(HTMLElement.prototype, name, original);
      } else {
        Reflect.deleteProperty(HTMLElement.prototype, name);
      }
    }
  };
}

// ---------------------------------------------------------------------------
// Harness

function setPathname(path: string) {
  pathnameRef.current = path;
}

function Contributor({ spec }: { spec: SubPanelSpec | null }) {
  useContributeSubPanel(spec);
  return null;
}

// The global setup stubs matchMedia to always miss, so every other test in this
// file runs at the desktop tier. This narrows the viewport for the tier tests.
function stubTablet() {
  vi.stubGlobal(
    "matchMedia",
    (query: string) =>
      ({
        matches: query.includes("1023px"),
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
      }) as unknown as MediaQueryList,
  );
}

const STATIC_SPEC: SubPanelSpec = {
  kind: "static",
  title: "Settings",
  items: [
    {
      id: "environment",
      label: "Environment",
      to: "/developer/settings/environment",
    },
    { id: "secrets", label: "Secrets", to: "/developer/settings/secrets" },
    { id: "schema", label: "Schema", to: "/developer/settings/schema" },
  ],
};

const DYNAMIC_SPEC: SubPanelSpec = {
  kind: "dynamic",
  title: "Tenants",
  search: { placeholder: "Filter tenants", rows: 3 },
  children: <div data-testid="dynamic-body" />,
};

function renderLayout(spec: SubPanelSpec | null = STATIC_SPEC) {
  return render(
    <SubPanelProvider>
      <Contributor spec={spec} />
      <SubPanelLayout>
        <div data-testid="page" />
      </SubPanelLayout>
    </SubPanelProvider>,
  );
}

function storedPrefs(section: string): SubPanelPrefs | null {
  const raw = window.localStorage.getItem(subPanelStorageKey(section));
  return raw ? (JSON.parse(raw) as SubPanelPrefs) : null;
}

function storePrefs(section: string, prefs: SubPanelPrefs) {
  window.localStorage.setItem(
    subPanelStorageKey(section),
    JSON.stringify(prefs),
  );
}

function separator(): HTMLElement {
  return screen.getByRole("separator", { name: "Resize sub-panel" });
}

function panelElement(): HTMLElement {
  const panel = screen.getByTestId("sub-panel").closest("[data-panel]");
  if (!(panel instanceof HTMLElement)) throw new Error("no panel element");
  return panel;
}

function pressArrow(key: "ArrowLeft" | "ArrowRight", times = 1) {
  for (let i = 0; i < times; i += 1) {
    fireEvent.keyDown(separator(), { key });
  }
}

beforeEach(() => {
  setPathname("/developer/settings");
  searchRef.current = {};
  window.localStorage.clear();
  installGeometry();
});

afterEach(() => {
  restoreGeometry();
  window.localStorage.clear();
  vi.unstubAllGlobals();
});

// ---------------------------------------------------------------------------

describe("SubPanelLayout", () => {
  it("renders only the page when no contributor mounts", () => {
    renderLayout(null);
    expect(screen.getByTestId("page")).toBeInTheDocument();
    expect(screen.queryByTestId("sub-panel")).toBeNull();
    expect(screen.queryByTestId("sub-panel-separator")).toBeNull();
    expect(storedPrefs("settings")).toBeNull();
  });

  it("keeps the page mounted when a spec arrives", () => {
    function Host() {
      const [spec, setSpec] = useState<SubPanelSpec | null>(null);
      return (
        <SubPanelProvider>
          <Contributor spec={spec} />
          <SubPanelLayout>
            <input data-testid="page-input" defaultValue="" />
          </SubPanelLayout>
          <button
            type="button"
            data-testid="contribute"
            onClick={() => setSpec(STATIC_SPEC)}
          >
            contribute
          </button>
        </SubPanelProvider>
      );
    }
    render(<Host />);
    const input = screen.getByTestId<HTMLInputElement>("page-input");
    fireEvent.change(input, { target: { value: "typed" } });
    fireEvent.click(screen.getByTestId("contribute"));
    expect(screen.getByTestId("sub-panel")).toBeInTheDocument();
    // Same element, same uncommitted value: the page did not remount.
    expect(screen.getByTestId("page-input")).toBe(input);
    expect(input.value).toBe("typed");
  });

  it("renders a static contributor with items + active-state highlight", () => {
    setPathname("/developer/settings/secrets");
    renderLayout(STATIC_SPEC);
    const panel = screen.getByTestId("sub-panel");
    expect(panel).toHaveAttribute("data-kind", "static");
    expect(panel).toHaveAttribute("data-collapsed", "false");
    expect(screen.getByTestId("sub-panel-item-environment")).toHaveAttribute(
      "data-active",
      "false",
    );
    const secrets = screen.getByTestId("sub-panel-item-secrets");
    expect(secrets).toHaveAttribute("data-active", "true");
    expect(secrets).toHaveAttribute("aria-current", "page");
  });

  it("renders dynamic contributor children", () => {
    renderLayout(DYNAMIC_SPEC);
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-kind",
      "dynamic",
    );
    expect(screen.getByTestId("dynamic-body")).toBeInTheDocument();
  });

  it("shows the search field only once the list exceeds twenty rows", () => {
    const { unmount } = renderLayout({
      ...DYNAMIC_SPEC,
      search: { placeholder: "Filter tenants", rows: 20 },
    });
    expect(screen.queryByTestId("sub-panel-search")).toBeNull();
    unmount();
    renderLayout({
      ...DYNAMIC_SPEC,
      search: { placeholder: "Filter tenants", rows: 21 },
    });
    const search = screen.getByTestId("sub-panel-search");
    expect(search).toHaveAttribute("placeholder", "Filter tenants");
    expect(search).toHaveAttribute("data-inline-search", "panel");
  });

  it("uses search-param match for active state when items declare search", () => {
    setPathname("/developer/settings");
    searchRef.current = { section: "secrets" };
    renderLayout({
      kind: "static",
      title: "Settings",
      items: [
        {
          id: "environment",
          label: "Environment",
          to: "/developer/settings",
          search: { section: "environment" },
        },
        {
          id: "secrets",
          label: "Secrets",
          to: "/developer/settings",
          search: { section: "secrets" },
        },
      ],
    });
    expect(screen.getByTestId("sub-panel-item-environment")).toHaveAttribute(
      "data-active",
      "false",
    );
    expect(screen.getByTestId("sub-panel-item-secrets")).toHaveAttribute(
      "data-active",
      "true",
    );
  });

  it("clears the spec when the contributor unmounts", () => {
    function Host() {
      const [mounted, setMounted] = useState(true);
      return (
        <SubPanelProvider>
          {mounted ? <Contributor spec={STATIC_SPEC} /> : null}
          <SubPanelLayout>
            <div data-testid="page" />
          </SubPanelLayout>
          <button
            type="button"
            data-testid="toggle"
            onClick={() => setMounted(false)}
          >
            unmount
          </button>
        </SubPanelProvider>
      );
    }
    render(<Host />);
    expect(screen.getByTestId("sub-panel")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("toggle"));
    expect(screen.queryByTestId("sub-panel")).toBeNull();
    expect(screen.getByTestId("page")).toBeInTheDocument();
  });
});

describe("SubPanelLayout resizing", () => {
  it("opens at the default width and records it for the section", () => {
    renderLayout();
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_DEFAULT_WIDTH);
    expect(storedPrefs("settings")).toEqual({
      width: SUB_PANEL_DEFAULT_WIDTH,
      collapsed: false,
    });
  });

  it("resizes from the keyboard on the separator", () => {
    renderLayout();
    const handle = separator();
    expect(handle).toHaveAttribute("tabindex", "0");
    pressArrow("ArrowRight");
    expect(panelElement().offsetWidth).toBe(
      SUB_PANEL_DEFAULT_WIDTH + ARROW_STEP,
    );
    expect(storedPrefs("settings")?.width).toBe(
      SUB_PANEL_DEFAULT_WIDTH + ARROW_STEP,
    );
    pressArrow("ArrowLeft", 2);
    expect(panelElement().offsetWidth).toBe(
      SUB_PANEL_DEFAULT_WIDTH - ARROW_STEP,
    );
    expect(storedPrefs("settings")?.width).toBe(
      SUB_PANEL_DEFAULT_WIDTH - ARROW_STEP,
    );
  });

  it("collapses to the rail on a key press past the minimum and keeps the last width", () => {
    renderLayout();
    pressArrow("ArrowLeft");
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_MIN_WIDTH);
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    pressArrow("ArrowLeft");
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_RAIL_WIDTH);
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(storedPrefs("settings")).toEqual({
      width: SUB_PANEL_MIN_WIDTH,
      collapsed: true,
    });
    fireEvent.click(screen.getByTestId("sub-panel-toggle"));
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_MIN_WIDTH);
    expect(storedPrefs("settings")).toEqual({
      width: SUB_PANEL_MIN_WIDTH,
      collapsed: false,
    });
  });

  it("keeps the width across a remount", () => {
    const { unmount } = renderLayout();
    pressArrow("ArrowRight", 2);
    const chosen = SUB_PANEL_DEFAULT_WIDTH + 2 * ARROW_STEP;
    expect(storedPrefs("settings")?.width).toBe(chosen);
    unmount();
    renderLayout();
    expect(panelElement().offsetWidth).toBe(chosen);
    expect(storedPrefs("settings")).toEqual({
      width: chosen,
      collapsed: false,
    });
  });

  it("keeps one width per section", () => {
    setPathname("/developer/storage/users");
    const { unmount } = renderLayout();
    pressArrow("ArrowRight");
    expect(storedPrefs("storage")?.width).toBe(
      SUB_PANEL_DEFAULT_WIDTH + ARROW_STEP,
    );
    expect(storedPrefs("settings")).toBeNull();
    unmount();
    setPathname("/operator/settings");
    renderLayout();
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_DEFAULT_WIDTH);
    expect(storedPrefs("storage")?.width).toBe(
      SUB_PANEL_DEFAULT_WIDTH + ARROW_STEP,
    );
  });

  it("collapses to the rail and restores the chosen width on expand", () => {
    renderLayout();
    pressArrow("ArrowRight", 2);
    const chosen = SUB_PANEL_DEFAULT_WIDTH + 2 * ARROW_STEP;
    fireEvent.click(screen.getByTestId("sub-panel-toggle"));
    // Collapsed to a rail: still present and reachable, not removed.
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_RAIL_WIDTH);
    expect(storedPrefs("settings")).toEqual({
      width: chosen,
      collapsed: true,
    });
    fireEvent.click(screen.getByTestId("sub-panel-toggle"));
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    expect(panelElement().offsetWidth).toBe(chosen);
    expect(storedPrefs("settings")).toEqual({
      width: chosen,
      collapsed: false,
    });
  });

  it("hydrates a collapsed section as the rail and expands to the stored width", () => {
    storePrefs("settings", { width: 300, collapsed: true });
    renderLayout();
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(panelElement().offsetWidth).toBe(SUB_PANEL_RAIL_WIDTH);
    fireEvent.click(screen.getByTestId("sub-panel-toggle"));
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "false",
    );
    expect(panelElement().offsetWidth).toBe(300);
  });

  it("clamps a stored width into the allowed range", () => {
    storePrefs("settings", { width: 9000, collapsed: false });
    renderLayout();
    expect(panelElement().offsetWidth).toBe(400);
    expect(storedPrefs("settings")?.width).toBe(400);
  });

  it("shows all rail items in the collapsed rail and switches on click without expanding", () => {
    storePrefs("settings", { width: SUB_PANEL_DEFAULT_WIDTH, collapsed: true });
    const onFunctions = vi.fn();
    const onSandboxes = vi.fn();
    renderLayout({
      kind: "dynamic",
      title: "Compute",
      children: <div data-testid="dynamic-body" />,
      railItems: [
        {
          id: "functions",
          label: "Functions",
          icon: Box,
          active: true,
          onSelect: onFunctions,
        },
        {
          id: "sandboxes",
          label: "Sandboxes",
          icon: Box,
          active: false,
          onSelect: onSandboxes,
        },
      ],
    });
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(screen.getByTestId("sub-panel-rail-item-functions")).toHaveAttribute(
      "data-active",
      "true",
    );
    expect(screen.getByTestId("sub-panel-rail-item-sandboxes")).toHaveAttribute(
      "data-active",
      "false",
    );
    fireEvent.click(screen.getByTestId("sub-panel-rail-item-sandboxes"));
    expect(onSandboxes).toHaveBeenCalledTimes(1);
    // Switching sub-view from the rail does not expand the panel.
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(storedPrefs("settings")?.collapsed).toBe(true);
  });
});

describe("SubPanelLayout below the desktop tier", () => {
  it("shows the rail without a sheet even when the stored state is open", () => {
    stubTablet();
    storePrefs("settings", { width: 300, collapsed: false });
    renderLayout(DYNAMIC_SPEC);
    // The sheet is modal. Honoring a stored desktop preference here would
    // drop a scrim over the content the moment the viewport narrows.
    expect(screen.getByTestId("sub-panel")).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(screen.queryByTestId("sub-panel-overlay")).toBeNull();
    expect(screen.queryByTestId("sub-panel-scrim")).toBeNull();
    expect(screen.queryByTestId("sub-panel-separator")).toBeNull();
  });

  it("opens an overlay sheet on an explicit expand", () => {
    stubTablet();
    renderLayout(DYNAMIC_SPEC);
    fireEvent.click(screen.getByTestId("sub-panel-toggle"));
    expect(screen.getByTestId("sub-panel-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("sub-panel-scrim")).toBeInTheDocument();
    // The rail stays in flow beneath the sheet, so dismissing it still
    // leaves a visible way back.
    expect(screen.getByTestId("sub-panel")).toBeInTheDocument();
    expect(screen.getByTestId("dynamic-body")).toBeInTheDocument();
  });

  it("never writes the desktop preference at tablet width", () => {
    stubTablet();
    storePrefs("settings", { width: 300, collapsed: false });
    renderLayout(DYNAMIC_SPEC);
    fireEvent.click(screen.getByTestId("sub-panel-toggle"));
    expect(screen.getByTestId("sub-panel-overlay")).toBeInTheDocument();
    expect(storedPrefs("settings")).toEqual({ width: 300, collapsed: false });
  });
});

/**
 * DESIGN.md §Spacing And Shape: "Icon button: 32px square, 36px on touch
 * surfaces."
 *
 * The two toggles are one control in two positions -- collapse the panel and
 * the rail's expand button lands where your pointer already is. They shipped at
 * 24px and 28px, so the smallest targets in the shell were the two buttons that
 * swap places with each other, sitting beside 32px rail items.
 *
 * happy-dom performs no layout, so the size utilities are the only thing a
 * test can read back.
 */
describe("SubPanel icon-button hit targets", () => {
  const SPEC: SubPanelSpec = {
    kind: "dynamic",
    title: "Compute",
    children: <div data-testid="dynamic-body" />,
    railItems: [
      {
        id: "functions",
        label: "Functions",
        icon: Box,
        active: true,
        onSelect: () => {},
      },
    ],
  };

  it("sizes the panel collapse toggle 32px square", () => {
    renderLayout(SPEC);
    const collapse = screen.getByTestId("sub-panel-toggle");
    expect(collapse.className.split(" ")).toEqual(
      expect.arrayContaining(["h-8", "w-8"]),
    );
  });

  it("sizes the rail expand toggle to match the rail items under it", () => {
    storePrefs("settings", { width: SUB_PANEL_DEFAULT_WIDTH, collapsed: true });
    renderLayout(SPEC);
    const expand = screen.getByTestId("sub-panel-toggle");
    const railItem = screen.getByTestId("sub-panel-rail-item-functions");
    // The rail is a 32px column, so both fill its width rather than naming it.
    expect(expand.className.split(" ")).toEqual(
      expect.arrayContaining(["h-8", "w-full"]),
    );
    expect(railItem.className.split(" ")).toEqual(
      expect.arrayContaining(["h-8", "w-full"]),
    );
  });
});

/**
 * A disabled item points at a sub-view that does not exist yet.
 *
 * It used to render as a real `<Link>` dimmed with `pointer-events-none`,
 * which only guards the mouse: the anchor kept its place in the tab order and
 * still fired on Enter. And because the target view is unbuilt, the router
 * dropped the item's `tab` search param and landed on a different view than
 * the label named -- keyboard users were routed somewhere they did not ask
 * for, silently.
 */
describe("SubPanel disabled items", () => {
  const SPEC: SubPanelSpec = {
    kind: "static",
    title: "Observability",
    items: [
      { id: "logs", label: "Logs", to: "/operator/observability" },
      {
        id: "events",
        label: "Events",
        to: "/operator/observability",
        disabled: true,
      },
    ],
  };

  function renderSpec() {
    setPathname("/operator/observability");
    renderLayout(SPEC);
  }

  it("is not a link and is not in the tab order", async () => {
    const user = userEvent.setup();
    renderSpec();
    const events = screen.getByTestId("sub-panel-item-events");
    const logs = screen.getByTestId("sub-panel-item-logs");

    expect(events.tagName).not.toBe("A");
    expect(events).not.toHaveAttribute("href");
    expect(events).toHaveAttribute("aria-disabled", "true");
    expect(events).not.toHaveAttribute("tabindex");

    // Tab order, not `focus()`: the DOM happily focuses any element you ask
    // it to, so only sequential navigation answers the question a keyboard
    // user is actually asking. Events sits directly after Logs in the list,
    // so it is the very next stop if it is still a link.
    logs.focus();
    await user.tab();
    expect(document.activeElement).not.toBe(events);

    // The enabled sibling is still a link, so the difference is the disabled
    // state and not a broken renderer.
    expect(logs.tagName).toBe("A");
  });

  it("carries the shared coming-soon chip rather than the label text", () => {
    renderSpec();
    const chip = screen.getByTestId("sub-panel-item-events-coming-soon");

    expect(chip).toHaveTextContent("coming soon");
    // Hidden from the accessibility tree: `aria-disabled` on the row already
    // announces the state, so the chip would be a duplicate.
    expect(chip).toHaveAttribute("aria-hidden");
    // The label itself stays clean -- no "· soon" suffix baked into the route
    // that each caller has to remember to write.
    expect(screen.getByTestId("sub-panel-item-events")).toHaveTextContent(
      /^Eventscoming soon$/,
    );
  });
});
