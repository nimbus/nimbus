import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { useSearchMock, navigateMock } = vi.hoisted(() => ({
  useSearchMock: vi.fn(),
  navigateMock: vi.fn(),
}));

// The current address the page reads; set per test and applied to the
// search reducer the page hands to navigate.
let currentSearch: Record<string, unknown> = {};
function setSearchTo(search: Record<string, unknown>) {
  currentSearch = search;
  useSearchMock.mockReturnValue(search);
}

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useSearch: (..._args: unknown[]) => useSearchMock(),
  useNavigate: () => navigateMock,
  Link: ({
    children,
    "data-testid": testId,
  }: {
    children: React.ReactNode;
    "data-testid"?: string;
  }) => (
    <a href="#mock" data-testid={testId}>
      {children}
    </a>
  ),
}));

const { useQueryMock } = vi.hoisted(() => ({ useQueryMock: vi.fn() }));

vi.mock("@nimbus/nimbus/react", () => ({
  useQuery: (..._args: unknown[]) => useQueryMock(),
}));

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: () => undefined,
}));

const { scheduleApiMock, toastMock } = vi.hoisted(() => ({
  scheduleApiMock: {
    runNow: vi.fn(),
    cancel: vi.fn(),
    listCrons: vi.fn(),
    removeCron: vi.fn(),
  },
  toastMock: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
}));

vi.mock("../../lib/api-mutations", () => ({ schedules: scheduleApiMock }));
vi.mock("sonner", () => ({ toast: toastMock }));

import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import { Route } from "./schedules";

const SchedulesPage = routeComponent(Route);

const PENDING_JOB = {
  _id: "scheduled-job:acme:job~2d1",
  tenantId: "acme",
  functionPath: "documents.pings.insert",
  status: "pending",
  scheduledTime: Date.now() + 60_000,
  args: { type: "insert", table: "pings", fields: { at: 1 } },
};

const DONE_JOB = {
  _id: "scheduled-job:acme:job~2d2",
  tenantId: "acme",
  functionPath: "documents.pings.delete",
  status: "failed",
  scheduledTime: Date.now() - 60_000,
  args: { type: "delete", table: "pings", id: "p1" },
  result: {
    finishedAt: Date.now() - 59_000,
    outcome: "failed",
    error: "document p1 not found",
  },
};

const CRON = {
  _id: "cron-job:acme:sweep",
  tenantId: "acme",
  name: "sweep",
  schedule: "interval:30s",
  functionPath: "documents.pings.insert",
  status: "active",
  nextRunAt: Date.now() + 30_000,
};

beforeEach(() => {
  useQueryMock.mockReset();
  navigateMock.mockReset();
  for (const fn of Object.values(scheduleApiMock)) fn.mockReset();
  scheduleApiMock.runNow.mockResolvedValue({
    ok: true,
    data: { job_id: "j2" },
  });
  toastMock.success.mockReset();
  toastMock.error.mockReset();
  useSearchMock.mockReset();
  setSearchTo({ section: "scheduled" });
  useUiStore.setState({ activeTenant: "acme" });
});

afterEach(() => {
  useUiStore.setState({ activeTenant: null });
});

// The mock navigate receives a search reducer; the tests apply it to the
// current search to see what address the page asked for.
function requestedSearch(call = -1): Record<string, unknown> {
  const calls = navigateMock.mock.calls;
  const args = calls.at(call)?.[0] as
    | { search: (prev: Record<string, unknown>) => Record<string, unknown> }
    | undefined;
  if (!args) throw new Error("navigate was not called");
  return args.search(currentSearch);
}

function headerLabels(container: HTMLElement) {
  return Array.from(container.querySelectorAll('[role="columnheader"]')).map(
    (cell) => cell.textContent?.trim(),
  );
}

describe("SchedulesPage loading state", () => {
  it("holds the scheduled table geometry with skeleton rows", () => {
    useQueryMock.mockReturnValue(undefined);
    render(<SchedulesPage />);

    const table = screen.getByTestId("schedules-scheduled-table");
    expect(table).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getAllByTestId("schedules-scheduled-table-skeleton-row"),
    ).toHaveLength(8);
    // Function, Status, Scheduled, Finished, Outcome, and the actions column.
    expect(table.querySelectorAll('[role="columnheader"]')).toHaveLength(6);
    expect(table).toHaveTextContent("Outcome");
  });

  it("holds the cron table geometry with skeleton rows", () => {
    setSearchTo({ section: "cron" });
    useQueryMock.mockReturnValue(undefined);
    render(<SchedulesPage />);

    const table = screen.getByTestId("schedules-cron-table");
    expect(table).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getAllByTestId("schedules-cron-table-skeleton-row"),
    ).toHaveLength(8);
    expect(table.querySelectorAll('[role="columnheader"]')).toHaveLength(7);
    expect(table).toHaveTextContent("Next run");
  });

  it("gives each skeleton the same header as its loaded table", () => {
    for (const section of ["scheduled", "cron"] as const) {
      setSearchTo({ section });

      useQueryMock.mockReturnValue(undefined);
      const loading = render(<SchedulesPage />);
      const skeletonHeader = headerLabels(loading.container);
      loading.unmount();

      useQueryMock.mockReturnValue([section === "cron" ? CRON : PENDING_JOB]);
      const loaded = render(<SchedulesPage />);
      expect(headerLabels(loaded.container)).toEqual(skeletonHeader);
      loaded.unmount();
    }
  });
});

