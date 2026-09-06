'use strict';

const assert = require('assert');

assert.strictEqual(process.release.name, 'node');
assert.strictEqual(process.version, 'v24.20.0');
assert.strictEqual(process.versions.node, '24.20.0');
assert.strictEqual(process.versions.modules, '137');
assert.strictEqual(process.release.lts, 'Krypton');
