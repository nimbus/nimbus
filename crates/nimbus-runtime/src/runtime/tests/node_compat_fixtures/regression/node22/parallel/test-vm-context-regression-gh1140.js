'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

const context = vm.createContext({});
let exception;

try {
  vm.runInContext('throw new Error()', context, 'expected-filename.js');
} catch (err) {
  exception = err;
  assert.match(err.stack, /expected-filename/);
}

assert.strictEqual(exception.toString(), 'Error');

