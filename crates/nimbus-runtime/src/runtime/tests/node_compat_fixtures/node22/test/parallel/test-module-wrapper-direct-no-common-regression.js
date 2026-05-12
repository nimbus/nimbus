'use strict';
const assert = require('assert');
const Module = require('module');

global.__nimbusDirectNoCommonWrapperRegression = 0;

const patchedWrapper = { ...Module.wrapper };
patchedWrapper[0] +=
  'global.__nimbusDirectNoCommonWrapperRegression = ' +
  '(global.__nimbusDirectNoCommonWrapperRegression || 0) + 1;';
Module.wrapper = patchedWrapper;

require('../fixtures/not-main-module.js');

assert.strictEqual(global.__nimbusDirectNoCommonWrapperRegression, 1);

