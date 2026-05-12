import assert from 'node:assert';
import { createRequire } from 'node:module';

import bridgeDefault, {
  exportedKind as importedEntryKind,
  featureKind as importedFeatureKind,
} from 'bridge-target';
import importedFeature from 'bridge-target/feature';

const require = createRequire(import.meta.url);
const requiredBridge = require('bridge-target');
const requiredFeature = require('bridge-target/feature');
const commonjsOnlyNamespace = await import(
  './fixtures/module-resolution-bridge/commonjs-only.cjs'
);

assert.strictEqual(importedEntryKind, 'esm-entry');
assert.strictEqual(importedFeatureKind, 'esm-feature');
assert.strictEqual(importedFeature, 'esm-feature');
assert.deepStrictEqual(bridgeDefault, {
  entry: 'esm-entry',
  feature: 'esm-feature',
});

assert.deepStrictEqual(requiredBridge, {
  exportedKind: 'cjs-entry',
  featureKind: 'cjs-feature',
});
assert.deepStrictEqual(requiredFeature, {
  kind: 'cjs-feature',
});
assert.ok(require.resolve('bridge-target').endsWith('cjs-entry.cjs'));
assert.ok(require.resolve('bridge-target/feature').endsWith('cjs-feature.cjs'));

assert.deepStrictEqual(commonjsOnlyNamespace.default, {
  mode: 'cjs-default-bridge',
  requiredBuiltin: 'x.js',
});
