# Documentation and Application Verification Reliability Proofs

Status: AVR0 and AVR1 are complete. AVR2 is in progress. Its phase-one
candidate is in review correction.

Owner: `docs/private/plans/docs-and-app-verification-reliability-plan.md`

[`plan-review-resolution.md`](plan-review-resolution.md) records the complete
pre-activation review disposition. [`acceptance-contract.md`](acceptance-contract.md)
owns the detailed decisions, verifier map, phase counts, and exact task
commands.

- [`avr0.md`](avr0.md) records the baseline, red verifier, and private routing.
- [`avr1.md`](avr1.md) records stable network contracts and archival.

AVR2 creates its proof at closeout. Each later task creates its proof when work
starts. A task proof records fail-before evidence, exact commands, counts,
hashes, review results, and residual risk. Checks that cannot run use
`UNVERIFIED` with a reason.

The final closeout proof records all three implementation pull requests, the
cleanup pull request, merge commits, hosted runs, minicloud reports, and cleanup
results.
