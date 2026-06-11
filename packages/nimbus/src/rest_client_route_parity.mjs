// rest_client_route_parity: the LR7 anti-drift guard for the native REST
// client. Two assertions:
//
// 1. The client's NIMBUS_REST_ROUTES table deep-equals the checked-in
//    native_rest_routes.json manifest (which the nimbus-server
//    rest_client_route_parity test probes against the live router).
// 2. Every client method actually honors its table entry: each method is
//    invoked against a recording fetch and must emit exactly its entry's
//    verb and expanded path.
//
// A route changed on only one side — client table, client method, server
// router, or manifest — fails one of the three checks.

import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";

const MANIFEST_URL = new URL("./native_rest_routes.json", import.meta.url);

// Sample parameters used to expand each path template; values are chosen
// so encodeURIComponent is observable (the space in the cron name).
const SAMPLE_PARAMS = {
  tenant_id: "demo-tenant",
  table: "notes",
  document_id: "doc-1",
  job_id: "job-1",
  name: "daily report",
};

// How to call each client method with the sample parameters.
const METHOD_CALLS = {
  health: (client) => client.health(),
  createTenant: (client) => client.createTenant(SAMPLE_PARAMS.tenant_id),
  listTenants: (client) => client.listTenants(),
  deleteTenant: (client) => client.deleteTenant(SAMPLE_PARAMS.tenant_id),
  getSchema: (client) => client.getSchema(SAMPLE_PARAMS.tenant_id),
  getTableSchema: (client) => client.getTableSchema(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table),
  setTableSchema: (client) =>
    client.setTableSchema(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table, {
      table: SAMPLE_PARAMS.table,
      fields: [],
      indexes: [],
    }),
  deleteTableSchema: (client) =>
    client.deleteTableSchema(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table),
  insertDocument: (client) =>
    client.insertDocument(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table, { body: "hi" }),
  listDocuments: (client) => client.listDocuments(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table),
  getDocument: (client) =>
    client.getDocument(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table, SAMPLE_PARAMS.document_id),
  updateDocument: (client) =>
    client.updateDocument(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table, SAMPLE_PARAMS.document_id, {
      body: "hello",
    }),
  deleteDocument: (client) =>
    client.deleteDocument(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.table, SAMPLE_PARAMS.document_id),
  query: (client) =>
    client.query(SAMPLE_PARAMS.tenant_id, { table: SAMPLE_PARAMS.table, filters: [] }),
  queryPaginated: (client) =>
    client.queryPaginated(SAMPLE_PARAMS.tenant_id, {
      query: { table: SAMPLE_PARAMS.table, filters: [] },
      page_size: 1,
    }),
  readJournal: (client) => client.readJournal(SAMPLE_PARAMS.tenant_id),
  bootstrapJournal: (client) => client.bootstrapJournal(SAMPLE_PARAMS.tenant_id),
  scheduleMutation: (client) =>
    client.scheduleMutation(SAMPLE_PARAMS.tenant_id, {
      run_after_ms: 1,
      mutation: { type: "insert", table: SAMPLE_PARAMS.table, fields: {} },
    }),
  listScheduledJobs: (client) => client.listScheduledJobs(SAMPLE_PARAMS.tenant_id),
  cancelScheduledJob: (client) =>
    client.cancelScheduledJob(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.job_id),
  getScheduledJobResult: (client) =>
    client.getScheduledJobResult(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.job_id),
  createCronJob: (client) =>
    client.createCronJob(SAMPLE_PARAMS.tenant_id, {
      name: SAMPLE_PARAMS.name,
      schedule: { type: "interval", seconds: 60 },
      mutation: { type: "insert", table: SAMPLE_PARAMS.table, fields: {} },
    }),
  listCronJobs: (client) => client.listCronJobs(SAMPLE_PARAMS.tenant_id),
  deleteCronJob: (client) => client.deleteCronJob(SAMPLE_PARAMS.tenant_id, SAMPLE_PARAMS.name),
};

function expandTemplate(template) {
  return template.replace(/\{([a-z_]+)\}/g, (_, name) => {
    const value = SAMPLE_PARAMS[name];
    assert.notEqual(value, undefined, `manifest template ${template} uses unknown param ${name}`);
    return encodeURIComponent(value);
  });
}

export async function assertRestClientRouteParity(restBundlePath) {
  const manifest = JSON.parse(await fs.readFile(fileURLToPath(MANIFEST_URL), "utf8")).routes;
  const { NimbusRestClient, NIMBUS_REST_ROUTES } = await import(
    pathToFileURL(restBundlePath).href
  );

  // 1. Table ↔ manifest.
  assert.deepEqual(
    NIMBUS_REST_ROUTES,
    manifest,
    "rest_client_route_parity: NIMBUS_REST_ROUTES drifted from native_rest_routes.json",
  );

  // 2. Methods ↔ table, observed through a recording fetch.
  const names = Object.keys(manifest);
  assert.deepEqual(
    Object.keys(METHOD_CALLS).sort(),
    [...names].sort(),
    "rest_client_route_parity: METHOD_CALLS must cover exactly the manifest routes",
  );
  for (const name of names) {
    let observed = null;
    const client = new NimbusRestClient("http://nimbus.test", {
      fetch: async (input, init) => {
        observed = { url: String(input), method: init?.method ?? "GET" };
        return new Response("{}", {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      },
    });
    await METHOD_CALLS[name](client);
    const expected = manifest[name];
    assert.equal(
      observed.method,
      expected.verb,
      `rest_client_route_parity: ${name} sent ${observed.method}, manifest says ${expected.verb}`,
    );
    assert.equal(
      observed.url,
      `http://nimbus.test${expandTemplate(expected.path)}`,
      `rest_client_route_parity: ${name} hit ${observed.url}`,
    );
  }
  console.log(`  ✓ rest_client_route_parity verified (${names.length} routes)`);
}
