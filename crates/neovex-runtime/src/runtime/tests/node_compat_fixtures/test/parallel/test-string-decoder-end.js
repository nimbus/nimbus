'use strict';

require('../common');
const assert = require('assert');
const SD = require('string_decoder').StringDecoder;
const encodings = ['base64', 'base64url', 'hex', 'utf8', 'utf16le', 'ucs2'];

const bufs = [ '☃💩', 'asdf' ].map((b) => Buffer.from(b));

for (let i = 1; i <= 16; i++) {
  const bytes = '.'.repeat(i - 1).split('.').map((_, j) => j + 0x78);
  bufs.push(Buffer.from(bytes));
}

encodings.forEach(testEncoding);

testEnd('utf8', Buffer.of(0xE2), Buffer.of(0x61), '\uFFFDa');
testEnd('utf8', Buffer.of(0xE2), Buffer.of(0x82), '\uFFFD\uFFFD');
testEnd('utf8', Buffer.of(0xE2), Buffer.of(0xE2), '\uFFFD\uFFFD');
testEnd('utf8', Buffer.of(0xE2, 0x82), Buffer.of(0x61), '\uFFFDa');
testEnd('utf8', Buffer.of(0xE2, 0x82), Buffer.of(0xAC), '\uFFFD\uFFFD');
testEnd('utf8', Buffer.of(0xE2, 0x82), Buffer.of(0xE2), '\uFFFD\uFFFD');
testEnd('utf8', Buffer.of(0xE2, 0x82, 0xAC), Buffer.of(0x61), '€a');

testEnd('utf16le', Buffer.of(0x3D), Buffer.of(0x61, 0x00), 'a');
testEnd('utf16le', Buffer.of(0x3D), Buffer.of(0xD8, 0x4D, 0xDC), '\u4DD8');
testEnd('utf16le', Buffer.of(0x3D, 0xD8), Buffer.of(), '\uD83D');
testEnd('utf16le', Buffer.of(0x3D, 0xD8), Buffer.of(0x61, 0x00), '\uD83Da');
testEnd(
  'utf16le',
  Buffer.of(0x3D, 0xD8),
  Buffer.of(0x4D, 0xDC),
  '\uD83D\uDC4D'
);
testEnd('utf16le', Buffer.of(0x3D, 0xD8, 0x4D), Buffer.of(), '\uD83D');
testEnd(
  'utf16le',
  Buffer.of(0x3D, 0xD8, 0x4D),
  Buffer.of(0x61, 0x00),
  '\uD83Da'
);
testEnd('utf16le', Buffer.of(0x3D, 0xD8, 0x4D), Buffer.of(0xDC), '\uD83D');
testEnd(
  'utf16le',
  Buffer.of(0x3D, 0xD8, 0x4D, 0xDC),
  Buffer.of(0x61, 0x00),
  '👍a'
);

testEnd('base64', Buffer.of(0x61), Buffer.of(), 'YQ==');
testEnd('base64', Buffer.of(0x61), Buffer.of(0x61), 'YQ==YQ==');
testEnd('base64', Buffer.of(0x61, 0x61), Buffer.of(), 'YWE=');
testEnd('base64', Buffer.of(0x61, 0x61), Buffer.of(0x61), 'YWE=YQ==');
testEnd('base64', Buffer.of(0x61, 0x61, 0x61), Buffer.of(), 'YWFh');
testEnd('base64', Buffer.of(0x61, 0x61, 0x61), Buffer.of(0x61), 'YWFhYQ==');

testEnd('base64url', Buffer.of(0x61), Buffer.of(), 'YQ');
testEnd('base64url', Buffer.of(0x61), Buffer.of(0x61), 'YQYQ');
testEnd('base64url', Buffer.of(0x61, 0x61), Buffer.of(), 'YWE');
testEnd('base64url', Buffer.of(0x61, 0x61), Buffer.of(0x61), 'YWEYQ');
testEnd('base64url', Buffer.of(0x61, 0x61, 0x61), Buffer.of(), 'YWFh');
testEnd('base64url', Buffer.of(0x61, 0x61, 0x61), Buffer.of(0x61), 'YWFhYQ');

function testEncoding(encoding) {
  bufs.forEach((buf) => {
    testBuf(encoding, buf);
  });
}

function testBuf(encoding, buf) {
  let s = new SD(encoding);
  let res1 = '';
  for (let i = 0; i < buf.length; i++) {
    res1 += s.write(buf.slice(i, i + 1));
  }
  res1 += s.end();

  let res2 = '';
  s = new SD(encoding);
  res2 += s.write(buf);
  res2 += s.end();

  const res3 = buf.toString(encoding);

  assert.strictEqual(res1, res3);
  assert.strictEqual(res2, res3);
}

function testEnd(encoding, incomplete, next, expected) {
  let res = '';
  const s = new SD(encoding);
  res += s.write(incomplete);
  res += s.end();
  res += s.write(next);
  res += s.end();

  assert.strictEqual(res, expected);
}
