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
  useNavigate: () => vi.fn(),
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useNimbus: () => ({ url: "http://nimbus.example:9000/convex/_nimbus" }),
}));

vi.mock("../../../hooks/use-tenant-list", () => ({
  useTenantList: () => ({
    kind: "loaded",
    tenants: [{ id: "acme" }, { id: "beta" }],
    reload: () => {},
  }),
}));

import { ERROR_GROUP_LIMIT, ErrorsTab, errorGroupsPath } from "./-errors";
import type { ObservabilityTabProps } from "./-facets";
import type { ErrorGroup, ErrorGroupPage, ObservabilitySearch } from "./-types";

// The groups are an HTTP read, not a subscription, so they go through msw.
const server = setupServer();
beforeAll(() => server.listen({ onUnhandledRequest: "bypass" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const NOW = 1_700_000_000_000;

const THROWN: ErrorGroup = {
  fingerprint: "0123456789abcdef",
  tenantId: "acme",
  functionPath: "messages:send",
  kind: "mutation",
  class: "function_thrown",
  message: "text is required",
  location: "messages:12",
  count: 2,
  firstSeen: NOW + 1_000,
  lastSeen: NOW + 2_000,
  latestRunId: "run-2",
};

const INVALID: ErrorGroup = {
  fingerprint: "fedcba9876543210",
  tenantId: "beta",
  functionPath: "agent:tick",
  kind: "action",
  class: "invalid_input",
  message: "text is required",
  location: null,
  count: 1,
  firstSeen: NOW + 3_000,
  lastSeen: NOW + 3_000,
  latestRunId: "run-3",
};

function answer(page: Partial<ErrorGroupPage>, onRequest?: (url: URL) => void) {
  server.use(
    http.get("*/api/console/errors", ({ request }) => {
      onRequest?.(new URL(request.url));
      return HttpResponse.json({
        groups: [],
        scanned: 0,
        exhaustive: true,
        limit: ERROR_GROUP_LIMIT,
        ...page,
      });
    }),
  );
}

function renderTab(
  search: ObservabilitySearch = { tab: "errors" },
  overrides: Partial<ObservabilityTabProps> = {},
) {
  const props: ObservabilityTabProps = {
    search,
    tenantId: "acme",
    allowAllTenants: false,
    setSearch: vi.fn(),
    setSearchAction: vi.fn(),
    ...overrides,
  };
  render(<ErrorsTab {...props} />);
  return props;
}

describe("error group read", () => {
  it("scopes the read to the tenant and asks for the server's page size", () => {
    expect(errorGroupsPath("acme")).toBe(
      `/api/console/errors?tenant=acme&limit=${ERROR_GROUP_LIMIT}`,
    );
    expect(errorGroupsPath(null)).toBe(
      `/api/console/errors?limit=${ERROR_GROUP_LIMIT}`,
    );
  });

  it("sends the tenant to the server rather than filtering on the client", async () => {
    const seen: URL[] = [];
    answer({ groups: [THROWN], scanned: 2 }, (url) => seen.push(url));
    renderTab();

    await screen.findByTestId("observability-error-row-0123456789abcdef");
    expect(seen[0]?.searchParams.get("tenant")).toBe("acme");
  });
});

describe("ErrorsTab table", () => {
  it("shows one row per group with its count, class, message, and location", async () => {
    answer({ groups: [INVALID, THROWN], scanned: 3 });
    renderTab({ tab: "errors" }, { tenantId: null, allowAllTenants: true });

    const row = await screen.findByTestId(
      "observability-error-row-0123456789abcdef",
    );
    expect(row).toHaveTextContent("messages:send");
    expect(row).toHaveTextContent("function_thrown");
    expect(row).toHaveTextContent("text is required");
    expect(row).toHaveTextContent("messages:12");
    expect(row).toHaveTextContent("2");
    expect(row).toHaveTextContent("acme");
    expect(
      screen.getByTestId("observability-error-row-fedcba9876543210"),
    ).toHaveTextContent("invalid_input");
  });

  it("drills a group into the Runs tab narrowed to its fingerprint", async () => {
    answer({ groups: [THROWN], scanned: 2 });
    const props = renderTab();

    fireEvent.click(
      await screen.findByTestId("observability-error-row-0123456789abcdef"),
    );
    expect(props.setSearchAction).toHaveBeenCalledWith({
      tab: "runs",
      fingerprint: "0123456789abcdef",
      status: undefined,
      functionPath: undefined,
      run: undefined,
    });
  });

  it("says how many failed runs the groups fold, and when older ones were left out", async () => {
    answer({ groups: [THROWN], scanned: 2, exhaustive: true });
    renderTab();
    expect(
      await screen.findByTestId("observability-error-scan"),
    ).toHaveTextContent("2 failed runs grouped");
  });

  it("warns when the scan window ended before the oldest failure", async () => {
    answer({ groups: [THROWN], scanned: 2000, exhaustive: false });
    renderTab();
    expect(
      await screen.findByTestId("observability-error-scan"),
    ).toHaveTextContent("older failures are not shown");
  });
});

describe("ErrorsTab read states", () => {
  it("distinguishes a read in flight from a settled empty result", () => {
    answer({ groups: [] });
    renderTab();

    expect(screen.getByTestId("observability-errors-table")).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(screen.queryByTestId("observability-errors-empty")).toBeNull();
  });

  it("gives the empty tab a title and a body that names the tenant", async () => {
    answer({ groups: [] });
    renderTab();

    const title = await screen.findByTestId("observability-errors-empty-title");
    expect(title).toHaveTextContent("No failed runs");
    expect(
      screen.getByTestId("observability-errors-empty-body"),
    ).toHaveTextContent("acme");
  });

  it("reports a failed read and retries it on demand", async () => {
    let calls = 0;
    server.use(
      http.get("*/api/console/errors", () => {
        calls += 1;
        return calls === 1
          ? HttpResponse.json({ error: "engine unavailable" }, { status: 503 })
          : HttpResponse.json({
              groups: [THROWN],
              scanned: 2,
              exhaustive: true,
              limit: ERROR_GROUP_LIMIT,
            });
      }),
    );
    renderTab();

    const failed = await screen.findByTestId("observability-errors-failed");
    fireEvent.click(within(failed).getByRole("button", { name: /try again/i }));
    await waitFor(() =>
      expect(
        screen.getByTestId("observability-error-row-0123456789abcdef"),
      ).toBeInTheDocument(),
    );
    expect(calls).toBe(2);
  });
});
