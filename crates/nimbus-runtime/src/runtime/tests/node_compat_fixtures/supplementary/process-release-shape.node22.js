'use strict';

const assert = require('assert');

assert.strictEqual(process.release.name, 'node');
assert.strictEqual(process.version, 'v22.22.3');
assert.strictEqual(process.versions.node, '22.22.3');
assert.strictEqual(process.versions.modules, '127');
assert.strictEqual(process.release.lts, 'Jod');
