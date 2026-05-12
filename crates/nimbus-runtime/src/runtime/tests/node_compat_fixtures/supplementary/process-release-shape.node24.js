'use strict';

const assert = require('assert');

assert.strictEqual(process.release.name, 'node');
assert.match(process.version, /^v24\./);
assert.match(process.versions.node, /^24\./);
assert.strictEqual(process.release.lts, 'Krypton');
