// The next action of an empty page is the one command that puts the first
// row on it. Every tenant-scoped empty state reads its command from here, so
// the commands share one server address, one token convention, and the names
// the quick start uses. A page passes the real tenant and table when it has
// them; the fallbacks are the quick-start names, so a pasted command still
// runs on a fresh server.
//
// Each command is a string the CopyButton copies whole. Multi-line commands
// end every line but the last with a backslash, so a paste runs as one.

export type NextActionContext = {
  // serverUrl is the origin of this server; empty falls back to the local
  // default port.
  serverUrl: string;
  // tenant is the active tenant, or null before one is chosen.
  tenant: string | null;
};

const LOCAL_SERVER = "http://localhost:3210";
const QUICK_START_TENANT = "demo";
const QUICK_START_TABLE = "messages";
const QUICK_START_FUNCTION = "messages:send";

const AUTH_HEADER = `-H "Authorization: Bearer $NIMBUS_TOKEN"`;
const JSON_HEADER = `-H "Content-Type: application/json"`;

function serverOf(serverUrl: string): string {
  return serverUrl || LOCAL_SERVER;
}

function tenantOf(tenant: string | null | undefined): string {
  return tenant ?? QUICK_START_TENANT;
}

function tableOf(table: string | null | undefined): string {
  return table ?? QUICK_START_TABLE;
}

function curlJson(method: "POST" | "PUT", url: string, body: string): string {
  return [
    `curl -s -X ${method} ${url} \\`,
    `  ${AUTH_HEADER} \\`,
    `  ${JSON_HEADER} \\`,
    `  -d '${body}'`,
  ].join("\n");
}

// createTenantCommand makes the first tenant. It takes no tenant because
// there is none yet; the id is the quick-start name.
export function createTenantCommand({
  serverUrl,
}: Pick<NextActionContext, "serverUrl">): string {
  return curlJson(
    "POST",
    `${serverOf(serverUrl)}/api/tenants`,
    `{"id": "${QUICK_START_TENANT}"}`,
  );
}

// insertDocumentCommand writes one document. A table exists once it holds a
// document, so this is also the command that creates a table.
export function insertDocumentCommand({
  serverUrl,
  tenant,
  table,
}: NextActionContext & { table: string | null }): string {
  return curlJson(
    "POST",
    `${serverOf(serverUrl)}/api/tenants/${tenantOf(tenant)}/documents`,
    `{"table": "${tableOf(table)}", "fields": {"body": "hello"}}`,
  );
}

// paginatedQueryCommand runs one document query from a shell. The Query
// tab shows it beside the request body, so the query an operator built in
// the console is the one a script or a teammate can run verbatim.
export function paginatedQueryCommand({
  serverUrl,
  tenant,
  body,
}: NextActionContext & { body: string }): string {
  return curlJson(
    "POST",
    `${serverOf(serverUrl)}/api/tenants/${tenantOf(tenant)}/query/paginated`,
    body,
  );
}

// deployFunctionsCommand deploys the app in the working directory and keeps
// deploying it on change. It is the same command the Compute page shows.
export function deployFunctionsCommand(): string {
  return "nimbus dev --app-dir .";
}

// callFunctionCommand invokes one function through the CLI, which is what
// records a run. The function is the page's own when it has one.
export function callFunctionCommand({
  serverUrl,
  tenant,
  functionPath,
}: NextActionContext & { functionPath: string | null }): string {
  const path = functionPath ?? QUICK_START_FUNCTION;
  return `nimbus run ${serverOf(serverUrl)} functions ${path} '{}' --tenant ${tenantOf(tenant)}`;
}

// scheduleJobCommand enqueues one mutation a minute out.
export function scheduleJobCommand({
  serverUrl,
  tenant,
  table,
}: NextActionContext & { table: string | null }): string {
  return curlJson(
    "POST",
    `${serverOf(serverUrl)}/api/tenants/${tenantOf(tenant)}/schedule`,
    `{"run_after_ms": 60000, "mutation": {"type": "insert", "table": "${tableOf(table)}", "fields": {"body": "later"}}}`,
  );
}

// createCronCommand registers one interval cron that inserts a heartbeat row.
export function createCronCommand({
  serverUrl,
  tenant,
  table,
}: NextActionContext & { table: string | null }): string {
  return curlJson(
    "POST",
    `${serverOf(serverUrl)}/api/tenants/${tenantOf(tenant)}/crons`,
    `{"name": "heartbeat", "schedule": {"type": "interval", "seconds": 300}, "mutation": {"type": "insert", "table": "${tableOf(table)}", "fields": {"body": "tick"}}}`,
  );
}

// uploadObjectCommand puts one file into a bucket. A bucket exists once it
// holds an object, so this is also the command that creates a bucket.
export function uploadObjectCommand({
  serverUrl,
  tenant,
}: NextActionContext): string {
  return [
    `curl -s -X PUT ${serverOf(serverUrl)}/api/tenants/${tenantOf(tenant)}/objects/assets/hello.txt \\`,
    `  ${AUTH_HEADER} \\`,
    `  --data-binary @hello.txt`,
  ].join("\n");
}
