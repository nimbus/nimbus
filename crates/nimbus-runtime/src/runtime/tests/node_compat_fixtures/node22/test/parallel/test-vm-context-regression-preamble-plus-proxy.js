'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

let script = new vm.Script('"passed";');
let context = vm.createContext();
assert.strictEqual(script.runInContext(context), 'passed');

context = vm.createContext({ foo: 'bar', thing: 'lala' });
script = new vm.Script('foo = 3;');
assert.strictEqual(script.runInContext(context), 3);
assert.strictEqual(context.foo, 3);
assert.strictEqual(context.thing, 'lala');

const target = {};
const proxyContext = new Proxy(target, {});
assert.strictEqual(typeof vm.runInNewContext('String', proxyContext), 'function');
vm.runInNewContext('answer = 42', proxyContext);
assert.strictEqual(target.answer, 42);

