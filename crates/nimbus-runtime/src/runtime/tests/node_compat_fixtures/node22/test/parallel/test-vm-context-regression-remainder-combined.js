'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

assert.throws(() => vm.runInNewContext('', null, 'some.js'), {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});

vm.createScript(`
  const assert = require('assert');
  assert.throws(function() { throw 'hello world'; }, /hello/);
`, 'some.js').runInNewContext({ require });

let context = {};
Object.defineProperty(context, 'b', { configurable: false });
context = vm.createContext(context);
assert.strictEqual(vm.createScript('delete b').runInContext(context), false);

