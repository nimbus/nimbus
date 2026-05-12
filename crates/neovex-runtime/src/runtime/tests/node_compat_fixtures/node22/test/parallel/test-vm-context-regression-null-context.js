'use strict';
require('../common');
const assert = require('assert');
const vm = require('vm');

assert.throws(() => {
  vm.runInNewContext('', null, 'some.js');
}, {
  code: 'ERR_INVALID_ARG_TYPE',
  name: 'TypeError',
});

