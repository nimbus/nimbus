'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

const target = {};
const context = new Proxy(target, {});

assert.strictEqual(typeof vm.runInNewContext('String', context), 'function');

vm.runInNewContext('answer = 42', context);
assert.strictEqual(target.answer, 42);

