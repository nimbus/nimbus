# Multi-Backend Adapter Hardening

This is the operating contract produced by the MBA0-MBA14 hardening wave.
Use it when changing storage backends, compatibility adapters, runtime worker
boundaries, or cross-cutting verification.

## Contracts

- Track cross-cutting debt in `docs/technical-debt.md` with category, severity,
  owner, status, description, and motivation.
- Keep storage traits focused. Static-dispatch async traits are acceptable when
  they are not stored behind `dyn`; object-erased async traits use boxed
  futures per `docs/architecture/trait-conventions.md`.
- Keep built-in backend and adapter registration explicit and typed. Do not add
  `inventory` or linker-section registration without a later ADR that proves an
  out-of-tree plugin boundary.
- Put backend-coupled workers behind `RuntimeHooks`. Engine-owned workers stay
  engine-owned.
- Add dual-target tests for adapter protocol fidelity. PR lanes may run Nimbus
  only; credentialed external targets run in the nightly matrix.
- Follow `docs/decisions/002-auth-caching-policy.md`: security-sensitive auth
  and policy decisions do not get implicit caches.
- Follow the per-backend SQL-safety ADRs. User values are parameters; dynamic
  identifiers go through documented helper allowlists.
- Instrument hot paths with named latency segments and budgets from
  `docs/operating/latency-budgets.md`.
- Keep stable logical table identity separate from physical layout. Public APIs
  remain `TableName` based; storage uses the table-catalog contract from
  `docs/architecture/storage/table-identity.md`.
- Preserve typed ordering in range scans. String, numeric, and future binary
  keys must use type-correct storage or expressions.
- Route reads by real backend consistency surfaces. Unsupported eventual-read
  paths are `not_applicable`, not faked.
- Use hybrid event capture: storage owns atomicity, engine owns generic
  committed-event metadata, adapters own wire shapes.

## Verification

The reusable gate is:

```sh
bash scripts/verify-multi-backend-adapter-hardening.sh
```

The gate expects the archived plan and proof bundle under
`docs/plans/proof/multi-backend-adapter-hardening/`.
