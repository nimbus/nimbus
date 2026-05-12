'use strict';

const assert = require('assert');

assert.strictEqual(process.release.name, 'node');
assert.match(process.version, /^v20\./);
assert.match(process.versions.node, /^20\./);
assert.strictEqual(process.release.lts, 'Iron');