describe("SchedulesPage empty states", () => {
  it("shows the scheduled empty state once the query settles on zero jobs", () => {
    useQueryMock.mockReturnValue([]);
    render(<SchedulesPage />);

    expect(screen.getByText("No scheduled jobs")).toBeInTheDocument();
    expect(screen.queryByTestId("schedules-scheduled-table")).toBeNull();
  });

  it("shows the cron empty state once the query settles on zero jobs", () => {
    setSearchTo({ section: "cron" });
    useQueryMock.mockReturnValue([]);
    render(<SchedulesPage />);

    expect(screen.getByText("No cron jobs")).toBeInTheDocument();
    expect(screen.queryByTestId("schedules-cron-table")).toBeNull();
  });
});

describe("SchedulesPage rows", () => {
  it("shows the outcome and error of a finished job", () => {
    useQueryMock.mockReturnValue([PENDING_JOB, DONE_JOB]);
    render(<SchedulesPage />);

    const row = screen.getByTestId(`schedules-scheduled-${DONE_JOB._id}`);
    expect(row).toHaveTextContent("failed: document p1 not found");
    expect(row.querySelector("[data-state]")).toHaveAttribute(
      "data-state",
      "failed",
    );
  });

  it("says the cron schedule the way an operator would", () => {
    setSearchTo({ section: "cron" });
    useQueryMock.mockReturnValue([CRON]);
    render(<SchedulesPage />);

    expect(screen.getByTestId("schedules-cron-sweep")).toHaveTextContent(
      "every 30s",
    );
  });

  it("opens the job sheet through ?job= on row activation", () => {
    useQueryMock.mockReturnValue([PENDING_JOB]);
    render(<SchedulesPage />);

    fireEvent.click(
      screen.getByTestId(`schedules-scheduled-${PENDING_JOB._id}`),
    );
    expect(requestedSearch()).toEqual({
      section: "scheduled",
      job: PENDING_JOB._id,
      cron: undefined,
    });
  });

  it("opens the cron sheet through ?cron= on row activation", () => {
    setSearchTo({ section: "cron" });
    useQueryMock.mockReturnValue([CRON]);
    render(<SchedulesPage />);

    fireEvent.click(screen.getByTestId("schedules-cron-sweep"));
    expect(requestedSearch()).toEqual({
      section: "cron",
      cron: "sweep",
      job: undefined,
    });
  });
});

describe("SchedulesPage Run now", () => {
  it("re-enqueues the job mutation from the row menu and toasts", async () => {
    useQueryMock.mockReturnValue([PENDING_JOB]);
    render(<SchedulesPage />);

    fireEvent.contextMenu(
      screen.getByTestId(`schedules-scheduled-${PENDING_JOB._id}`),
    );
    const menu = screen.getByTestId("schedules-row-menu");
    fireEvent.click(within(menu).getByTestId("schedules-row-menu-run"));

    await waitFor(() =>
      expect(scheduleApiMock.runNow).toHaveBeenCalledTimes(1),
    );
    expect(scheduleApiMock.runNow).toHaveBeenCalledWith(
      "acme",
      PENDING_JOB.args,
    );
    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith(
        "Queued documents.pings.insert to run now",
      ),
    );
  });

  it("reads the cron mutation from the crons route before enqueueing it", async () => {
    setSearchTo({ section: "cron" });
    useQueryMock.mockReturnValue([CRON]);
    const mutation = { type: "insert", table: "pings", fields: { by: "cron" } };
    scheduleApiMock.listCrons.mockResolvedValue({
      ok: true,
      data: {
        crons: [
          {
            name: "other",
            schedule: { type: "interval", seconds: 5 },
            mutation: { type: "delete", table: "x", id: "1" },
            enabled: true,
          },
          {
            name: "sweep",
            schedule: { type: "interval", seconds: 30 },
            mutation,
            enabled: true,
          },
        ],
      },
    });
    render(<SchedulesPage />);

    fireEvent.click(screen.getByTestId("schedules-cron-actions-sweep"));
    fireEvent.click(
      within(screen.getByTestId("schedules-row-menu")).getByTestId(
        "schedules-row-menu-run",
      ),
    );

    await waitFor(() =>
      expect(scheduleApiMock.runNow).toHaveBeenCalledTimes(1),
    );
    expect(scheduleApiMock.listCrons).toHaveBeenCalledWith("acme");
    expect(scheduleApiMock.runNow).toHaveBeenCalledWith("acme", mutation);
    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith("Queued sweep to run now"),
    );
  });

  it("reports a refusal through the error toast", async () => {
    useQueryMock.mockReturnValue([PENDING_JOB]);
    scheduleApiMock.runNow.mockResolvedValue({
      ok: false,
      error: "tenant_not_found",
      status: 404,
    });
    render(<SchedulesPage />);

    fireEvent.contextMenu(
      screen.getByTestId(`schedules-scheduled-${PENDING_JOB._id}`),
    );
    fireEvent.click(
      within(screen.getByTestId("schedules-row-menu")).getByTestId(
        "schedules-row-menu-run",
      ),
    );

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith(
        "Run now refused for documents.pings.insert",
        { description: "tenant_not_found" },
      ),
    );
    expect(toastMock.success).not.toHaveBeenCalled();
  });
});

