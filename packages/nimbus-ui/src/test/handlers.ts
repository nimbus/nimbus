import { HttpResponse, http } from "msw";

export type TenantsResponse = { tenants: string[] };

export const defaultTenants: TenantsResponse = {
  tenants: ["demo", "staging"],
};

export const handlers = [
  http.get("*/api/tenants", () => HttpResponse.json(defaultTenants)),

  http.post("*/api/tenants", async ({ request }) => {
    const body = (await request.json()) as { id?: string };
    if (!body.id) {
      return HttpResponse.json(
        {
          error: {
            code: "validation.invalid",
            message: "id is required",
            requestId: "test-1",
            timestamp: new Date().toISOString(),
            severity: "error",
            retryable: false,
          },
        },
        { status: 400 },
      );
    }
    return HttpResponse.json({ id: body.id }, { status: 201 });
  }),

  http.delete("*/api/tenants/:id", () => HttpResponse.json({ ok: true })),

  http.get("*/debug/license/status", () =>
    HttpResponse.json({
      tier: "community",
      mauCap: 500,
      mauCurrent: 0,
    }),
  ),

  http.get("*/debug/encryption/status", () =>
    HttpResponse.json({ status: "ok", keyFingerprint: "fp_abc123" }),
  ),

  http.get("*/debug/runtime/metrics", () =>
    HttpResponse.json({
      engine: "v8",
      runtime: "java_script",
      profile: "application",
      maxConcurrentRuns: 16,
      runTimeoutMs: 30_000,
      memoryLimitMb: 256,
    }),
  ),
];
