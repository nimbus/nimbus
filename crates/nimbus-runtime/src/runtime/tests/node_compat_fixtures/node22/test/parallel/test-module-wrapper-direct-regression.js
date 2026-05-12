'use strict';
require('../common');
const assert = require('assert');
const Module = require('module');

global.__nimbusDirectWrapperRegression = 0;

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__nimbusDirectWrapperRegression = ' +
  '(global.__nimbusDirectWrapperRegression || 0) + 1;';
Module.wrapper = patchedWrapper;

require('../fixtures/not-main-module.js');

assert.strictEqual(global.__nimbusDirectWrapperRegression, 1);

