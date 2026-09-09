import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

const { navigateMock } = vi.hoisted(() => ({ navigateMock: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (config: Record<string, unknown>) => config,
  useNavigate: () => navigateMock,
  Link: ({
    to,
    children,
    "data-testid": testId,
    className,
    title,
  }: {
    to: string;
    children: React.ReactNode;
    "data-testid"?: string;
    className?: string;
    title?: string;
  }) => (
    <a href={to} data-testid={testId} className={className} title={title}>
      {children}
    </a>
  ),
}));

vi.mock("../../shell/sub-panel", () => ({
  useContributeSubPanel: () => undefined,
  useSubPanelSearch: () => "",
}));

vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

import { useUiStore } from "../../store/ui-store";
import { routeComponent } from "../../test/route-internals";
import { makeSandbox } from "../../test/sandbox-fixtures";
import { Route } from "./sandboxes";

const Page = routeComponent(Route);

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
beforeEach(() => {
  navigateMock.mockReset();
  useUiStore.setState({ activeTenant: "acme" });
});
afterEach(() => {
  server.resetHandlers();
  useUiStore.setState({ activeTenant: null });
});
afterAll(() => server.close());

const READY = makeSandbox({ id: "sb-api", displayName: "api shell" });
const STOPPED = makeSandbox({
  id: "sb-old",
  profile: "desktop",
  backend: "container",
  lifecycleState: "stopped",
  readiness: "stopped",
  health: "unknown",
});

function serveList(items: ReturnType<typeof makeSandbox>[]) {
  let reads = 0;
  server.use(
    http.get("*/api/tenants/:t/sandboxes", () => {
      reads += 1;
      return HttpResponse.json({ metadata: { tenantId: "acme" }, items });
    }),
  );
  return () => reads;
}

