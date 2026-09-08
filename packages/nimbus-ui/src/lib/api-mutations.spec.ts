import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import {
  documents,
  machines,
  objects,
  schedules,
  schema,
  services,
  system,
  tenants,
} from "./api-mutations";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe("api-mutations core", () => {
  it("maps a JSON success body to { ok:true, data }", async () => {
    server.use(
      http.post("*/api/tenants", () =>
        HttpResponse.json({ id: "demo" }, { status: 201 }),
      ),
    );
    const result = await tenants.create("demo");
    expect(result).toEqual({ ok: true, data: { id: "demo" } });
  });

  it("maps an { error: { message } } envelope to { ok:false, error }", async () => {
    server.use(
      http.post("*/api/tenants/:t/documents", () =>
        HttpResponse.json(
          { error: { message: "table users is read-only" } },
          { status: 403 },
        ),
      ),
    );
    const result = await documents.insert("demo", "users", { a: 1 });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("table users is read-only");
      expect(result.status).toBe(403);
    }
  });

  it("maps a bare { error: string } envelope to { ok:false, error }", async () => {
    server.use(
      http.post("*/api/system/token/rotate", () =>
        HttpResponse.json({ error: "invalid bearer" }, { status: 401 }),
      ),
    );
    const result = await system.rotateToken("nope");
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toBe("invalid bearer");
  });

  it("yields a readable status fallback for a non-JSON error body", async () => {
    server.use(
      http.delete("*/api/tenants/:id", () =>
        HttpResponse.text("gateway timeout", { status: 502 }),
      ),
    );
    const result = await tenants.remove("demo");
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toBe("Request failed: 502");
  });

  it("maps a network failure to { ok:false } without throwing", async () => {
    server.use(
      http.post("*/api/tenants/:t/query/paginated", () => HttpResponse.error()),
    );
    const result = await documents.queryPaginated(
      "demo",
      { table: "users", filters: [], order: null, limit: null },
      25,
      null,
    );
    expect(result.ok).toBe(false);
  });
});

describe("api-mutations request shapes", () => {
  it("rotate sends the bearer Authorization header", async () => {
    let seen: string | null = null;
    server.use(
      http.post("*/api/system/token/rotate", ({ request }) => {
        seen = request.headers.get("authorization");
        return HttpResponse.json({ generation: 7 });
      }),
    );
    const result = await system.rotateToken("secret-token");
    expect(seen).toBe("Bearer secret-token");
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.data.generation).toBe(7);
  });

  it("document insert POSTs { table, fields } to the tenant documents route", async () => {
    let body: unknown = null;
    server.use(
      http.post("*/api/tenants/:t/documents", async ({ request, params }) => {
        body = await request.json();
        expect(params.t).toBe("demo");
        return HttpResponse.json({ _id: "x" });
      }),
    );
    await documents.insert("demo", "users", { name: "ada" });
    expect(body).toEqual({ table: "users", fields: { name: "ada" } });
  });

  it("document update PATCHes { patch } to the id-scoped route", async () => {
    let method: string | null = null;
    let body: unknown = null;
    server.use(
      http.patch(
        "*/api/tenants/:t/documents/:table/:id",
        async ({ request }) => {
          method = request.method;
          body = await request.json();
          return HttpResponse.json({ ok: true });
        },
      ),
    );
    await documents.update("demo", "users", "u1", { name: "grace" });
    expect(method).toBe("PATCH");
    expect(body).toEqual({ patch: { name: "grace" } });
  });

  it("schema put sends the raw schema object", async () => {
    let body: unknown = null;
    server.use(
      http.put("*/api/tenants/:t/schema/:table", async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ ok: true });
      }),
    );
    await schema.put("demo", "users", { fields: [{ name: "id" }] });
    expect(body).toEqual({ fields: [{ name: "id" }] });
  });

  it("machine action POSTs an empty body with the accept hint", async () => {
    let accept: string | null = null;
    let body: unknown = null;
    server.use(
      http.post("*/api/machines/:name/:action", async ({ request, params }) => {
        accept = request.headers.get("accept");
        body = await request.json();
        expect(params.action).toBe("restart");
        return HttpResponse.json({ ok: true });
      }),
    );
    await machines.action("vm-1", "restart");
    expect(accept).toBe("application/json");
    expect(body).toEqual({});
  });

  it("queryPaginated returns the page envelope as typed data", async () => {
    server.use(
      http.post("*/api/tenants/:t/query/paginated", () =>
        HttpResponse.json({
          data: [{ _id: "a" }],
          next_cursor: "c1",
          has_more: true,
        }),
      ),
    );
    const result = await documents.queryPaginated(
      "demo",
      { table: "users", filters: [], order: null, limit: null },
      25,
      null,
    );
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data.data).toHaveLength(1);
      expect(result.data.next_cursor).toBe("c1");
      expect(result.data.has_more).toBe(true);
    }
  });

  // The query travels as the engine spells it (`nimbus_core::query`): a
  // snake_case operator, a `direction` on the order, and the cursor as `after`
  // beside `page_size`. A renamed field here reads as an empty page, not as an
  // error.
  it("queryPaginated POSTs the filter, the order and the cursor as the engine names them", async () => {
    let body: unknown = null;
    server.use(
      http.post("*/api/tenants/:t/query/paginated", async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({
          data: [],
          next_cursor: null,
          has_more: false,
        });
      }),
    );
    await documents.queryPaginated(
      "demo",
      {
        table: "users",
        filters: [{ field: "author", op: "gte", value: 3 }],
        order: { field: "author", direction: "desc" },
        limit: null,
      },
      25,
      "cursor-1",
    );
    expect(body).toEqual({
      query: {
        table: "users",
        filters: [{ field: "author", op: "gte", value: 3 }],
        order: { field: "author", direction: "desc" },
        limit: null,
      },
      page_size: 25,
      after: "cursor-1",
    });
  });
});

