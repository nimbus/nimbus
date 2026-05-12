'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');

const context = vm.createContext({ foo: 'bar', thing: 'lala' });

let gh1140Exception;
try {
  vm.runInContext('throw new Error()', context, 'expected-filename.js');
} catch (e) {
  gh1140Exception = e;
  assert.match(e.stack, /expected-filename/);
}

assert.strictEqual(gh1140Exception.toString(), 'Error');

