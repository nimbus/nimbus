'use strict';
const assert = require('assert');
const Module = require('module');

global.__neovexNewlineWrapCounter = 0;

Module.wrap = function wrapWithSeparator(script) {
  script = script.replace(/^#!.*?\n/, '');
  return `${Module.wrapper[0]}\n${script}${Module.wrapper[1]}`;
};

Module.wrapper = [
  '(function (exports, require, module, __filename, __dirname) { global.__neovexNewlineWrapCounter = (global.__neovexNewlineWrapCounter || 0) + 1',
  '\n});',
];

require('./not-main-module.js');

assert.strictEqual(global.__neovexNewlineWrapCounter, 1);
