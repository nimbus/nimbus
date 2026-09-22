import { act, renderHook, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import { makeSandbox } from "../../../test/sandbox-fixtures";
import { useSandbox, useSandboxList } from "./-sandbox-read";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

// A Response body reads once, so each request gets a fresh 404. The two 404s
// these pages meet are different facts: the service-control routes are not
// mounted on this server, or a sandbox id names nothing. The server gives each
// its own code; the pages branch on the status and show the message.
const routeNotFound = () =>
  HttpResponse.json(
    {
      error: {
        code: "service.route_not_found",
        message: "no route matches GET /api/tenants/acme/sandboxes",
      },
    },
    { status: 404 },
  );

const resourceNotFound = () =>
  HttpResponse.json(
    { error: { code: "op.not_found", message: "no sandbox `ghost`" } },
    { status: 404 },
  );

describe("useSandboxList", () => {
  it("maps an unmounted route to the unavailable state with the server's message", async () => {
    server.use(http.get("*/api/tenants/:t/sandboxes", routeNotFound));
    const { result } = renderHook(() => useSandboxList("acme", 0, 20));
    await waitFor(() =>
      expect(result.current).toEqual({
        kind: "ok",
        value: {
          kind: "unavailable",
          message: "no route matches GET /api/tenants/acme/sandboxes",
        },
      }),
    );
  });

  it("polls while a sandbox is transitional and stops once every row settles", async () => {
    let reads = 0;
    server.use(
      http.get("*/api/tenants/:t/sandboxes", () => {
        reads += 1;
        const state = reads < 3 ? "starting" : "ready";
        return HttpResponse.json({
          metadata: { tenantId: "acme" },
          items: [makeSandbox({ lifecycleState: state })],
        });
      }),
    );
    const { result } = renderHook(() => useSandboxList("acme", 0, 20));
    await waitFor(() => {
      const read = result.current;
      expect(read.kind).toBe("ok");
      if (read.kind !== "ok" || read.value.kind !== "list") throw new Error();
      expect(read.value.items[0]?.status.lifecycleState).toBe("ready");
    });
    expect(reads).toBe(3);
    const settled = reads;
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(reads).toBe(settled);
  });

  it("reads again when the caller bumps the revision", async () => {
    let reads = 0;
    server.use(
      http.get("*/api/tenants/:t/sandboxes", () => {
        reads += 1;
        return HttpResponse.json({ metadata: { tenantId: "acme" }, items: [] });
      }),
    );
    const { result, rerender } = renderHook(
      ({ revision }) => useSandboxList("acme", revision, 20),
      { initialProps: { revision: 0 } },
    );
    await waitFor(() => expect(result.current.kind).toBe("ok"));
    expect(reads).toBe(1);
    await act(async () => {
      rerender({ revision: 1 });
    });
    await waitFor(() => expect(reads).toBe(2));
  });
});

describe("useSandbox", () => {
  it("maps an absent sandbox to the missing state and a found one to its resource", async () => {
    server.use(
      http.get("*/api/tenants/:t/sandboxes/:id", ({ params }) =>
        params.id === "ghost"
          ? resourceNotFound()
          : HttpResponse.json(makeSandbox({ id: String(params.id) })),
      ),
    );
    const missing = renderHook(() => useSandbox("acme", "ghost", 0, 20));
    await waitFor(() =>
      expect(missing.result.current).toEqual({
        kind: "ok",
        value: { kind: "missing", message: "no sandbox `ghost`" },
      }),
    );
    const found = renderHook(() => useSandbox("acme", "sb-9", 0, 20));
    await waitFor(() => {
      const read = found.result.current;
      if (read.kind !== "ok" || read.value.kind !== "sandbox")
        throw new Error();
      expect(read.value.sandbox.metadata.id).toBe("sb-9");
    });
  });
});