describe("developer/sandboxes", () => {
  it("lists each sandbox with its lifecycle pill, profile, backend and health", async () => {
    serveList([READY, STOPPED]);
    render(<Page />);
    expect(screen.getByTestId("sandboxes-loading")).toBeInTheDocument();

    const table = await screen.findByTestId("sandboxes-table");
    expect(table).toHaveAttribute("aria-rowcount", "3");
    expect(screen.getByTestId("sandboxes-link-sb-api")).toHaveTextContent(
      "api shell",
    );
    expect(screen.getByTestId("sandboxes-link-sb-old")).toHaveTextContent(
      "sb-old",
    );
    expect(screen.getByTestId("sandboxes-state-sb-api")).toHaveAttribute(
      "data-state",
      "ready",
    );
    expect(screen.getByTestId("sandboxes-state-sb-old")).toHaveAttribute(
      "data-state",
      "stopped",
    );
    const row = screen.getByTestId("sandboxes-row-sb-old");
    expect(row).toHaveTextContent("desktop");
    expect(row).toHaveTextContent("container");
    expect(row).toHaveTextContent("unknown");
    expect(screen.getByTestId("sandboxes-scope")).toHaveTextContent("acme");
  });

  it("names the missing routes when the server has no service manager", async () => {
    server.use(
      http.get("*/api/tenants/:t/sandboxes", () =>
        HttpResponse.json(
          {
            error: {
              code: "service.route_not_found",
              message: "no such route",
            },
          },
          { status: 404 },
        ),
      ),
    );
    render(<Page />);
    const state = await screen.findByTestId("sandboxes-unavailable");
    expect(state).toHaveTextContent("Sandbox routes not available");
    expect(state).toHaveTextContent("no such route");
    expect(screen.getByTestId("sandboxes-create")).toBeDisabled();
  });

  it("asks for a tenant before it reads anything", () => {
    useUiStore.setState({ activeTenant: null });
    render(<Page />);
    expect(screen.getByTestId("sandboxes-no-tenant")).toBeInTheDocument();
    expect(screen.queryByTestId("sandboxes-create")).toBeNull();
  });

  it("offers New sandbox from the empty state", async () => {
    serveList([]);
    render(<Page />);
    const empty = await screen.findByTestId("sandboxes-empty");
    expect(empty).toHaveTextContent("No live sandboxes");
    fireEvent.click(within(empty).getByRole("button", { name: "New sandbox" }));
    expect(screen.getByTestId("sandbox-create-dialog")).toBeInTheDocument();
  });

  it("stops a sandbox only after the confirmation, then reads the list again", async () => {
    const reads = serveList([READY, STOPPED]);
    const stops: string[] = [];
    server.use(
      http.post("*/api/tenants/:t/sandboxes/:id/stop", ({ request }) => {
        stops.push(new URL(request.url).pathname);
        return HttpResponse.json(
          makeSandbox({ id: "sb-api", lifecycleState: "stopping" }),
          { status: 202 },
        );
      }),
    );
    render(<Page />);
    await screen.findByTestId("sandboxes-table");

    // A stopped sandbox has no stop action: there is no way back.
    fireEvent.click(screen.getByTestId("sandboxes-row-actions-sb-old"));
    let menu = screen.getByTestId("sandboxes-row-menu");
    expect(within(menu).getByTestId("sandboxes-row-menu-open")).toBeTruthy();
    expect(within(menu).getByTestId("sandboxes-row-menu-console")).toBeTruthy();
    expect(within(menu).queryByTestId("sandboxes-row-menu-stop")).toBeNull();
    fireEvent.keyDown(menu, { key: "Escape" });

    fireEvent.click(screen.getByTestId("sandboxes-row-actions-sb-api"));
    menu = screen.getByTestId("sandboxes-row-menu");
    fireEvent.click(within(menu).getByTestId("sandboxes-row-menu-stop"));
    const dialog = screen.getByTestId("sandboxes-stop-dialog");
    expect(dialog).toHaveTextContent("Stop api shell?");
    expect(stops).toEqual([]);

    fireEvent.click(screen.getByTestId("sandboxes-stop-dialog-confirm"));
    await waitFor(() =>
      expect(stops).toEqual(["/api/tenants/acme/sandboxes/sb-api/stop"]),
    );
    await waitFor(() => expect(reads()).toBe(2));
  });

  it("opens the console from the row menu", async () => {
    serveList([READY]);
    render(<Page />);
    await screen.findByTestId("sandboxes-table");
    fireEvent.click(screen.getByTestId("sandboxes-row-actions-sb-api"));
    fireEvent.click(screen.getByTestId("sandboxes-row-menu-console"));
    expect(navigateMock).toHaveBeenCalledWith({
      to: "/developer/sandboxes/$sandbox",
      params: { sandbox: "sb-api" },
      search: { tab: "console" },
    });
  });

  it("creates a sandbox from the picker and opens it", async () => {
    const user = userEvent.setup();
    serveList([]);
    const bodies: unknown[] = [];
    server.use(
      http.post("*/api/tenants/:t/sandboxes", async ({ request }) => {
        bodies.push(await request.json());
        return HttpResponse.json(makeSandbox({ id: "scratch-1" }), {
          status: 201,
        });
      }),
    );
    render(<Page />);
    await screen.findByTestId("sandboxes-empty");
    await user.click(screen.getByTestId("sandboxes-create"));

    // An empty form never reaches the server; the dialog names the field.
    await user.click(screen.getByTestId("sandbox-create-dialog-confirm"));
    expect(screen.getByTestId("sandbox-create-dialog")).toHaveTextContent(
      "The id needs",
    );
    expect(bodies).toEqual([]);

    await user.click(screen.getByTestId("sandbox-create-profile"));
    await user.click(
      screen.getByTestId("sandbox-create-profile-option-desktop"),
    );
    await user.click(screen.getByTestId("sandbox-create-backend"));
    await user.click(
      screen.getByTestId("sandbox-create-backend-option-container"),
    );
    fireEvent.change(screen.getByTestId("sandbox-create-id"), {
      target: { value: "scratch-1" },
    });
    fireEvent.change(screen.getByTestId("sandbox-create-name"), {
      target: { value: "scratch shell" },
    });
    fireEvent.change(screen.getByTestId("sandbox-create-image"), {
      target: { value: "docker.io/library/alpine:3.20" },
    });
    fireEvent.change(screen.getByTestId("sandbox-create-command"), {
      target: { value: "/bin/sh\n-c\nsleep 30\n" },
    });
    await user.click(screen.getByTestId("sandbox-create-dialog-confirm"));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toEqual({
      id: "scratch-1",
      profile: "desktop",
      spec: {
        owner: { kind: "standalone", displayName: "scratch shell" },
        backend: "container",
        root: {
          kind: "oci_image",
          source: {
            kind: "reference",
            reference: "docker.io/library/alpine:3.20",
          },
        },
        process: { argv: ["/bin/sh", "-c", "sleep 30"] },
      },
    });
    await waitFor(() =>
      expect(navigateMock).toHaveBeenCalledWith({
        to: "/developer/sandboxes/$sandbox",
        params: { sandbox: "scratch-1" },
      }),
    );
  });
});
