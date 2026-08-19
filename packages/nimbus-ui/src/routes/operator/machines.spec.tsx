import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
  }) => (
    <a href={to} data-testid={testId} className={className}>
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

vi.mock("../../shell/sub-drawer", () => ({
  useContributeSubDrawer: () => undefined,
}));

const { toastMock } = vi.hoisted(() => ({
  toastMock: Object.assign(vi.fn(), { error: vi.fn() }),
}));

vi.mock("sonner", () => ({ toast: toastMock }));

// The page's lifecycle side effects live in this hook. Mocking it is what
// lets a test hold a row in its in-flight state without driving a fetch.
const { machineActionsRef, handleActionMock } = vi.hoisted(() => ({
  machineActionsRef: { current: {} as Record<string, string> },
  handleActionMock: vi.fn(),
}));

// Partial: the module also exports `actionsForState` and `OPTIMISTIC_STATES`,
// which the table needs for real. Only the hook is replaced.
vi.mock("./-use-machine-actions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./-use-machine-actions")>()),
  useMachineActions: () => ({
    pending: machineActionsRef.current,
    errors: {},
    confirmDelete: null,
    setConfirmDelete: vi.fn(),
    runAction: vi.fn(),
    handleAction: handleActionMock,
  }),
}));

import { routeComponent } from "../../test/route-internals";
import { Route } from "./machines";

const MachinesPage = routeComponent(Route);

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  machineActionsRef.current = {};
  handleActionMock.mockReset();
  useQueryMock.mockReset().mockReturnValue([]);
  writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

afterEach(() => {
  toastMock.mockClear();
  toastMock.error.mockClear();
});

describe("MachinesPage empty state", () => {
  it("renders the init command as inline code, never as markdown backticks", () => {
    render(<MachinesPage />);
    const body = screen.getByTestId("machines-empty-body");
    expect(body.textContent).not.toContain("`");
    const code = body.querySelector("code");
    expect(code).not.toBeNull();
    expect(code).toHaveTextContent("nimbus machine init");
  });

  it("gives the empty state a next action that copies the command", async () => {
    render(<MachinesPage />);
    fireEvent.click(screen.getByTestId("machines-empty-cta"));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith("nimbus machine init"),
    );
    expect(toastMock).toHaveBeenCalledWith("Copied nimbus machine init");
  });

  it("says so when the host has no clipboard", async () => {
    // The console is normally served over plain http from a remote box, where
    // `navigator.clipboard` does not exist. The command was read off it before
    // any promise existed, so the rejection handler could never run and the
    // only next action the empty state offers failed in silence.
    Object.defineProperty(navigator, "clipboard", {
      value: undefined,
      configurable: true,
    });

    render(<MachinesPage />);
    fireEvent.click(screen.getByTestId("machines-empty-cta"));

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith(
        "Copy failed. The clipboard is not available.",
      ),
    );
    expect(toastMock).not.toHaveBeenCalledWith("Copied nimbus machine init");
  });

  it("keeps the empty state distinct from the loading state", () => {
    render(<MachinesPage />);
    expect(screen.getByTestId("machines-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("machines-loading")).toBeNull();
  });
});

describe("MachinesPage loading state", () => {
  it("holds the table geometry with skeleton rows instead of a centered label", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<MachinesPage />);

    const loading = screen.getByTestId("machines-loading");
    expect(
      loading.querySelectorAll('[data-testid="skeleton-row"]'),
    ).toHaveLength(8);
    expect(loading.querySelectorAll("thead th")).toHaveLength(9);
    // The header is what makes the swap invisible: it must survive the load.
    expect(loading).toHaveTextContent("Provider");
    expect(screen.queryByTestId("machines-empty")).toBeNull();
  });

  it("gives the skeleton the same header as the loaded table", () => {
    useQueryMock.mockReturnValue(undefined);
    const loading = render(<MachinesPage />);
    const skeletonHeader = headerLabels(loading.container);
    loading.unmount();

    useQueryMock.mockReturnValue([
      { _id: "m1", name: "default", state: "running" },
    ]);
    const loaded = render(<MachinesPage />);

    expect(headerLabels(loaded.container)).toEqual(skeletonHeader);
  });
});

