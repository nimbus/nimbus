import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
  vi,
} from "vitest";

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => vi.fn(),
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
}));

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: () => undefined,
  useSubPanelSearch: () => "",
}));

const { toastMock } = vi.hoisted(() => ({
  toastMock: { success: vi.fn(), error: vi.fn() },
}));
vi.mock("sonner", () => ({ toast: toastMock }));

import type { DeployActivation, DeployHistory } from "../../lib/types/deploy";
import { routeComponent } from "../../test/route-internals";
import { Route } from "./deploys";

const Page = routeComponent(Route);

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => {
  server.resetHandlers();
  toastMock.success.mockReset();
  toastMock.error.mockReset();
});
afterAll(() => server.close());

const SHA_NOTES = "1".repeat(64);
const SHA_TODOS = "2".repeat(64);
const SHA_BOOT = "3".repeat(64);

const fns = (...paths: string[]) =>
  paths.map((path) => ({ path, kind: "query" }));

function activation(over: Partial<DeployActivation>): DeployActivation {
  return {
    sha256: SHA_NOTES,
    generation: 1,
    activatedAt: Date.now() - 60_000,
    actor: "deploy-admin",
    sourceRef: "deploy:generation:1",
    kind: "deploy",
    silo: "convex-team",
    retained: true,
    functions: [],
    ...over,
  };
}

// Newest first, as the server orders it: todos (active, gen 3) replaced
// notes (gen 2), which replaced the bundle the server started with (gen 0,
// never retained).
const HISTORY: DeployHistory = {
  active: SHA_TODOS,
  activations: [
    activation({
      sha256: SHA_TODOS,
      generation: 3,
      sourceRef: "deploy:generation:3",
      functions: fns("todos:list", "todos:add", "shared:ping"),
    }),
    activation({
      sha256: SHA_NOTES,
      generation: 2,
      sourceRef: "deploy:generation:2",
      functions: fns("notes:list", "shared:ping"),
    }),
    activation({
      sha256: SHA_BOOT,
      generation: 0,
      actor: "server",
      sourceRef: "startup",
      kind: "startup",
      silo: undefined,
      retained: false,
      functions: fns("boot:hello"),
    }),
  ],
};

function serveHistory(history: DeployHistory) {
  let reads = 0;
  server.use(
    http.get("*/api/admin/deploys", () => {
      reads += 1;
      return HttpResponse.json(history);
    }),
  );
  return () => reads;
}

