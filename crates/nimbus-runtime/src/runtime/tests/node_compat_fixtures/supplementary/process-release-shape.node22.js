'use strict';

const assert = require('assert');

assert.strictEqual(process.release.name, 'node');
assert.match(process.version, /^v22\./);
assert.match(process.versions.node, /^22\./);
assert.strictEqual(process.release.lts, 'Jod');