describe("MachinesPage action column", () => {
  // The table stopped wrapping its cells, which bought uniform rows and cost
  // width: at 1440x900 it needs 948px in a 910px scroller. Whatever hangs off
  // the right edge is unreachable in practice, because macOS draws no resting
  // scrollbar to say the table scrolls. Here that column is Actions, so the
  // rows read fine and cannot be acted on. jsdom has no layout, so these
  // assert the mechanism rather than the geometry; `pin-verify` measures the
  // overhang itself against a live server.
  const machines = [
    { _id: "m1", name: "default", state: "running" },
    { _id: "m2", name: "spare-01", state: "stopped" },
  ];

  it("pins the actions column against the scroller's right edge", () => {
    useQueryMock.mockReturnValue(machines);
    const { container } = render(<MachinesPage />);

    const header = container.querySelector("thead tr")?.lastElementChild;
    expect(header).toHaveTextContent("Actions");
    expect(header?.className).toContain("sticky");
    expect(header?.className).toContain("right-0");

    const cells = Array.from(container.querySelectorAll("tbody tr")).map(
      (row) => row.lastElementChild,
    );
    expect(cells).toHaveLength(machines.length);
    for (const cell of cells) {
      expect(cell?.className).toContain("sticky");
      expect(cell?.className).toContain("right-0");
    }
  });

  it("gives every row a background for the pinned cell to paint", () => {
    // A pinned cell is transparent unless the row publishes `--row-bg`, and
    // the scrolled columns then read straight through it. Hover counts: it is
    // the state the operator is in while aiming at the button.
    useQueryMock.mockReturnValue(machines);
    const { container } = render(<MachinesPage />);

    for (const row of container.querySelectorAll("tbody tr")) {
      expect(row.className).toMatch(/\[--row-bg:/);
      expect(row.className).toMatch(
        /hover:\[--row-bg:|bg-surface-2 \[--row-bg:/,
      );
    }
  });

  it("keeps the sticky header above the pinned body cells", () => {
    // `position: sticky` with `z-index: auto` opens no stacking context, so a
    // `z-10` pinned cell in the body paints over an unranked header as the
    // rows scroll under it.
    useQueryMock.mockReturnValue(machines);
    const { container } = render(<MachinesPage />);

    expect(container.querySelector("thead")?.className).toContain("z-20");
    expect(
      container.querySelector("tbody tr")?.lastElementChild?.className,
    ).toContain("z-10");
  });
});

function headerLabels(container: HTMLElement) {
  return Array.from(container.querySelectorAll("thead th")).map((cell) =>
    cell.textContent?.trim(),
  );
}

describe("MachinesPage action affordance", () => {
  const machines = [
    { _id: "m1", name: "default", state: "running" },
    { _id: "m2", name: "spare-01", state: "stopped" },
  ];

  // `OPTIMISTIC_STATES` maps every lifecycle action onto an in-flight state,
  // and `actionsForState` offers nothing for those, so a row with a pending
  // action renders no buttons at all rather than grayed-out ones. That is the
  // anti-race contract; this pins it, because the alternative reading — that
  // the row keeps its buttons and disables them — is what the ActionButton's
  // own `disabled` branch is written for, and nothing else says which is true.
  it("offers no actions at all while a lifecycle request is in flight", () => {
    useQueryMock.mockReturnValue(machines);
    machineActionsRef.current = { m1: "stop" };
    render(<MachinesPage />);

    for (const action of ["start", "stop", "restart", "delete"]) {
      expect(
        screen.queryByTestId(`machines-action-${action}-default`),
      ).not.toBeInTheDocument();
    }
    expect(screen.getByTestId("machines-row-default")).toHaveTextContent("…");
  });

  it("leaves an untouched row's actions operable while another is busy", () => {
    useQueryMock.mockReturnValue(machines);
    machineActionsRef.current = { m1: "stop" };
    render(<MachinesPage />);

    const other = screen.getByTestId("machines-action-delete-spare-01");
    expect(other).toHaveAttribute("aria-disabled", "false");
    expect(other).not.toBeDisabled();

    other.focus();
    expect(document.activeElement).toBe(other);

    fireEvent.click(other);
    expect(handleActionMock).toHaveBeenCalledTimes(1);
  });
});
