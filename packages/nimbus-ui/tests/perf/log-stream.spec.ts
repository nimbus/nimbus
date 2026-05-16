import { expect, test } from "../e2e/fixtures/nimbus-server";

const EVENT_RATE_HZ = 100;
const SUSTAIN_SECONDS = 10;
const LONG_TASK_BUDGET_MS = 50;
const HEAP_GROWTH_BUDGET_BYTES = 50 * 1024 * 1024;
const FPS_BUDGET = 30;

test.describe("observability LogStream perf budget", () => {
  test(`sustains ${EVENT_RATE_HZ} events/sec for ${SUSTAIN_SECONDS}s within the budget`, async ({
    page,
    nimbusServer,
    request,
  }) => {
    const token = nimbusServer.readToken();
    const sessionRes = await request.post(
      `${nimbusServer.baseURL}/ui/auth/session`,
      {
        data: { token },
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
        },
      },
    );
    expect(sessionRes.status()).toBe(200);
    const setCookie = sessionRes.headers()["set-cookie"];
    expect(setCookie).toBeTruthy();
    const cookieKv = setCookie.split(/;\s*/)[0].split("=");
    await page.context().addCookies([
      {
        name: cookieKv[0],
        value: cookieKv.slice(1).join("="),
        url: nimbusServer.baseURL,
      },
    ]);

    await page.addInitScript(() => {
      type Listener = () => void;
      const ring: unknown[] = [];
      const listeners = new Set<Listener>();
      let snapshotCache: unknown[] = ring.slice();
      let snapshotDirty = false;
      const store = {
        subscribe(listener: Listener) {
          listeners.add(listener);
          return () => {
            listeners.delete(listener);
          };
        },
        snapshot() {
          if (snapshotDirty) {
            snapshotCache = ring.slice();
            snapshotDirty = false;
          }
          return snapshotCache;
        },
        push(event: unknown) {
          ring.unshift(event);
          if (ring.length > 200) {
            ring.length = 200;
          }
          snapshotDirty = true;
          for (const listener of listeners) listener();
        },
      };
      (window as unknown as { __nimbusEvents: typeof store }).__nimbusEvents =
        store;
      (window as unknown as { __nimbusPerf: { longTasks: number[] } }).__nimbusPerf =
        { longTasks: [] };
      const target = (
        window as unknown as { __nimbusPerf: { longTasks: number[] } }
      ).__nimbusPerf;
      try {
        const observer = new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) {
            target.longTasks.push(entry.duration);
          }
        });
        observer.observe({ entryTypes: ["longtask"] });
      } catch {
        // longtask observer not available in this environment
      }
    });

    await page.goto(`${nimbusServer.baseURL}/ui/observability?tab=logs`);
    await page.getByTestId("observability-log-empty").waitFor({ timeout: 10_000 });

    const result = await page.evaluate(
      async ({ rate, seconds }) => {
        const win = window as unknown as {
          __nimbusEvents: {
            push: (event: unknown) => void;
            snapshot: () => unknown[];
          };
          __nimbusPerf: { longTasks: number[] };
        };
        const memBefore = (
          performance as unknown as { memory?: { usedJSHeapSize: number } }
        ).memory?.usedJSHeapSize ?? 0;
        let frameCount = 0;
        let rafActive = true;
        const rafTick = () => {
          frameCount += 1;
          if (rafActive) requestAnimationFrame(rafTick);
        };
        requestAnimationFrame(rafTick);

        const start = performance.now();
        const total = rate * seconds;
        const intervalMs = 1000 / rate;
        let pushed = 0;
        await new Promise<void>((resolveDone) => {
          const handle = window.setInterval(() => {
            if (pushed >= total) {
              window.clearInterval(handle);
              resolveDone();
              return;
            }
            pushed += 1;
            win.__nimbusEvents.push({
              _id: `perf-${pushed}`,
              _creationTime: Date.now(),
              createdAt: Date.now(),
              level: pushed % 17 === 0 ? "warn" : "info",
              source: "perf",
              category: "synthetic",
              message: `synthetic event ${pushed} of ${total}`,
              correlationId: pushed % 5 === 0 ? `corr-${pushed}` : null,
            });
          }, intervalMs);
        });
        const elapsedSec = (performance.now() - start) / 1000;
        rafActive = false;
        const memAfter = (
          performance as unknown as { memory?: { usedJSHeapSize: number } }
        ).memory?.usedJSHeapSize ?? 0;
        const longTasks = win.__nimbusPerf.longTasks.slice();
        const renderedRows = document
          .querySelectorAll('[data-testid^="observability-log-row-"]')
          .length;
        return {
          pushed,
          elapsedSec,
          fps: frameCount / elapsedSec,
          maxLongTaskMs: longTasks.reduce((m, v) => Math.max(m, v), 0),
          longTaskCount: longTasks.length,
          heapGrowthBytes: memAfter - memBefore,
          renderedRows,
        };
      },
      { rate: EVENT_RATE_HZ, seconds: SUSTAIN_SECONDS },
    );

    expect(result.pushed).toBe(EVENT_RATE_HZ * SUSTAIN_SECONDS);
    expect(result.elapsedSec).toBeGreaterThanOrEqual(SUSTAIN_SECONDS * 0.9);
    expect(result.renderedRows).toBeGreaterThan(0);
    expect(result.maxLongTaskMs).toBeLessThanOrEqual(LONG_TASK_BUDGET_MS);
    expect(result.heapGrowthBytes).toBeLessThanOrEqual(HEAP_GROWTH_BUDGET_BYTES);
    expect(result.fps).toBeGreaterThanOrEqual(FPS_BUDGET);

    console.log(
      `[perf] pushed=${result.pushed} fps=${result.fps.toFixed(1)} maxLongTaskMs=${result.maxLongTaskMs.toFixed(1)} longTasks=${result.longTaskCount} heapGrowthMB=${(result.heapGrowthBytes / (1024 * 1024)).toFixed(2)} rows=${result.renderedRows}`,
    );
  });
});
