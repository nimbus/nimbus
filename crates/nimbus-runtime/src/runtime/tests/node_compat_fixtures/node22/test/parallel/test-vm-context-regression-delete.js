'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

let script = vm.createScript('delete b');
let context = {};
Object.defineProperty(context, 'b', { configurable: false });
context = vm.createContext(context);
assert.strictEqual(script.runInContext(context), false);

context = vm.createContext();
const descriptor = vm.runInContext(`
  this.x = 'prop';
  delete this.x;
  Object.getOwnPropertyDescriptor(this, 'x');
`, context);
assert.strictEqual(descriptor, undefined);

