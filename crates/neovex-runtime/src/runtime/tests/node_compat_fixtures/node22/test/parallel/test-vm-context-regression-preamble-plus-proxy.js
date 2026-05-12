'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');
const Script = vm.Script;

let script = new Script('"passed";');

let context = vm.createContext();
let result = script.runInContext(context);
assert.strictEqual(result, 'passed');

context = vm.createContext({ 'foo': 'bar', 'thing': 'lala' });
assert.strictEqual(context.foo, 'bar');
assert.strictEqual(context.thing, 'lala');

script = new Script('foo = 3;');
result = script.runInContext(context);
assert.strictEqual(context.foo, 3);
assert.strictEqual(context.thing, 'lala');

const ctx = new Proxy({}, {});
assert.strictEqual(typeof vm.runInNewContext('String', ctx), 'function');

