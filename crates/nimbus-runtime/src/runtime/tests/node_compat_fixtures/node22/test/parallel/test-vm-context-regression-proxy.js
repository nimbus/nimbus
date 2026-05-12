'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');

const ctx = new Proxy({}, {});
assert.strictEqual(typeof vm.runInNewContext('String', ctx), 'function');

