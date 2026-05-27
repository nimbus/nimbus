import assert from "node:assert/strict";

function envFlag(name) {
  return process.env[name] === "1" || process.env[name] === "true";
}

export function selectHttpTarget(adapter, targets) {
  const targetName = process.env.NIMBUS_TEST_TARGET ?? "nimbus";
  const target = targets[targetName];
  assert.ok(
    target,
    `${adapter} dual-target test does not define NIMBUS_TEST_TARGET=${targetName}. Known targets: ${Object.keys(targets).join(", ")}`,
  );

  const baseUrl = process.env[target.urlEnv] ?? "";
  const dryRun = envFlag("NIMBUS_DUAL_TARGET_DRY_RUN");
  if (!baseUrl && !dryRun) {
    throw new Error(
      `${adapter} ${targetName} target requires ${target.urlEnv}. Set NIMBUS_DUAL_TARGET_DRY_RUN=1 to validate only the target contract.`,
    );
  }

  return {
    adapter,
    baseUrl,
    dryRun,
    name: targetName,
    target,
  };
}

export function requestUrl(selected, path) {
  assert.ok(
    path === "" || path.startsWith("/"),
    `${selected.adapter} ${selected.name} request path must be absolute or empty`,
  );
  const base = selected.baseUrl.endsWith("/")
    ? selected.baseUrl
    : `${selected.baseUrl}/`;
  return new URL(path.replace(/^\//u, ""), base).toString();
}

export async function runHttpAuthErrorProbe({ adapter, targets, buildRequest, expect }) {
  const selected = selectHttpTarget(adapter, targets);
  const request = buildRequest(selected);
  assert.ok(request.method, "dual-target request must declare method");
  assert.ok(request.path !== undefined, "dual-target request must declare path");
  assert.ok(
    String(process.env.NIMBUS_TEST_TARGET ?? "nimbus").length > 0,
    "NIMBUS_TEST_TARGET must select the active dual target",
  );

  if (selected.dryRun) {
    console.log(
      `dual-target dry-run: ${adapter}/${selected.name} ${request.method} ${request.path} via ${selected.target.urlEnv}`,
    );
    return;
  }

  const response = await fetch(requestUrl(selected, request.path), {
    body: request.body,
    headers: request.headers,
    method: request.method,
  });
  await assertAuthErrorResponse(response, expect);
}

export async function assertAuthErrorResponse(
  response,
  { status = [401, 403], bodyPattern = /auth|token|credential|permission|unauth/i } = {},
) {
  assert.ok(
    status.includes(response.status),
    `expected auth failure status ${status.join(" or ")}, got ${response.status}`,
  );
  const body = await response.text();
  assert.match(
    body,
    bodyPattern,
    `auth failure body should identify an auth, token, credential, or permission problem: ${body}`,
  );
}
