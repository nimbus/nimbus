'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');

const script = vm.createScript('delete b');
let ctx = {};
Object.defineProperty(ctx, 'b', { configurable: false });
ctx = vm.createContext(ctx);
assert.strictEqual(script.runInContext(ctx), false);

ctx = new Proxy({}, {});
assert.strictEqual(typeof vm.runInNewContext('String', ctx), 'function');

