'use strict';

const { isWindows } = require('../common');
const assert = require('assert');
const url = require('url');

{
  const fileURL = url.pathToFileURL('test/').href;
  assert.ok(fileURL.startsWith('file:///'));
  assert.ok(fileURL.endsWith('/'));
}

{
  const fileURL = url.pathToFileURL('test\\').href;
  assert.ok(fileURL.startsWith('file:///'));
  assert.match(fileURL, isWindows ? /\/$/ : /%5C$/);
}

{
  const fileURL = url.pathToFileURL('test/%').href;
  assert.ok(fileURL.includes('%25'));
}

{
  if (isWindows) {
    assert.throws(() => url.pathToFileURL('\\\\\\no-server'), {
      code: 'ERR_INVALID_ARG_VALUE',
    });

    assert.throws(() => url.pathToFileURL('\\\\host'), {
      code: 'ERR_INVALID_ARG_VALUE',
    });

    assert.throws(() => url.pathToFileURL([
      '\\\\',
      { [Symbol.toPrimitive]: () => 'blep\\blop' },
    ]), {
      code: 'ERR_INVALID_ARG_TYPE',
    });
    assert.throws(() => url.pathToFileURL(['\\\\', 'blep\\blop']), {
      code: 'ERR_INVALID_ARG_TYPE',
    });
    assert.throws(() => url.pathToFileURL({
      [Symbol.toPrimitive]: () => '\\\\blep\\blop',
    }), {
      code: 'ERR_INVALID_ARG_TYPE',
    });
  } else {
    const fileURL = url.pathToFileURL('\\\\nas\\share\\path.txt').href;
    assert.match(fileURL, /file:\/\/.+%5C%5Cnas%5Cshare%5Cpath\.txt$/);
  }
}

