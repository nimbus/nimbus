'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');
const Script = vm.Script;

let script = new Script('"passed";');
let context = vm.createContext({ foo: 'bar', thing: 'lala' });
assert.strictEqual(script.runInContext(context), 'passed');

let gh1140Exception;
try {
  vm.runInContext('throw new Error()', context, 'expected-filename.js');
} catch (e) {
  gh1140Exception = e;
  assert.match(e.stack, /expected-filename/);
}
assert.strictEqual(gh1140Exception.toString(), 'Error');

let stack = null;
assert.throws(() => {
  vm.runInContext(' throw new Error()', context, {
    filename: 'expected-filename.js',
    lineOffset: 32,
    columnOffset: 123,
  });
}, (err) => {
  stack = err.stack;
  return /^ \^/m.test(stack) &&
    /expected-filename\.js:33:131/.test(stack);
}, `stack not formatted as expected: ${stack}`);

