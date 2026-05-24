import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import {
  defaultRuntimeDiagnostics,
  defaultTenants,
  handlers,
} from "./handlers";

const server = setupServer(...handlers);

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe("msw handlers", () => {
  it("GET /api/tenants returns the seed tenant list", async () => {
    const res = await fetch("http://nimbus.test/api/tenants");
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual(defaultTenants);
  });

  it("POST /api/tenants without an id returns 400 with the error envelope", async () => {
    const res = await fetch("http://nimbus.test/api/tenants", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("validation.invalid");
  });

  it("POST /api/tenants with an id returns 201 echoing the id", async () => {
    const res = await fetch("http://nimbus.test/api/tenants", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: "demo" }),
    });
    expect(res.status).toBe(201);
    expect(await res.json()).toEqual({ id: "demo" });
  });

  it("GET /debug/runtime/metrics returns the runtime lane contract", async () => {
    const res = await fetch("http://nimbus.test/debug/runtime/metrics");
    expect(res.status).toBe(200);
    const body = (await res.json()) as typeof defaultRuntimeDiagnostics;

    expect(body.limits?.runtime_backend).toBe("v8");
    expect(body.limits?.memory_enforcement).toBe("v8_isolate_heap_limit");
    expect(body.lanes?.map((lane) => lane.lane_name)).toEqual([
      "default",
      "node20",
      "node22",
      "node24",
      "bun_jsc",
    ]);
    expect(body.lanes?.every((lane) => lane.executor_started === false)).toBe(
      true,
    );

    const bunLane = body.lanes?.find((lane) => lane.lane_name === "bun_jsc");
    expect(bunLane?.execution_adapter_state).toBe("not_linked");
    expect(bunLane?.limits?.runtime_backend).toBe("bun_jsc");
    expect(bunLane?.limits?.memory_enforcement).toBe("outer_quota_required");
    expect(bunLane?.limits?.tenant_budget?.memory_enforcement).toBe(
      "outer_quota_required",
    );
  });
});
