import { runHttpAuthErrorProbe } from "../support/auth-error.mjs";

// NIMBUS_TEST_TARGET selects either "nimbus" or "firebase_cloud".
const firestorePath =
  "/v1/projects/dual-target/databases/(default)/documents/__auth_probe__/missing";

const targets = {
  nimbus: {
    urlEnv: "NIMBUS_FIREBASE_DUAL_TARGET_URL",
    path: firestorePath,
  },
  firebase_cloud: {
    urlEnv: "FIREBASE_CLOUD_DUAL_TARGET_URL",
    path: firestorePath,
  },
};

await runHttpAuthErrorProbe({
  adapter: "firebase",
  targets,
  buildRequest: ({ target }) => ({
    method: "GET",
    path: target.path,
    headers: {
      authorization: "Bearer nimbus-dual-target-invalid-token",
      "x-goog-api-key": process.env.FIREBASE_DUAL_TARGET_API_KEY ?? "dual-target",
    },
  }),
  expect: {
    status: [401, 403],
    bodyPattern: /auth|token|credential|permission|unauth/i,
  },
});
