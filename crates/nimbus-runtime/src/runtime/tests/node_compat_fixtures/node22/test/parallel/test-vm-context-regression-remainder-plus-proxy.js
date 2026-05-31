'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

assert.throws(() => vm.runInNewContext('', null, 'some.js'), {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});

let context = {};
Object.defineProperty(context, 'b', { configurable: false });
context = vm.createContext(context);
assert.strictEqual(vm.createScript('delete b').runInContext(context), false);

const target = {};
const proxyContext = new Proxy(target, {});
vm.runInNewContext('answer = 42', proxyContext);
assert.strictEqual(target.answer, 42);

