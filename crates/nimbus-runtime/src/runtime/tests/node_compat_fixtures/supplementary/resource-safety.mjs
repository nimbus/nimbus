import assert from 'node:assert';
import { open, rm } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const fixtureDir = path.dirname(fileURLToPath(import.meta.url));
const fixtureFile = path.join(fixtureDir, 'resource-safety.tmp');

const handle = await open(fixtureFile, 'w+');
await handle.writeFile('resource-safety-ok');
await handle.sync();
await handle.close();

await assert.rejects(
  handle.readFile(),
  (error) => (
    error?.code === 'EBADF' ||
    /bad resource|closed|invalid/i.test(String(error?.message))
  ),
);
assert.strictEqual(
  await rm(fixtureFile, { force: true }).then(() => 'removed'),
  'removed',
);

const controller = new AbortController();
const abortedDelay = delay(60_000, 'late-value', { signal: controller.signal });
controller.abort();
await assert.rejects(abortedDelay, { name: 'AbortError' });
