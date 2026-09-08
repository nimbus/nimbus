import { describe, expect, it } from "vitest";

import {
  callFunctionCommand,
  createCronCommand,
  createTenantCommand,
  deployFunctionsCommand,
  insertDocumentCommand,
  scheduleJobCommand,
  uploadObjectCommand,
} from "./next-action";

const ctx = { serverUrl: "http://nimbus.example:9000", tenant: "acme" };

describe("next-action commands", () => {
  it("creates a tenant against the server's tenants route", () => {
    const cmd = createTenantCommand({ serverUrl: ctx.serverUrl });
    expect(cmd).toContain("POST http://nimbus.example:9000/api/tenants");
    expect(cmd).toContain('"Authorization: Bearer $NIMBUS_TOKEN"');
    expect(cmd).toContain('{"id": "demo"}');
  });

  it("inserts a document into the named table of the active tenant", () => {
    const cmd = insertDocumentCommand({ ...ctx, table: "messages" });
    expect(cmd).toContain(
      "http://nimbus.example:9000/api/tenants/acme/documents",
    );
    expect(cmd).toContain('"table": "messages"');
    expect(cmd).toContain('"fields":');
  });

  it("falls back to the quick-start names without a tenant or a table", () => {
    const cmd = insertDocumentCommand({
      serverUrl: "",
      tenant: null,
      table: null,
    });
    expect(cmd).toContain("http://localhost:3210/api/tenants/demo/documents");
    expect(cmd).toContain('"table": "messages"');
  });

  it("deploys through nimbus dev in the app directory", () => {
    expect(deployFunctionsCommand()).toBe("nimbus dev --app-dir .");
  });

  it("calls a function through the CLI, addressed to the tenant", () => {
    expect(callFunctionCommand({ ...ctx, functionPath: "messages:send" })).toBe(
      "nimbus run http://nimbus.example:9000 functions messages:send '{}' --tenant acme",
    );
    expect(callFunctionCommand({ ...ctx, functionPath: null })).toContain(
      "functions messages:send",
    );
  });

  it("schedules one mutation and creates one interval cron", () => {
    const job = scheduleJobCommand({ ...ctx, table: "messages" });
    expect(job).toContain(
      "http://nimbus.example:9000/api/tenants/acme/schedule",
    );
    expect(job).toContain('"run_after_ms": 60000');
    expect(job).toContain('"type": "insert"');
    expect(job).toContain('"table": "messages"');

    const cron = createCronCommand({ ...ctx, table: "messages" });
    expect(cron).toContain("http://nimbus.example:9000/api/tenants/acme/crons");
    expect(cron).toContain('"name": "heartbeat"');
    expect(cron).toContain('"type": "interval", "seconds": 300');
    expect(cron).toContain('"table": "messages"');
  });

  it("uploads one object through the tenant's object route", () => {
    const cmd = uploadObjectCommand(ctx);
    expect(cmd).toContain(
      "PUT http://nimbus.example:9000/api/tenants/acme/objects/assets/hello.txt",
    );
    expect(cmd).toContain("--data-binary @hello.txt");
  });

  it("writes every multi-line command with a trailing backslash per line", () => {
    for (const cmd of [
      createTenantCommand({ serverUrl: ctx.serverUrl }),
      insertDocumentCommand({ ...ctx, table: "messages" }),
      scheduleJobCommand({ ...ctx, table: "messages" }),
      createCronCommand({ ...ctx, table: "messages" }),
      uploadObjectCommand(ctx),
    ]) {
      const lines = cmd.split("\n");
      for (const line of lines.slice(0, -1)) expect(line).toMatch(/\\$/);
      expect(lines.at(-1)).not.toMatch(/\\$/);
    }
  });
});
