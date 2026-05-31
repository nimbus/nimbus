'use strict';

require('../common');
const assert = require('assert');
const vm = require('vm');

let script = new vm.Script('"passed";');
let context = vm.createContext();
assert.strictEqual(script.runInContext(context), 'passed');

context = vm.createContext({ foo: 'bar', thing: 'lala' });
assert.strictEqual(context.foo, 'bar');
assert.strictEqual(context.thing, 'lala');

script = new vm.Script('foo = 3;');
assert.strictEqual(script.runInContext(context), 3);
assert.strictEqual(context.foo, 3);
assert.strictEqual(context.thing, 'lala');

assert.throws(() => vm.runInNewContext('', null, 'some.js'), {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});

let gh1140Exception;
try {
  vm.runInContext('throw new Error()', context, 'expected-filename.js');
} catch (err) {
  gh1140Exception = err;
  assert.match(err.stack, /expected-filename/);
}
assert.strictEqual(gh1140Exception.toString(), 'Error');

const nonContextualObjectError = {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
  message: /must be of type object/,
};
const contextifiedObjectError = {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
  message: /The "contextifiedObject" argument must be an vm\.Context/,
};

[
  [undefined, nonContextualObjectError],
  [null, nonContextualObjectError],
  [0, nonContextualObjectError],
  [0.0, nonContextualObjectError],
  ['', nonContextualObjectError],
  [{}, contextifiedObjectError],
  [[], contextifiedObjectError],
].forEach(([value, expected]) => {
  assert.throws(() => script.runInContext(value), expected);
  assert.throws(() => vm.runInContext('', value), expected);
});

vm.createScript(`
  const assert = require('assert');
  assert.throws(function() { throw 'hello world'; }, /hello/);
`, 'some.js').runInNewContext({ require });

script = vm.createScript('delete b');
let deleteContext = {};
Object.defineProperty(deleteContext, 'b', { configurable: false });
deleteContext = vm.createContext(deleteContext);
assert.strictEqual(script.runInContext(deleteContext), false);

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

