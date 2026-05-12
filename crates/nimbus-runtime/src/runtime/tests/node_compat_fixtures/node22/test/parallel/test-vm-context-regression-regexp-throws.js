'use strict';
require('../common');
const vm = require('vm');

const script = vm.createScript(
  'const assert = require("assert"); assert.throws(function() { throw "hello world"; }, /hello/);',
  'some.js',
);
script.runInNewContext({ require });

