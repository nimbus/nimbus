'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

const context = vm.createContext({});
let thrown;

try {
  vm.runInContext('throw new Error("shared")', context, 'shared-context.js');
} catch (err) {
  thrown = err;
}

assert.strictEqual(thrown.message, 'shared');
assert.match(thrown.stack, /shared-context\.js/);
assert.throws(() => vm.runInContext('', {}), {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
  message: /The "contextifiedObject" argument must be an vm\.Context/,
});

