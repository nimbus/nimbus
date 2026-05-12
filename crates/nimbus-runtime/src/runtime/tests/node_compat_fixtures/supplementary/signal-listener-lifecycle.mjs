import assert from 'node:assert';

const signalName = 'SIGINT';
const before = process.listenerCount(signalName);
function noopSignalListener() {}

process.on(signalName, noopSignalListener);
assert.strictEqual(process.listenerCount(signalName), before + 1);
process.off(signalName, noopSignalListener);
assert.strictEqual(process.listenerCount(signalName), before);