describe("developer/deploys", () => {
  it("lists each activation newest first with its hash, kind, generation and function delta", async () => {
    serveHistory(HISTORY);
    render(<Page />);
    expect(screen.getByTestId("deploys-loading")).toBeInTheDocument();

    const table = await screen.findByTestId("deploys-table");
    expect(table).toHaveAttribute("aria-rowcount", "4");
    const rows = screen.getAllByTestId(/^deploys-row-\d+$/);
    expect(rows.map((row) => row.dataset.testid)).toEqual([
      "deploys-row-3",
      "deploys-row-2",
      "deploys-row-0",
    ]);
    expect(screen.getByTestId("deploys-sha-3")).toHaveTextContent(
      SHA_TODOS.slice(0, 12),
    );
    expect(screen.getByTestId("deploys-active-3")).toHaveAttribute(
      "data-state",
      "active",
    );
    expect(screen.queryByTestId("deploys-active-2")).toBeNull();
    expect(rows[0]).toHaveTextContent("deploy");
    expect(rows[0]).toHaveTextContent("deploy-admin");
    expect(rows[2]).toHaveTextContent("startup");
    expect(rows[2]).toHaveTextContent("server");
    // todos (3 paths) replaced notes (2): shared:ping stayed, todos:list
    // and todos:add came in, notes:list went out.
    expect(screen.getByTestId("deploys-functions-3")).toHaveTextContent(
      "3+2−1",
    );
    expect(screen.getByTestId("deploys-functions-0")).toHaveTextContent("1");
    expect(screen.getByTestId("deploys-functions-0")).not.toHaveTextContent(
      "+",
    );
  });

  it("says that deploys arrive through the CLI when there are none", async () => {
    serveHistory({ active: null, activations: [] });
    render(<Page />);
    const empty = await screen.findByTestId("deploys-empty");
    expect(empty).toHaveTextContent("No deploys yet");
    expect(empty).toHaveTextContent("nimbus deploy");
  });

  it("reports a refused read with the server's message", async () => {
    server.use(
      http.get("*/api/admin/deploys", () =>
        HttpResponse.json(
          { error: "local server access denied" },
          { status: 403 },
        ),
      ),
    );
    render(<Page />);
    const state = await screen.findByTestId("deploys-error");
    expect(state).toHaveTextContent("Deploy history did not load");
    expect(state).toHaveTextContent("local server access denied");
  });

  it("compares a selected bundle's function paths with the active one", async () => {
    serveHistory(HISTORY);
    render(<Page />);
    await screen.findByTestId("deploys-table");

    fireEvent.click(screen.getByTestId("deploys-row-actions-2"));
    const menu = screen.getByTestId("deploys-row-menu");
    expect(
      within(menu).getByTestId("deploys-row-menu-compare"),
    ).toHaveTextContent("Compare with active");
    fireEvent.click(within(menu).getByTestId("deploys-row-menu-compare"));

    const diff = screen.getByTestId("deploys-diff");
    expect(diff).toHaveTextContent("Generation 2");
    expect(screen.getByTestId("deploys-diff-added")).toHaveTextContent(
      "notes:list",
    );
    const removed = screen.getByTestId("deploys-diff-removed");
    expect(removed).toHaveTextContent("todos:add");
    expect(removed).toHaveTextContent("todos:list");
    expect(screen.getByTestId("deploys-diff-kept")).toHaveTextContent(
      "shared:ping",
    );

    // The active row shows its plain inventory instead of a diff.
    fireEvent.click(screen.getByTestId("deploys-row-actions-3"));
    fireEvent.click(
      within(screen.getByTestId("deploys-row-menu")).getByTestId(
        "deploys-row-menu-compare",
      ),
    );
    const inventory = screen.getByTestId("deploys-inventory");
    expect(inventory).toHaveTextContent("shared:ping");
    expect(inventory).toHaveTextContent("todos:add");
    expect(screen.queryByTestId("deploys-diff-added")).toBeNull();

    fireEvent.click(screen.getByTestId("deploys-diff-close"));
    expect(screen.queryByTestId("deploys-diff")).toBeNull();
  });

  it("rolls back only after the confirmation, then reads the history again", async () => {
    const reads = serveHistory(HISTORY);
    const posts: string[] = [];
    server.use(
      http.post("*/api/admin/deploys/:sha/rollback", ({ request }) => {
        posts.push(new URL(request.url).pathname);
        return HttpResponse.json({
          activated: true,
          generation: 4,
          previousGeneration: 3,
          sha256: SHA_NOTES,
        });
      }),
    );
    render(<Page />);
    await screen.findByTestId("deploys-table");

    fireEvent.click(screen.getByTestId("deploys-row-actions-2"));
    fireEvent.click(
      within(screen.getByTestId("deploys-row-menu")).getByTestId(
        "deploys-row-menu-rollback",
      ),
    );
    const dialog = screen.getByTestId("deploys-rollback-dialog");
    expect(dialog).toHaveTextContent(`Roll back to ${SHA_NOTES.slice(0, 12)}?`);
    // notes lacks todos:list and todos:add, and brings notes:list back.
    expect(screen.getByTestId("deploys-rollback-impact")).toHaveTextContent(
      "2 function paths stop resolving and 1 come back",
    );
    expect(posts).toEqual([]);

    fireEvent.click(screen.getByTestId("deploys-rollback-dialog-confirm"));
    await waitFor(() =>
      expect(posts).toEqual([`/api/admin/deploys/${SHA_NOTES}/rollback`]),
    );
    await waitFor(() => expect(reads()).toBe(2));
    expect(toastMock.success).toHaveBeenCalledWith(
      expect.stringContaining("generation 4"),
    );
    expect(screen.queryByTestId("deploys-rollback-dialog")).toBeNull();
  });

  it("keeps the dialog open with the server's refusal", async () => {
    serveHistory(HISTORY);
    server.use(
      http.post("*/api/admin/deploys/:sha/rollback", () =>
        HttpResponse.json(
          { error: "deploy artifact integrity check failed for bundle" },
          { status: 400 },
        ),
      ),
    );
    render(<Page />);
    await screen.findByTestId("deploys-table");
    fireEvent.click(screen.getByTestId("deploys-row-actions-2"));
    fireEvent.click(
      within(screen.getByTestId("deploys-row-menu")).getByTestId(
        "deploys-row-menu-rollback",
      ),
    );
    fireEvent.click(screen.getByTestId("deploys-rollback-dialog-confirm"));
    const dialog = await screen.findByTestId("deploys-rollback-dialog");
    await waitFor(() =>
      expect(dialog).toHaveTextContent("integrity check failed"),
    );
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("disables rollback on the active bundle and on a bundle without retained files", async () => {
    serveHistory(HISTORY);
    render(<Page />);
    await screen.findByTestId("deploys-table");

    fireEvent.click(screen.getByTestId("deploys-row-actions-3"));
    let item = within(screen.getByTestId("deploys-row-menu")).getByTestId(
      "deploys-row-menu-rollback",
    );
    expect(item).toBeDisabled();
    expect(item).toHaveTextContent("already active");
    fireEvent.keyDown(screen.getByTestId("deploys-row-menu"), {
      key: "Escape",
    });

    fireEvent.click(screen.getByTestId("deploys-row-actions-0"));
    item = within(screen.getByTestId("deploys-row-menu")).getByTestId(
      "deploys-row-menu-rollback",
    );
    expect(item).toBeDisabled();
    expect(item).toHaveTextContent("files not retained");
    fireEvent.click(item);
    expect(screen.queryByTestId("deploys-rollback-dialog")).toBeNull();
  });
});
