'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');
const Script = vm.Script;

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

