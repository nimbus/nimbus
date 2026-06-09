import assert from "node:assert/strict";

import {
  assertTenantBundleAdmission,
  collectTenantBundleAdmissionViolations,
  isTenantBundleOperatorOnlySpecifier,
} from "../module_specifiers.mjs";

async function runCapabilityBoundaryFixtures() {
  assert.equal(
    isTenantBundleOperatorOnlySpecifier("@nimbus/nimbus/transports/rest"),
    true,
  );
  assert.equal(
    isTenantBundleOperatorOnlySpecifier("@nimbus/nimbus/transports/host"),
    true,
  );
  assert.equal(
    isTenantBundleOperatorOnlySpecifier("@nimbus/nimbus/transports/grpc"),
    true,
  );
  assert.equal(isTenantBundleOperatorOnlySpecifier("nimbus/rest"), true);
  assert.equal(isTenantBundleOperatorOnlySpecifier("@nimbus/nimbus"), false);

  assert.throws(
    () =>
      assertTenantBundleAdmission(
        'import { NimbusRestClient } from "@nimbus/nimbus/transports/rest";\n',
        { file: "actions/control.ts" },
      ),
    /tenant bundle admission failed.*@nimbus\/nimbus\/transports\/rest.*operator-only/,
  );
  assert.throws(
    () =>
      assertTenantBundleAdmission(
        'const transport = await import("nimbus/rest");\n',
        { file: "actions/legacy.ts" },
      ),
    /tenant bundle admission failed.*nimbus\/rest.*operator-only/,
  );
  assert.throws(
    () =>
      assertTenantBundleAdmission(
        'import "@nimbus/nimbus/transports/host";\n',
        { file: "actions/host.ts" },
      ),
    /tenant bundle admission failed.*@nimbus\/nimbus\/transports\/host.*operator-only/,
  );
  assert.throws(
    () =>
      assertTenantBundleAdmission(
        'const token = process.env.NIMBUS_TOKEN; // operator credential\n',
        { file: "actions/token.ts" },
      ),
    /tenant bundle admission failed.*operator credential.*NIMBUS_TOKEN/,
  );
  assert.throws(
    () =>
      assertTenantBundleAdmission(
        "type Stored = LocalAdminTokenRecord;\n",
        { file: "actions/admin.ts" },
      ),
    /tenant bundle admission failed.*operator credential.*LocalAdminTokenRecord/,
  );

  const allowedSource = [
    'import { Nimbus } from "@nimbus/nimbus";',
    "const nimbus = new Nimbus();",
    "const workload = process.env.NIMBUS_WORKLOAD_IDENTITY_TOKEN;",
    'await nimbus.services.start({ name: "search", waitUntil: "ready" });',
  ].join("\n");
  assert.deepEqual(
    collectTenantBundleAdmissionViolations(allowedSource, {
      file: "actions/workload.ts",
    }),
    [],
    "high-level SDK tenant bundle admission should allow workload identity auth",
  );
}

export { runCapabilityBoundaryFixtures };