const windowsTestCases = [
  { path: 'C:\\foo', expected: 'file:///C:/foo' },
  { path: 'C:\\FOO', expected: 'file:///C:/FOO' },
  { path: 'C:\\dir\\foo', expected: 'file:///C:/dir/foo' },
  { path: 'C:\\dir\\', expected: 'file:///C:/dir/' },
  { path: 'C:\\foo.mjs', expected: 'file:///C:/foo.mjs' },
  { path: 'C:\\foo bar', expected: 'file:///C:/foo%20bar' },
  { path: 'C:\\foo?bar', expected: 'file:///C:/foo%3Fbar' },
  { path: 'C:\\foo#bar', expected: 'file:///C:/foo%23bar' },
  { path: 'C:\\foo&bar', expected: 'file:///C:/foo&bar' },
  { path: 'C:\\foo=bar', expected: 'file:///C:/foo=bar' },
  { path: 'C:\\foo:bar', expected: 'file:///C:/foo:bar' },
  { path: 'C:\\foo;bar', expected: 'file:///C:/foo;bar' },
  { path: 'C:\\foo%bar', expected: 'file:///C:/foo%25bar' },
  { path: 'C:\\foo\\bar', expected: 'file:///C:/foo/bar' },
  { path: 'C:\\foo\bbar', expected: 'file:///C:/foo%08bar' },
  { path: 'C:\\foo\tbar', expected: 'file:///C:/foo%09bar' },
  { path: 'C:\\foo\nbar', expected: 'file:///C:/foo%0Abar' },
  { path: 'C:\\foo\rbar', expected: 'file:///C:/foo%0Dbar' },
  { path: 'C:\\fóóbàr', expected: 'file:///C:/f%C3%B3%C3%B3b%C3%A0r' },
  { path: 'C:\\€', expected: 'file:///C:/%E2%82%AC' },
  { path: 'C:\\🚀', expected: 'file:///C:/%F0%9F%9A%80' },
  { path: 'C:\\foo^bar', expected: 'file:///C:/foo%5Ebar' },
  { path: 'C:\\foo[bar', expected: 'file:///C:/foo%5Bbar' },
  { path: 'C:\\foo]bar', expected: 'file:///C:/foo%5Dbar' },
  { path: '\\\\?\\C:\\path\\to\\file.txt', expected: 'file:///C:/path/to/file.txt' },
  { path: '\\\\nas\\My Docs\\File.doc', expected: 'file://nas/My%20Docs/File.doc' },
  { path: '\\\\?\\UNC\\server\\share\\folder\\file.txt', expected: 'file://server/share/folder/file.txt' },
];
const alphabet = String.fromCharCode(...Array.from({ length: 26 }, (_, i) => 'a'.charCodeAt() + i));
const posixTestCases = [
  { path: '/foo', expected: 'file:///foo' },
  { path: '/FOO', expected: 'file:///FOO' },
  { path: '/dir/foo', expected: 'file:///dir/foo' },
  { path: '/dir/', expected: 'file:///dir/' },
  { path: '/foo.mjs', expected: 'file:///foo.mjs' },
  { path: '/foo bar', expected: 'file:///foo%20bar' },
  { path: '/foo?bar', expected: 'file:///foo%3Fbar' },
  { path: '/foo#bar', expected: 'file:///foo%23bar' },
  { path: '/foo&bar', expected: 'file:///foo&bar' },
  { path: '/foo=bar', expected: 'file:///foo=bar' },
  { path: '/foo:bar', expected: 'file:///foo:bar' },
  { path: '/foo;bar', expected: 'file:///foo;bar' },
  { path: '/foo%bar', expected: 'file:///foo%25bar' },
  { path: '/foo\\bar', expected: 'file:///foo%5Cbar' },
  { path: '/foo\bbar', expected: 'file:///foo%08bar' },
  { path: '/foo\tbar', expected: 'file:///foo%09bar' },
  { path: '/foo\nbar', expected: 'file:///foo%0Abar' },
  { path: '/foo\rbar', expected: 'file:///foo%0Dbar' },
  { path: '/fóóbàr', expected: 'file:///f%C3%B3%C3%B3b%C3%A0r' },
  { path: '/€', expected: 'file:///%E2%82%AC' },
  { path: '/🚀', expected: 'file:///%F0%9F%9A%80' },
  { path: '/foo\r\n\t<>"#%{}|^[\\~]`?bar', expected: 'file:///foo%0D%0A%09%3C%3E%22%23%25%7B%7D%7C%5E%5B%5C%7E%5D%60%3Fbar' },
  {
    path: `/${Array.from({ length: 0x7FFF }, (_, i) => String.fromCharCode(i)).join('')}`,
    expected: `file:///${
      Array.from({ length: 0x21 }, (_, i) => `%${i.toString(16).toUpperCase().padStart(2, '0')}`).join('')
    }!%22%23$%25&'()*+,-./0123456789:;%3C=%3E%3F@${
      alphabet.toUpperCase()
    }%5B%5C%5D%5E_%60${alphabet}%7B%7C%7D%7E%7F${
      Array.from({ length: 0x800 - 0x80 }, (_, i) => `%${
        (Math.floor((i - 0x80) / 0x40) + 0xC4).toString(16).toUpperCase()
      }%${
        ((i % 0x40) + 0x80).toString(16).toUpperCase()
      }`).join('')
    }${
      Array.from({ length: 0x7FFF - 0x800 }, (_, i) => i + 0x800).map((i) => `%E${
        (i >> 12).toString(16).toUpperCase()
      }%${
        (((i >> 6) % 0x40) + 0x80).toString(16).toUpperCase()
      }%${
        ((i % 0x40) + 0x80).toString(16).toUpperCase()
      }`).join('')
    }`
  },
  { path: `/${String.fromCodePoint(0x1F303)}`, expected: 'file:///%F0%9F%8C%83' },
];

for (const { path, expected } of windowsTestCases) {
  const actual = url.pathToFileURL(path, { windows: true }).href;
  assert.strictEqual(actual, expected);
}

for (const { path, expected } of posixTestCases) {
  const actual = url.pathToFileURL(path, { windows: false }).href;
  assert.strictEqual(actual, expected);
}

const testCases = isWindows ? windowsTestCases : posixTestCases;

const whenNullActual = url.pathToFileURL(testCases[0].path, null);
assert.strictEqual(whenNullActual.href, testCases[0].expected);

for (const { path, expected } of testCases) {
  const actual = url.pathToFileURL(path).href;
  assert.strictEqual(actual, expected);
}

{
  for (const badPath of [
    undefined, null, true, 42, 42n, Symbol('42'), NaN, {}, [], () => {},
    Promise.resolve('foo'),
    new Date(),
    new String('notPrimitive'),
    { toString() { return 'amObject'; } },
    { [Symbol.toPrimitive]: (hint) => 'amObject' },
  ]) {
    assert.throws(() => url.pathToFileURL(badPath), {
      code: 'ERR_INVALID_ARG_TYPE',
    });
  }
}