describe("api-mutations services and schedules", () => {
  it("service start and stop POST no body to the lifecycle routes", async () => {
    const seen: Array<{ url: string; body: string }> = [];
    server.use(
      http.post(
        "*/api/tenants/:t/services/:name/:action",
        async ({ request }) => {
          seen.push({ url: request.url, body: await request.text() });
          return HttpResponse.json({ name: "api", state: "starting" });
        },
      ),
    );
    const start = await services.start("acme", "api");
    const stop = await services.stop("acme", "web api");
    expect(start).toEqual({
      ok: true,
      data: { name: "api", state: "starting" },
    });
    expect(stop.ok).toBe(true);
    expect(seen.map((s) => new URL(s.url).pathname)).toEqual([
      "/api/tenants/acme/services/api/start",
      "/api/tenants/acme/services/web%20api/stop",
    ]);
    expect(seen.map((s) => s.body)).toEqual(["", ""]);
  });

  it("service restart sends the source generation and request id in camelCase", async () => {
    let body: unknown;
    server.use(
      http.post(
        "*/api/tenants/:t/services/:name/restart",
        async ({ request }) => {
          body = await request.json();
          return HttpResponse.json(
            { request_id: "r1", disposition: "accepted" },
            { status: 202 },
          );
        },
      ),
    );
    const result = await services.restart("acme", "api", {
      sourceGeneration: 3,
      requestId: "r1",
    });
    expect(result.ok).toBe(true);
    expect(body).toEqual({ sourceGeneration: 3, requestId: "r1" });
  });

  it("runNow POSTs run_after_ms 0 with the mutation and returns the job id", async () => {
    let body: unknown;
    server.use(
      http.post("*/api/tenants/:t/schedule", async ({ request }) => {
        body = await request.json();
        return HttpResponse.json({ job_id: "job-9" }, { status: 201 });
      }),
    );
    const mutation = {
      type: "insert" as const,
      table: "pings",
      fields: { at: 1 },
    };
    const result = await schedules.runNow("acme", mutation);
    expect(result).toEqual({ ok: true, data: { job_id: "job-9" } });
    expect(body).toEqual({ run_after_ms: 0, mutation });
  });

  it("cancel DELETEs the job route and removeCron DELETEs the cron route", async () => {
    const urls: string[] = [];
    server.use(
      http.delete("*/api/tenants/:t/schedule/:job", ({ request }) => {
        urls.push(new URL(request.url).pathname);
        return new HttpResponse(null, { status: 204 });
      }),
      http.delete("*/api/tenants/:t/crons/:name", ({ request }) => {
        urls.push(new URL(request.url).pathname);
        return new HttpResponse(null, { status: 204 });
      }),
    );
    expect((await schedules.cancel("acme", "job:1")).ok).toBe(true);
    expect((await schedules.removeCron("acme", "sweep")).ok).toBe(true);
    expect(urls).toEqual([
      "/api/tenants/acme/schedule/job%3A1",
      "/api/tenants/acme/crons/sweep",
    ]);
  });

  it("listCrons GETs the tenant cron list as typed data", async () => {
    server.use(
      http.get("*/api/tenants/:t/crons", () =>
        HttpResponse.json({
          crons: [
            {
              name: "sweep",
              schedule: { type: "interval", seconds: 60 },
              mutation: { type: "insert", table: "pings", fields: {} },
              enabled: true,
            },
          ],
        }),
      ),
    );
    const result = await schedules.listCrons("acme");
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.data.crons?.[0]?.name).toBe("sweep");
  });
});

