import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

const { navigateMock, routeState } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
  routeState: {
    params: { sandbox: "sb-api" },
    search: {} as { tab?: string },
  },
}));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => ({
    ...config,
    useParams: () => routeState.params,
  }),
  useSearch: () => routeState.search,
  useNavigate: () => navigateMock,
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

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: () => undefined,
  useSubPanelSearch: () => "",
}));

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

// The console has its own spec; here it is a stub that shows what it got.
vi.mock("./sandboxes/-sandbox-console", () => ({
  SandboxConsole: ({
    sandboxId,
    lifecycleState,
    testid,
  }: {
    sandboxId: string;
    lifecycleState: string;
    testid: string;
  }) => (
    <div
      data-testid={testid}
      data-sandbox={sandboxId}
      data-state={lifecycleState}
    />
  ),
}));

import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import { makeSandbox } from "../../test/sandbox-fixtures";
import { Route } from "./sandboxes_.$sandbox";

const Page = routeComponent(Route);

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
beforeEach(() => {
  navigateMock.mockReset();
  routeState.params = { sandbox: "sb-api" };
  routeState.search = {};
  useUiStore.setState({ activeTenant: "acme" });
});
afterEach(() => {
  server.resetHandlers();
  useUiStore.setState({ activeTenant: null });
});
afterAll(() => server.close());

const SANDBOX = makeSandbox({
  id: "sb-api",
  displayName: "api shell",
  endpoints: [{ name: "http", protocol: "tcp", host: "10.0.0.4", port: 8080 }],
  conditions: [
    {
      type: "Ready",
      status: "True",
      reason: "ProcessUp",
      message: "process is running",
      observedGeneration: 1,
      lastTransitionTime: "2026-09-08T10:00:00Z",
    },
  ],
  labels: { team: "core" },
});

function serve(sandbox = SANDBOX) {
  server.use(
    http.get("*/api/tenants/:t/sandboxes/:id", ({ params }) =>
      params.id === sandbox.metadata.id
        ? HttpResponse.json(sandbox)
        : HttpResponse.json(
            { error: { message: `sandbox ${String(params.id)} not found` } },
            { status: 404 },
          ),
    ),
    http.get("*/api/tenants/:t/sandboxes", () =>
      HttpResponse.json({ metadata: { tenantId: "acme" }, items: [sandbox] }),
    ),
  );
}

describe("developer/sandboxes/$sandbox", () => {
  it("shows the overview with the state pill, endpoints and conditions", async () => {
    serve();
    render(<Page />);
    expect(screen.getByTestId("sandbox-loading")).toBeInTheDocument();

    await screen.findByTestId("sandbox-tab-overview");
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent(
      "api shell",
    );
    expect(screen.getByTestId("sandbox-detail-state")).toHaveAttribute(
      "data-state",
      "ready",
    );
    expect(screen.getByTestId("sandbox-detail-id")).toBeInTheDocument();
    expect(screen.getByTestId("sandbox-endpoints")).toHaveTextContent(
      "10.0.0.4:8080",
    );
    const condition = screen.getByTestId("sandbox-condition-Ready");
    expect(condition).toHaveTextContent("ProcessUp");
    expect(condition).toHaveTextContent("process is running");
    expect(screen.getByTestId("sandbox-tab-overview")).toHaveTextContent(
      "team=core",
    );
    expect(
      screen.getByTestId("sandbox-detail-tab-console"),
    ).toBeInTheDocument();
  });

  it("mounts the console on the console tab with the sandbox and its state", async () => {
    routeState.search = { tab: "console" };
    serve();
    render(<Page />);
    const console = await screen.findByTestId("sandbox-console");
    expect(console).toHaveAttribute("data-sandbox", "sb-api");
    expect(console).toHaveAttribute("data-state", "ready");
  });

  it("shows the spec with the image reference and the redacted counts", async () => {
    routeState.search = { tab: "spec" };
    serve();
    render(<Page />);
    const spec = await screen.findByTestId("sandbox-tab-spec");
    expect(spec).toHaveTextContent("docker.io/library/alpine:3.20");
    expect(screen.getByTestId("sandbox-spec-process")).toHaveTextContent(
      "3 values",
    );
    expect(screen.getByTestId("sandbox-spec-process")).toHaveTextContent(
      "2 values, redacted",
    );
    expect(screen.getByTestId("sandbox-spec-process")).toHaveTextContent(
      "/work",
    );
  });

  it("says when the sandbox is not there", async () => {
    routeState.params = { sandbox: "ghost" };
    serve();
    render(<Page />);
    const missing = await screen.findByTestId("sandbox-not-found");
    expect(missing).toHaveTextContent("sandbox ghost not found");
  });

  it("stops the sandbox behind the confirmation and re-reads it", async () => {
    let reads = 0;
    const stops: string[] = [];
    server.use(
      http.get("*/api/tenants/:t/sandboxes/:id", () => {
        reads += 1;
        return HttpResponse.json(
          reads > 1
            ? makeSandbox({ id: "sb-api", lifecycleState: "stopped" })
            : SANDBOX,
        );
      }),
      http.get("*/api/tenants/:t/sandboxes", () =>
        HttpResponse.json({ metadata: { tenantId: "acme" }, items: [] }),
      ),
      http.post("*/api/tenants/:t/sandboxes/:id/stop", ({ request }) => {
        stops.push(new URL(request.url).pathname);
        return HttpResponse.json(SANDBOX, { status: 202 });
      }),
    );
    render(<Page />);
    await screen.findByTestId("sandbox-tab-overview");

    fireEvent.click(screen.getByTestId("sandbox-detail-stop"));
    expect(screen.getByTestId("sandboxes-stop-dialog")).toBeInTheDocument();
    expect(stops).toEqual([]);
    fireEvent.click(screen.getByTestId("sandboxes-stop-dialog-confirm"));

    await waitFor(() =>
      expect(stops).toEqual(["/api/tenants/acme/sandboxes/sb-api/stop"]),
    );
    await waitFor(() =>
      expect(screen.getByTestId("sandbox-detail-state")).toHaveAttribute(
        "data-state",
        "stopped",
      ),
    );
    expect(screen.getByTestId("sandbox-detail-stop")).toBeDisabled();
  });

  it("asks for a tenant before it reads", () => {
    useUiStore.setState({ activeTenant: null });
    render(<Page />);
    expect(screen.getByTestId("sandbox-no-tenant")).toBeInTheDocument();
  });
});
