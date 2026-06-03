'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

const context = vm.createContext({ foo: 'bar', thing: 'lala' });
const script = new vm.Script(`
  foo = 3;
  thing = thing + '-ok';
  answer = foo + 39;
  answer;
`);

assert.strictEqual(script.runInContext(context), 42);
assert.strictEqual(context.foo, 3);
assert.strictEqual(context.thing, 'lala-ok');
assert.strictEqual(context.answer, 42);

