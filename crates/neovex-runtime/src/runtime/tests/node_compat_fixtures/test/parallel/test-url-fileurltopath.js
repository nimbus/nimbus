'use strict';
const { isWindows } = require('../common');

const { test } = require('node:test');
const assert = require('node:assert');
const url = require('node:url');

test('invalid arguments', () => {
  for (const arg of [null, undefined, 1, {}, true]) {
    assert.throws(() => url.fileURLToPath(arg), {
      code: 'ERR_INVALID_ARG_TYPE'
    });
  }
});

test('input must be a file URL', () => {
  assert.throws(() => url.fileURLToPath('https://a/b/c'), {
    code: 'ERR_INVALID_URL_SCHEME'
  });
});

test('fileURLToPath with host', () => {
  const withHost = new URL('file://host/a');

  if (isWindows) {
    assert.strictEqual(url.fileURLToPath(withHost), '\\\\host\\a');
  } else {
    assert.throws(() => url.fileURLToPath(withHost), {
      code: 'ERR_INVALID_FILE_URL_HOST'
    });
  }
});

const windowsTestCases = [
  { path: 'C:\\foo', fileURL: 'file:///C:/foo' },
  { path: 'C:\\FOO', fileURL: 'file:///C:/FOO' },
  { path: 'C:\\dir\\foo', fileURL: 'file:///C:/dir/foo' },
  { path: 'C:\\dir\\', fileURL: 'file:///C:/dir/' },
  { path: 'C:\\foo.mjs', fileURL: 'file:///C:/foo.mjs' },
  { path: 'C:\\foo bar', fileURL: 'file:///C:/foo%20bar' },
  { path: 'C:\\foo?bar', fileURL: 'file:///C:/foo%3Fbar' },
  { path: 'C:\\foo#bar', fileURL: 'file:///C:/foo%23bar' },
  { path: 'C:\\foo&bar', fileURL: 'file:///C:/foo&bar' },
  { path: 'C:\\foo=bar', fileURL: 'file:///C:/foo=bar' },
  { path: 'C:\\foo:bar', fileURL: 'file:///C:/foo:bar' },
  { path: 'C:\\foo;bar', fileURL: 'file:///C:/foo;bar' },
  { path: 'C:\\foo%bar', fileURL: 'file:///C:/foo%25bar' },
  { path: 'C:\\foo\\bar', fileURL: 'file:///C:/foo/bar' },
  { path: 'C:\\foo\bbar', fileURL: 'file:///C:/foo%08bar' },
  { path: 'C:\\foo\tbar', fileURL: 'file:///C:/foo%09bar' },
  { path: 'C:\\foo\nbar', fileURL: 'file:///C:/foo%0Abar' },
  { path: 'C:\\foo\rbar', fileURL: 'file:///C:/foo%0Dbar' },
  { path: 'C:\\fóóbàr', fileURL: 'file:///C:/f%C3%B3%C3%B3b%C3%A0r' },
  { path: 'C:\\€', fileURL: 'file:///C:/%E2%82%AC' },
  { path: 'C:\\🚀', fileURL: 'file:///C:/%F0%9F%9A%80' },
  { path: '\\\\nas\\My Docs\\File.doc', fileURL: 'file://nas/My%20Docs/File.doc' },
];

const posixTestCases = [
  { path: '/foo', fileURL: 'file:///foo' },
  { path: '/FOO', fileURL: 'file:///FOO' },
  { path: '/dir/foo', fileURL: 'file:///dir/foo' },
  { path: '/dir/', fileURL: 'file:///dir/' },
  { path: '/foo.mjs', fileURL: 'file:///foo.mjs' },
  { path: '/foo bar', fileURL: 'file:///foo%20bar' },
  { path: '/foo?bar', fileURL: 'file:///foo%3Fbar' },
  { path: '/foo#bar', fileURL: 'file:///foo%23bar' },
  { path: '/foo&bar', fileURL: 'file:///foo&bar' },
  { path: '/foo=bar', fileURL: 'file:///foo=bar' },
  { path: '/foo:bar', fileURL: 'file:///foo:bar' },
  { path: '/foo;bar', fileURL: 'file:///foo;bar' },
  { path: '/foo%bar', fileURL: 'file:///foo%25bar' },
  { path: '/foo\\bar', fileURL: 'file:///foo%5Cbar' },
  { path: '/foo\bbar', fileURL: 'file:///foo%08bar' },
  { path: '/foo\tbar', fileURL: 'file:///foo%09bar' },
  { path: '/foo\nbar', fileURL: 'file:///foo%0Abar' },
  { path: '/foo\rbar', fileURL: 'file:///foo%0Dbar' },
  { path: '/fóóbàr', fileURL: 'file:///f%C3%B3%C3%B3b%C3%A0r' },
  { path: '/€', fileURL: 'file:///%E2%82%AC' },
  { path: '/🚀', fileURL: 'file:///%F0%9F%9A%80' },
];

test('fileURLToPath with windows path', { skip: !isWindows }, () => {
  for (const { path, fileURL } of windowsTestCases) {
    const fromString = url.fileURLToPath(fileURL, { windows: true });
    assert.strictEqual(fromString, path);
    const fromURL = url.fileURLToPath(new URL(fileURL), { windows: true });
    assert.strictEqual(fromURL, path);
  }
});

test('fileURLToPath with posix path', { skip: isWindows }, () => {
  for (const { path, fileURL } of posixTestCases) {
    const fromString = url.fileURLToPath(fileURL, { windows: false });
    assert.strictEqual(fromString, path);
    const fromURL = url.fileURLToPath(new URL(fileURL), { windows: false });
    assert.strictEqual(fromURL, path);
  }
});

const defaultTestCases = isWindows ? windowsTestCases : posixTestCases;

test('options is null', () => {
  const whenNullActual = url.fileURLToPath(new URL(defaultTestCases[0].fileURL), null);
  assert.strictEqual(whenNullActual, defaultTestCases[0].path);
});

test('defaultTestCases', () => {
  for (const { path, fileURL } of defaultTestCases) {
    const fromString = url.fileURLToPath(fileURL);
    assert.strictEqual(fromString, path);
    const fromURL = url.fileURLToPath(new URL(fileURL));
    assert.strictEqual(fromURL, path);
  }
});
