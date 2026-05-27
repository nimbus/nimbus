import { runHttpAuthErrorProbe } from "../support/auth-error.mjs";

// NIMBUS_TEST_TARGET selects either "nimbus" or "convex_cloud".
const targets = {
  nimbus: {
    urlEnv: "NIMBUS_CONVEX_DUAL_TARGET_URL",
    path: "/query",
  },
  convex_cloud: {
    urlEnv: "CONVEX_CLOUD_DUAL_TARGET_URL",
    path: "/api/query",
  },
};

await runHttpAuthErrorProbe({
  adapter: "convex",
  targets,
  buildRequest: ({ target }) => ({
    method: "POST",
    path: target.path,
    headers: {
      authorization: "Bearer nimbus-dual-target-invalid-token",
      "content-type": "application/json",
    },
    body: JSON.stringify({
      args: {},
      format: "convex_encoded_json",
      name: "auth:whoami",
      path: "auth:whoami",
    }),
  }),
  expect: {
    status: [401, 403],
    bodyPattern: /auth|token|permission|unauth/i,
  },
});
