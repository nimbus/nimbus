# Documentation and Application Verification Reliability Proofs

Status: AVR0 through AVR6 are complete. Phase-one PR #275 merged into main.
The owner reconciled it, and AVR7 is in progress.

Owner: `docs/private/plans/docs-and-app-verification-reliability-plan.md`

[`plan-review-resolution.md`](plan-review-resolution.md) records the complete
pre-activation review disposition. [`acceptance-contract.md`](acceptance-contract.md)
owns the detailed decisions, verifier map, phase counts, and exact task
commands.

- [`avr0.md`](avr0.md) records the baseline, red verifier, and private routing.
- [`avr1.md`](avr1.md) records stable network contracts and archival.
- [`avr2.md`](avr2.md) records public network architecture and phase review.
- [`avr3.md`](avr3.md) records fresh-checkout prerequisites and the Node.js
  host contract.
- [`avr4.md`](avr4.md) records the validated case manifest, disposable
  workspaces, and source-byte postcondition.
- [`avr5.md`](avr5.md) records the explicit Compose-discovery opt-out and the
  deleted tracked-file sideline.
- [`avr6.md`](avr6.md) records equal explicit and bare-local invocation,
  isolated credential routing, and fail-closed trust evidence.

Each later task creates its proof when work starts. A task proof records
fail-before evidence, exact commands, counts, hashes, review results, and
residual risk. Checks that cannot run use `UNVERIFIED` with a reason.

The final closeout proof records all three implementation pull requests, the
cleanup pull request, merge commits, hosted runs, minicloud reports, and cleanup
results.
