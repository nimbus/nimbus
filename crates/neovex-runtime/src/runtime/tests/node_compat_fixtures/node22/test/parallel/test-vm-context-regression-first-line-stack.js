'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');

const context = vm.createContext({ foo: 'bar', thing: 'lala' });

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

