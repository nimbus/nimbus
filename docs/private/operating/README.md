# Operating Runbooks

This directory owns contributor and operator procedures. Architecture facts
belong in [`../architecture/README.md`](../architecture/README.md), and active
delivery state belongs in [`../plans/README.md`](../plans/README.md).

## Routing

| Task | Runbook |
| --- | --- |
| Bootstrap a checkout, choose the correct build entry point, or run Nimbus locally | [`local-dev.md`](local-dev.md) |
| Select tests, diagnose a false green or hang, or capture acceptance evidence | [`verification.md`](verification.md) |
| Configure or verify Cloudflare adapter behavior | [`cloudflare-adapters.md`](cloudflare-adapters.md) |
| Build and operate the container image | [`container-image.md`](container-image.md) |
| Configure encryption and key custody | [`encryption.md`](encryption.md) |
| Configure, observe, or recover metadata retention | [`metadata-retention.md`](metadata-retention.md) |
| Operate the RESP-compatible Nimbus KV listener | [`nimbus-kv.md`](nimbus-kv.md) |
| Validate tenant-isolation operations | [`tenant-isolation.md`](tenant-isolation.md) |

## Runbook contract

A runbook must name its prerequisites, exact commands, success signal,
failure evidence, cleanup owner, and any state that an operator can retain for
diagnosis. Commands must preserve their real exit status. A destructive or
host-wide procedure must name its exact targets and recovery boundary.

Use repository Make targets for full or fresh-checkout verification. They own
generated build prerequisites and the repository single-flight guard. Use a
focused direct command only when the runbook or active plan names it for an
iteration loop.
