'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');
const Script = vm.Script;

let script = new Script('"passed";');

let context = vm.createContext();
assert.strictEqual(script.runInContext(context), 'passed');

context = vm.createContext({ 'foo': 'bar', 'thing': 'lala' });
script = new Script('foo = 3;');
assert.strictEqual(script.runInContext(context), 3);
assert.strictEqual(context.foo, 3);

script = vm.createScript('delete b');
let ctx = {};
Object.defineProperty(ctx, 'b', { configurable: false });
ctx = vm.createContext(ctx);
assert.strictEqual(script.runInContext(ctx), false);

ctx = new Proxy({}, {});
assert.strictEqual(typeof vm.runInNewContext('String', ctx), 'function');

