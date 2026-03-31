import { buildRuntimeBundleSource } from "./runtime_bundle_parts.mjs";

function generateRuntimeBundle(manifest) {
  return buildRuntimeBundleSource(JSON.stringify(manifest, null, 2));
}

export { generateRuntimeBundle };
