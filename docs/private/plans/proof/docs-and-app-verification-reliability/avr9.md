# AVR9 Bounded Parallel Execution

Date: 2026-08-18

## Result

AVR9 gives the nine-case application-verification lane a bounded scheduler. The
runner accepts one through nine workers and CI uses five. Each case keeps its
own process, authentication, discovery, audit, app, data, control, and log
roots. All cases share the one run-global Nimbus network-state root required by
host-global lease authority.

Workers claim priority-ordered cases through atomic directories. The first
failure claims one atomic failure record. Active cases drain, and no later case
starts. INT and TERM use the same rule. Per-case output stays private until the
owner replays it in manifest order. Reports also stay in manifest order.

The accepted minicloud campaign passed all coverage and time requirements. The
serial median was 112,403 milliseconds. The five-worker median was 67,066
milliseconds, or 0.5967 of serial time. This is below the 0.60 relative limit
and the 1,200-second absolute limit.

## Fail-Before Evidence

| Candidate | Serial median | Parallel median | Ratio | Result |
| --- | ---: | ---: | ---: | --- |
| Four workers with the static predecessor schedule | 131,828 ms | 89,256 ms | 0.6771 | Failed the relative limit. |
| Five workers before source-proof optimization | 124,355 ms | 79,136 ms | 0.6364 | Failed the relative limit. |

Both candidates passed all nine cases, 37 anchors, source checks, cleanup
checks, and port-isolation checks. The retained verdicts prove that only the
relative budget was red:

- `avr9-fail-before-max4-verdict.json`
- `avr9-fail-before-max5-verdict.json`

The final source guard keeps the same fail-closed contract with less redundant
work. In Git mode it hashes the complete index state and the exact bytes of
every baseline-dirty tracked path. A clean tracked path that changes becomes
dirty and changes the index or dirty-path set. Export mode continues to hash
all protected inputs. The guard bounds byte hashing. AVR9 adds no
source-restoration path.

## Acceptance Ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR9.1 Verify host activity. | Pass. | Every sample records one stable host fingerprint and before/after activity. Two busy preflights were classified invalid and were not counted as product failures. |
| AVR9.2 Capture three serial samples. | Pass. | Durations were 112,403, 113,142, and 112,251 ms. All reports passed. |
| AVR9.3 Add bounded scheduling. | Pass. | The 1-9 worker bound, dynamic atomic claims, long/medium/short priority, case-local logs, and manifest-order replay are covered by the contract and live tests. CI selects five workers. |
| AVR9.4 Drain after failure. | Pass. | A targeted two-worker failure and a TERM run both drained active work, prevented later starts, emitted failed JSON/JUnit, and left only released or failed lease phases. |
| AVR9.5 Capture five parallel samples. | Pass. | Durations were 66,818, 67,194, 67,629, 66,811, and 67,066 ms. All nine cases and 37 anchors passed in every sample. |
| AVR9.6 Separate raw samples from the verdict. | Pass. | Eight immutable raw sample files point to retained report/JUnit pairs. The verdict is derived from them, and the validator recomputes the evaluation exactly. |

## Behavioral Evidence

| Command or proof | Result |
| --- | --- |
| `bash scripts/examples-verify-contract-test.sh --task AVR9` | Pass. AVRC23 1/1, evaluator 5/5, and scheduler 2/2. |
| `node scripts/examples-verify-runner-fault-test.mjs --bin target/debug/nimbus` | Pass. 7/7 interruption and credential-cleanup cases. |
| `node scripts/examples-verify-benchmark.mjs validate --verdict .../avr9-benchmark/verdict.json` | Pass. Recomputed eight samples and a passing verdict. |
| `bash scripts/verify-docs-app-verification.sh --task AVR9` | Pass. AVRC23 1/1. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 verifier mutations failed closed. |
| Bash syntax, ShellCheck, Node syntax, and Actionlint | Pass. No diagnostics. |
| Technical-writing lint | Pass. Plan and proof have zero diagnostics. The routing index stays at its unchanged 44-diagnostic baseline. |
| Docs checks and site build | Pass. 109 pages were link-clean, 110 HTML files built, and the site verifier passed 17/17. |
| `git diff --check` | Pass. |

The benchmark evaluator also proves rejection for invalid hosts, coverage or
order drift, independent relative and absolute budget failures, and duplicate
ports. The scheduler suite uses the real Nimbus binary. It does not mock the
network, process, report, or cleanup contracts.

## Benchmark Campaign

The retained evidence is under `avr9-benchmark/`:

- `raw/` contains three serial and five parallel sample records.
- `runs/` contains each sample's validated `report.json` and `junit.xml`.
- `verdict.json` contains the host-valid derived verdict.

| Field | Value |
| --- | --- |
| Host | `minicloud`; Linux `6.12.94+deb13-amd64`; x64; 4 CPUs; 8,246,829,056 bytes RAM |
| CPU | Intel Core i5-5200U |
| Node | v24.16.0 |
| Binary | `nimbus 0.1.45`; SHA-256 `9f5377b610925d9d8bedf1edc0f1e95e30e8e0ced5ca50f8c16880c90eab9fab` |
| Manifest | Schema 1; SHA-256 `9fd462b34d03d8af214f98aff26335636d7e89ee9af0221aa413bfac3c1c4a77` |
| Source | Before and after SHA-256 `c9527d75489f8f38b087c649bc44a584acd0168dd939bdeff2465579688ff2a7` |
| Serial | 3/3 valid; median 112,403 ms |
| Parallel | 5/5 valid; median 67,066 ms; ratio 0.5966566728646032 |
| Coverage | 9 cases and 37 anchors in the same order in all eight samples |
| Ports | Nine distinct provider-assigned ports in every sample |
| Outcomes | Every report, source check, and cleanup passed |

Two preflights observed one-minute load per CPU of 1.620 and 1.630, above the
1.5 threshold. The benchmark retained them as invalid preflights, waited for an
eligible host, and still collected exactly eight valid samples. No invalid
sample counted toward the median or produced a false product failure.

## CI and Ownership Boundary

The Examples Verify job sets `NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL=5`. Serial
mode remains available with the default value of one for diagnosis. The
scheduler coordinates tests only. It does not allocate ports, bind listeners,
or create a second network authority. Nimbus providers still assign and retain
real listener leases. AVR9 adds no product, transport, credential, or policy
authority.

The first remote debug build exhausted the 8 GB host during the final link.
Rebuilding the same development binary with debug information and incremental
artifacts disabled completed with four compile jobs. This was a host build
resource correction, not a product behavior change.
