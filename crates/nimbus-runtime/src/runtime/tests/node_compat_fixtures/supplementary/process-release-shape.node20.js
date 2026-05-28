'use strict';

const assert = require('assert');

assert.strictEqual(process.release.name, 'node');
assert.strictEqual(process.version, 'v20.20.2');
assert.strictEqual(process.versions.node, '20.20.2');
assert.strictEqual(process.versions.modules, '115');
assert.strictEqual(process.release.lts, 'Iron');
