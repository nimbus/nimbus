'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');
const Script = vm.Script;

assert.throws(() => {
  vm.runInNewContext('', null, 'some.js');
}, {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});

const script = new Script('"passed";');

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
].forEach((entry) => {
  assert.throws(() => { script.runInContext(entry[0]); }, entry[1]);
  assert.throws(() => { vm.runInContext('', entry[0]); }, entry[1]);
});

vm.createScript(
  'const assert = require("assert"); assert.throws(function() { throw "hello world"; }, /hello/);',
  'some.js',
).runInNewContext({ require });

const deleteScript = vm.createScript('delete b');
let ctx = {};
Object.defineProperty(ctx, 'b', { configurable: false });
ctx = vm.createContext(ctx);
assert.strictEqual(deleteScript.runInContext(ctx), false);

