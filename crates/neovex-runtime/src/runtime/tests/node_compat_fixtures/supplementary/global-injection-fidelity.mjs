import assert from 'node:assert';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

import * as esmShape from './fixtures/global-injection-fidelity/esm-shape.mjs';

const require = createRequire(import.meta.url);
const cjsShape = require('./fixtures/global-injection-fidelity/cjs-shape.cjs');
const expectedEsmFilename = fileURLToPath(
  new URL('./fixtures/global-injection-fidelity/esm-shape.mjs', import.meta.url),
);

assert.strictEqual(typeof globalThis.require, 'undefined');
assert.strictEqual(typeof __dirname, 'undefined');
assert.strictEqual(typeof __filename, 'undefined');

assert.strictEqual(esmShape.hasRequire, false);
assert.strictEqual(esmShape.hasDirname, false);
assert.strictEqual(esmShape.hasFilename, false);
assert.strictEqual(esmShape.metaFilename, expectedEsmFilename);
assert.strictEqual(path.basename(esmShape.metaDirname), 'global-injection-fidelity');

assert.strictEqual(cjsShape.hasRequire, true);
assert.strictEqual(cjsShape.requiredValue, 'cjs-required-ok');
assert.strictEqual(cjsShape.requiredBuiltinJoin, 'a/b');
assert.strictEqual(path.basename(cjsShape.dirname), 'global-injection-fidelity');
assert.strictEqual(path.basename(cjsShape.filename), 'cjs-shape.cjs');
