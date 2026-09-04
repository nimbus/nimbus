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
    const expectedLanes = [
      [
        "default",
        "v8",
        "web_standard_isolate",
        "linked",
        "v8_isolate_heap_limit",
      ],
      ["node20", "v8", "node20", "linked", "v8_isolate_heap_limit"],
      ["node22", "v8", "node22", "linked", "v8_isolate_heap_limit"],
      ["node24", "v8", "node24", "linked", "v8_isolate_heap_limit"],
      ["bun_jsc", "bun_jsc", "bun_jsc", "not_linked", "outer_quota_required"],
    ] as const;
    for (const [
      laneName,
      runtimeBackend,
      compatibilityTarget,
      executionAdapterState,
      memoryEnforcement,
    ] of expectedLanes) {
      const lane = body.lanes?.find((item) => item.lane_name === laneName);
      expect(lane?.executor_started).toBe(false);
      expect(lane?.execution_adapter_state).toBe(executionAdapterState);
      expect(lane?.execution_adapter_artifact?.status).toBe(
        executionAdapterState,
      );
      expect(lane?.limits?.runtime_backend).toBe(runtimeBackend);
      expect(lane?.limits?.compatibility_target).toBe(compatibilityTarget);
      expect(lane?.limits?.memory_enforcement).toBe(memoryEnforcement);
      expect(lane?.limits?.tenant_budget?.memory_enforcement).toBe(
        memoryEnforcement,
      );
    }
    const bunLane = body.lanes?.find((item) => item.lane_name === "bun_jsc");
    expect(bunLane?.execution_adapter_artifact?.source).toBe(
      "build_feature_disabled",
    );
    expect(bunLane?.execution_adapter_artifact?.expected?.source_ref).toBe(
      "bun-v1.4.0-nimbus.7",
    );
    expect(
      bunLane?.execution_adapter_artifact?.expected?.source_revision,
    ).toBe("eb9d7eba81616b4008a595ce942f1b3ea71041a6");
  });
});