describe("api-mutations request-fidelity", () => {
  it("omits Content-Type on no-body writes but sends it when a body is present", async () => {
    let dropCt: string | null = "unset";
    let insertCt: string | null = "unset";
    server.use(
      http.delete("*/api/tenants/:t/schema/:table", ({ request }) => {
        dropCt = request.headers.get("content-type");
        return HttpResponse.json({}, { status: 200 });
      }),
      http.post("*/api/tenants/:t/documents", ({ request }) => {
        insertCt = request.headers.get("content-type");
        return HttpResponse.json({ _id: "x" }, { status: 201 });
      }),
    );
    await schema.drop("demo", "users");
    await documents.insert("demo", "users", { a: 1 });
    expect(dropCt).toBeNull(); // no-body DELETE advertises no JSON body
    expect(insertCt).toBe("application/json"); // body-carrying POST does
  });

  it("keeps machines on same-origin while other writes use include", async () => {
    // Contrast makes this non-vacuous: if the credentials mode weren't
    // propagated, tenants would not read back as "include".
    const creds: Record<string, string> = {};
    server.use(
      http.post("*/api/machines/:name/:action", ({ request }) => {
        creds.machines = request.credentials;
        return HttpResponse.json({}, { status: 200 });
      }),
      http.post("*/api/tenants", ({ request }) => {
        creds.tenants = request.credentials;
        return HttpResponse.json({ id: "demo" }, { status: 201 });
      }),
    );
    await machines.action("m1", "start");
    await tenants.create("demo");
    expect(creds.machines).toBe("same-origin");
    expect(creds.tenants).toBe("include");
  });

  it("sends the Authorization bearer header on token rotate", async () => {
    let auth: string | null = null;
    server.use(
      http.post("*/api/system/token/rotate", ({ request }) => {
        auth = request.headers.get("authorization");
        return HttpResponse.json({ token: "new" }, { status: 200 });
      }),
    );
    await system.rotateToken("old-token");
    expect(auth).toBe("Bearer old-token");
  });
});

describe("objects", () => {
  it("lists buckets and objects under a prefix on the native object route", async () => {
    let listUrl = "";
    server.use(
      http.get("*/api/tenants/:t/objects", () =>
        HttpResponse.json({
          buckets: [{ bucket: "assets", objectCount: 2, totalBytes: 12 }],
        }),
      ),
      http.get("*/api/tenants/:t/objects/:bucket", ({ request }) => {
        listUrl = request.url;
        return HttpResponse.json({
          bucket: "assets",
          prefix: "docs/",
          objects: [],
          truncated: false,
        });
      }),
    );
    const buckets = await objects.buckets("demo");
    expect(buckets).toEqual({
      ok: true,
      data: { buckets: [{ bucket: "assets", objectCount: 2, totalBytes: 12 }] },
    });
    const listing = await objects.list("demo", "assets", "docs/", 500);
    expect(listing.ok).toBe(true);
    expect(listUrl).toMatch(/\/api\/tenants\/demo\/objects\/assets\?/);
    expect(new URL(listUrl).searchParams.get("prefix")).toBe("docs/");
    expect(new URL(listUrl).searchParams.get("limit")).toBe("500");
  });

  it("keeps the slashes of a key in the address and escapes the rest", () => {
    expect(objects.url("demo", "assets", "docs/hello world.txt")).toBe(
      "/api/tenants/demo/objects/assets/docs/hello%20world.txt",
    );
    expect(objects.url("demo", "a b", "x#y.txt", { download: true })).toBe(
      "/api/tenants/demo/objects/a%20b/x%23y.txt?download=1",
    );
  });

  it("reads an object as text and reports a missing one by status", async () => {
    server.use(
      http.get("*/api/tenants/:t/objects/:bucket/hello.txt", () =>
        HttpResponse.text("hello", { status: 200 }),
      ),
      http.get("*/api/tenants/:t/objects/:bucket/gone.txt", () =>
        HttpResponse.json({ error: { message: "missing" } }, { status: 404 }),
      ),
    );
    expect(await objects.readText("demo", "assets", "hello.txt")).toEqual({
      ok: true,
      data: "hello",
    });
    const gone = await objects.readText("demo", "assets", "gone.txt");
    expect(gone.ok).toBe(false);
    if (!gone.ok) expect(gone.status).toBe(404);
  });

  it("deletes one object", async () => {
    server.use(
      http.delete("*/api/tenants/:t/objects/:bucket/docs/a.txt", () =>
        HttpResponse.json(null, { status: 204 }),
      ),
    );
    const result = await objects.remove("demo", "assets", "docs/a.txt");
    expect(result.ok).toBe(true);
  });
});
