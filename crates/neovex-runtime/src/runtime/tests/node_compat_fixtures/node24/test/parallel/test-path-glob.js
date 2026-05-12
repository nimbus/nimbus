'use strict';

require('../common');
const assert = require('assert');
const path = require('path');

const globs = {
  win32: [
    ['foo\\bar\\baz', 'foo\\[bcr]ar\\baz', true],
    ['foo\\bar\\baz', 'foo\\[!bcr]ar\\baz', false],
    ['foo\\bar\\baz', 'foo\\[bc-r]ar\\baz', true],
    ['foo\\bar\\baz', 'foo\\*\\!bar\\*\\baz', false],
    ['foo\\bar1\\baz', 'foo\\bar[0-9]\\baz', true],
    ['foo\\bar5\\baz', 'foo\\bar[0-9]\\baz', true],
    ['foo\\barx\\baz', 'foo\\bar[a-z]\\baz', true],
    ['foo\\bar\\baz\\boo', 'foo\\[bc-r]ar\\baz\\*', true],
    ['foo\\bar\\baz', 'foo/**', true],
    ['foo\\bar\\baz', '*', false],
  ],
  posix: [
    ['foo/bar/baz', 'foo/[bcr]ar/baz', true],
    ['foo/bar/baz', 'foo/[!bcr]ar/baz', false],
    ['foo/bar/baz', 'foo/[bc-r]ar/baz', true],
    ['foo/bar/baz', 'foo/*/!bar/*/baz', false],
    ['foo/bar1/baz', 'foo/bar[0-9]/baz', true],
    ['foo/bar5/baz', 'foo/bar[0-9]/baz', true],
    ['foo/barx/baz', 'foo/bar[a-z]/baz', true],
    ['foo/bar/baz/boo', 'foo/[bc-r]ar/baz/*', true],
    ['foo/bar/baz', 'foo/**', true],
    ['foo/bar/baz', '*', false],
  ],
};

for (const [platform, platformGlobs] of Object.entries(globs)) {
  for (const [pathStr, glob, expected] of platformGlobs) {
    const actual = path[platform].matchesGlob(pathStr, glob);
    assert.strictEqual(
      actual,
      expected,
      `Expected ${pathStr} to ${expected ? '' : 'not '}match ${glob} on ${platform}`,
    );
  }
}

assert.throws(() => path.matchesGlob(123, 'foo/bar/baz'), /.*must be of type string.*/);
assert.throws(() => path.matchesGlob('foo/bar/baz', 123), /.*must be of type string.*/);