describe("SchedulesPage cancel and delete", () => {
  it("offers cancel on a pending job only, and cancels by the decoded job id", async () => {
    useQueryMock.mockReturnValue([PENDING_JOB, DONE_JOB]);
    scheduleApiMock.cancel.mockResolvedValue({ ok: true, data: undefined });
    render(<SchedulesPage />);

    fireEvent.contextMenu(
      screen.getByTestId(`schedules-scheduled-${DONE_JOB._id}`),
    );
    expect(
      within(screen.getByTestId("schedules-row-menu")).queryByTestId(
        "schedules-row-menu-cancel",
      ),
    ).toBeNull();
    fireEvent.keyDown(screen.getByTestId("schedules-row-menu"), {
      key: "Escape",
    });

    fireEvent.contextMenu(
      screen.getByTestId(`schedules-scheduled-${PENDING_JOB._id}`),
    );
    fireEvent.click(
      within(screen.getByTestId("schedules-row-menu")).getByTestId(
        "schedules-row-menu-cancel",
      ),
    );
    await waitFor(() =>
      expect(scheduleApiMock.cancel).toHaveBeenCalledWith("acme", "job-1"),
    );
    expect(toastMock.success).toHaveBeenCalledWith(
      "Cancelled documents.pings.insert",
    );
  });

  it("deletes a cron only after the confirm dialog", async () => {
    setSearchTo({ section: "cron" });
    useQueryMock.mockReturnValue([CRON]);
    scheduleApiMock.removeCron.mockResolvedValue({ ok: true, data: undefined });
    render(<SchedulesPage />);

    fireEvent.contextMenu(screen.getByTestId("schedules-cron-sweep"));
    fireEvent.click(
      within(screen.getByTestId("schedules-row-menu")).getByTestId(
        "schedules-row-menu-delete",
      ),
    );
    expect(scheduleApiMock.removeCron).not.toHaveBeenCalled();
    const dialog = await screen.findByTestId("schedules-delete-cron");
    expect(dialog).toHaveTextContent('Delete cron "sweep"?');

    fireEvent.click(screen.getByTestId("schedules-delete-cron-confirm"));
    await waitFor(() =>
      expect(scheduleApiMock.removeCron).toHaveBeenCalledWith("acme", "sweep"),
    );
    expect(toastMock.success).toHaveBeenCalledWith("Deleted cron sweep");
  });
});

describe("SchedulesPage sheet", () => {
  it("shows the job facts, mutation, and error for ?job=", () => {
    setSearchTo({ section: "scheduled", job: DONE_JOB._id });
    useQueryMock.mockReturnValue([PENDING_JOB, DONE_JOB]);
    render(<SchedulesPage />);

    const sheet = screen.getByTestId("schedules-sheet");
    expect(sheet).toHaveTextContent("documents.pings.delete");
    expect(
      within(sheet).getByTestId("schedules-sheet-status"),
    ).toHaveTextContent("failed");
    expect(within(sheet).getByTestId("schedules-sheet-args")).toHaveTextContent(
      '"table": "pings"',
    );
    expect(
      within(sheet).getByTestId("schedules-sheet-error"),
    ).toHaveTextContent("document p1 not found");
    // A finished job cannot be cancelled; it can be run again.
    expect(within(sheet).queryByTestId("schedules-sheet-cancel")).toBeNull();
    expect(within(sheet).getByTestId("schedules-sheet-run")).toBeTruthy();
  });

  it("runs the job from the sheet footer", async () => {
    setSearchTo({
      section: "scheduled",
      job: PENDING_JOB._id,
    });
    useQueryMock.mockReturnValue([PENDING_JOB]);
    render(<SchedulesPage />);

    fireEvent.click(screen.getByTestId("schedules-sheet-run"));
    await waitFor(() =>
      expect(scheduleApiMock.runNow).toHaveBeenCalledWith(
        "acme",
        PENDING_JOB.args,
      ),
    );
  });

  it("says when the addressed row is not in the list", () => {
    setSearchTo({ section: "cron", cron: "gone" });
    useQueryMock.mockReturnValue([CRON]);
    render(<SchedulesPage />);

    const sheet = screen.getByTestId("schedules-sheet");
    expect(sheet).toHaveTextContent("Not in this list");
    expect(within(sheet).getByTestId("schedules-sheet-missing")).toBeTruthy();
  });
});
