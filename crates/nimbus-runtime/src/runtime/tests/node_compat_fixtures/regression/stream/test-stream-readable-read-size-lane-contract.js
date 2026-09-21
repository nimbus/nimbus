'use strict';

// Nimbus regression probe: the amount that Readable#read() without a size
// returns from a paused byte stream, as observed on official Node v20.20.2,
// v22.23.1, v24.20.0 and v26.8.1. Node 26 returns one buffered chunk at a
// time (nodejs/node#60441). Node 24 and earlier return all buffered data.
// The official test-stream-readable-infinite-read.js fixture asserts only the
// behavior of the release line it comes from.

const common = require('../common');
const assert = require('assert');
const { Readable } = require('stream');

const nodeMajor = Number(process.versions.node.split('.')[0]);
const oneBufferAtATime = nodeMajor >= 26;

{
  const readable = new Readable({ read() {} });
  readable.push(Buffer.alloc(3));
  readable.push(Buffer.alloc(5));
  assert.strictEqual(readable.readableLength, 8);
  assert.strictEqual(readable.read().length, oneBufferAtATime ? 3 : 8);
  assert.strictEqual(readable.readableLength, oneBufferAtATime ? 5 : 0);
}

{
  // A string decoder always returns all buffered data.
  const readable = new Readable({ read() {}, encoding: 'utf8' });
  readable.push('ab');
  readable.push('cde');
  assert.strictEqual(readable.read(), 'abcde');
}

{
  // The infinite-read pattern with the default highWaterMark.
  const chunk = Buffer.alloc(8192);
  let reads = 0;
  const readable = new Readable({
    read() {
      reads++;
      this.push(chunk);
    },
  });
  let expected;
  if (oneBufferAtATime) {
    expected = [8192, 8192, 8192, 8192];
  } else if (nodeMajor >= 22) {
    expected = [16384, 73728, 73728, 73728];
  } else {
    expected = [16384, 24576, 24576, 24576];
  }
  const lengths = [];
  readable.on('readable', common.mustCall(function() {
    if (lengths.length === expected.length) {
      readable.removeAllListeners('readable');
      return;
    }
    lengths.push(readable.read().length);
  }, expected.length + 1));
  process.on('exit', () => {
    assert.deepStrictEqual(lengths, expected);
    assert.ok(reads > 0);
  });
}
