'use strict';

require('../common');
const assert = require('assert');
const path = require('path');
const pwd = process.cwd();

assert.strictEqual(path.posix.join(''), '.');
assert.strictEqual(path.posix.join('', ''), '.');
assert.strictEqual(path.win32.join(''), '.');
assert.strictEqual(path.win32.join('', ''), '.');
assert.strictEqual(path.join(pwd), pwd);
assert.strictEqual(path.join(pwd, ''), pwd);

assert.strictEqual(path.posix.normalize(''), '.');
assert.strictEqual(path.win32.normalize(''), '.');
assert.strictEqual(path.normalize(pwd), pwd);

assert.strictEqual(path.posix.isAbsolute(''), false);
assert.strictEqual(path.win32.isAbsolute(''), false);

assert.strictEqual(path.resolve(''), pwd);
assert.strictEqual(path.resolve('', ''), pwd);

assert.strictEqual(path.relative('', pwd), '');
assert.strictEqual(path.relative(pwd, ''), '');
assert.strictEqual(path.relative(pwd, pwd), '');
