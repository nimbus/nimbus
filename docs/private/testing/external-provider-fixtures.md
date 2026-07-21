# External Provider Test Fixtures

`compose.test-external-providers.yaml` and
`scripts/external-provider-fixture.sh` jointly own the disposable PostgreSQL,
MySQL, and libSQL verification lifecycle. The compose file is the declarative
source of truth for images, container configuration, localhost ports, labels,
and health checks. The lifecycle script owns runtime validation, safe reuse,
bounded readiness, test environment variables, diagnostics, and cleanup.

This interface is deliberately separate from the repository-root
`compose.yaml`. That file provides persistent developer backing services with
different versions and privileges. Test fixtures are ephemeral and include the
database-creation privileges required by provider tests.

## Commands

Run one provider, including fixture startup and cleanup:

```bash
make test-external-provider PROVIDER=mysql
```

Run all three providers:

```bash
make test-external-providers
```

Run one focused nextest expression through the same pinned fixture lifecycle:

```bash
make test-external-provider PROVIDER=mysql \
  TEST_FILTER='test(mysql_committer_lease_concurrent_acquire_has_exactly_one_winner)'
```

The provider-wide filter remains in force and is intersected with
`TEST_FILTER`; `--no-tests fail` rejects stale or zero-match expressions.

Start or remove one retained fixture without running tests:

```bash
make provider-fixture-up PROVIDER=mysql
make provider-fixture-down PROVIDER=mysql
```

The default `run` lifecycle removes only fixtures it started. `KEEP=1` retains
a newly started fixture for repeated work. A later test must opt in with
`REUSE=1`; this prevents an ordinary invocation from silently adopting or
removing a pre-existing container:

```bash
make test-external-provider PROVIDER=mysql KEEP=1
make test-external-provider PROVIDER=mysql REUSE=1
make provider-fixture-down PROVIDER=mysql
```

Fixtures are pinned to:

- PostgreSQL `postgres:16`;
- MySQL `mysql:8.4`;
- libSQL `ghcr.io/tursodatabase/libsql-server:v0.24.33`.

The lifecycle exports `NIMBUS_REQUIRE_EXTERNAL_PROVIDER_FIXTURES=1` and the
provider URL itself. Therefore a missing URL or an internal provider-test skip
is a failure, not passing evidence. The runner also passes `--no-tests fail` to
nextest so a stale filter cannot report green with zero selected tests. Each
provider lane includes storage, engine, and the corresponding `nimbus-system`
two-engine projection contract.

Rust provider tests never start Testcontainers implicitly. With all required
provider URLs present they use the shared fixture and isolate each test by a
unique PostgreSQL schema, MySQL database prefix, or libSQL namespace. With
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`, ordinary workspace
lanes deliberately omit provider execution and cannot be cited as provider
evidence. With neither URLs nor that explicit omission, tests fail with the
corresponding `make test-external-provider PROVIDER=...` command. This keeps the
pinned Compose file as the sole image and service-configuration authority.

## Ports and existing services

Published ports bind only to `127.0.0.1`. Defaults are PostgreSQL `5432`, MySQL
`3306`, and libSQL primary/admin `18080`/`18081`. Override a collision without
changing the URLs manually:

```bash
NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT=25432 \
  make test-external-provider PROVIDER=postgres

NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT=28080 \
NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT=28081 \
  make test-external-provider PROVIDER=libsql
```

Reuse requires an exact Nimbus owner label, provider label, image, Compose
configuration hash, and healthy state. A foreign or mismatched container is
never adopted, stopped, or deleted. A port owned by another process produces a
clear preflight failure. Startup and test failures print the selected fixture's
last 200 log lines before preserving the test exit code and cleaning up.

## Verification ownership

GitHub Actions keeps one matrix shard per provider with `fail-fast: false`, but
calls the same Make/lifecycle interface as local verification. Update image or
configuration pins only in `compose.test-external-providers.yaml`; the reuse
guard will reject containers built from the old Compose hash.

The deterministic process-boundary tests are:

```bash
make verify-external-provider-fixture-helper
```

They use fake container, port-check, HTTP, test-runner, and nextest commands to
cover selection, readiness success and timeout, exit-code propagation, log
emission, cleanup on success/failure/signal, keep/reuse, foreign-container
refusal, missing runtime and URL, port collision, unknown providers, and
zero-test failure.
