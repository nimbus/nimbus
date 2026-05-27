import { runHttpAuthErrorProbe } from "../support/auth-error.mjs";

// NIMBUS_TEST_TARGET selects either "nimbus" or "cloud_functions_cloud".
const targets = {
  nimbus: {
    urlEnv: "NIMBUS_CLOUD_FUNCTIONS_DUAL_TARGET_URL",
    path: "",
  },
  cloud_functions_cloud: {
    urlEnv: "CLOUD_FUNCTIONS_CLOUD_DUAL_TARGET_URL",
    path: "",
  },
};

await runHttpAuthErrorProbe({
  adapter: "cloud_functions",
  targets,
  buildRequest: ({ target }) => ({
    method: "POST",
    path: target.path,
    headers: {
      authorization: "Bearer nimbus-dual-target-invalid-token",
      "content-type": "application/json",
    },
    body: JSON.stringify({ data: { probe: "auth-error-fidelity" } }),
  }),
  expect: {
    status: [401, 403],
    bodyPattern: /auth|token|credential|permission|unauth/i,
  },
});
