# Plan Review Resolution

Date: 2026-08-17
Scope: plan-only correction before activation

| Review finding | Resolution |
|---|---|
| Shared operator state makes parallel cases cross-discoverable. | AVRF17, invariants 13-14, AVRD6, AVRC19-AVRC20, and AVR7 now require case-local operator roots with one run-global network root and product provider-assigned listener leases. |
| The staged plan could be reconciled before it became durable. | The promotion gate and goal now require the plan checkpoint in `HEAD` and a `git cat-file -e` proof before fetch or reconciliation. |
| Phase PRs, one implementation PR, and two final PRs conflicted. | AVRD7 and the campaign table define three implementation PRs plus one cleanup PR, with merge and reconciliation checkpoints. |
| The verifier had only aggregate counts. | The contract now defines AVRC01-AVRC24, task ownership, phase counts, terminal output, and one meaningful mutation per condition. |
| Status-only source checks could miss changed dirty bytes. | AVRF13, AVRC13-AVRC15, and AVR4 now require pre/post byte manifests for tracked, staged, and relevant untracked inputs. |
| The relative performance target could exceed hosted timeout. | AVRC23 and AVR9 now require both 60% of serial median and an absolute 1,200-second ceiling with controlled-host evidence. |
| The goal named a missing runbook. | AVRF18 and AVR0 now own all six broken private bootstrap routes. The goal reads them after AVR0 creates and verifies them. |
| Known stale paths and S3 port 9000 were absent. | AVRF05/AVRF08 and AVR1/AVR2/AVR7 now name the active architecture review, archived examples status, public concepts index, and S3 listener. |
| Direct script invocation was outside fresh-checkout acceptance. | AVRF02, AVRC11-AVRC12, and AVR3 now define and test direct and Make entry points. |
| Tasks lacked exact actions and commands; decisions lacked evidence. | Every task has numbered actions, the command-contract table names stable entry points, and AVRD1-AVRD8 are dated with evidence, consequences, and re-open conditions. |
| Review, discovery, and scope controls were incomplete. | The goal now loads the Nimbus autoreview and gh skills, routes discoveries, applies the two-times scope tripwire, and permits one review per frozen phase candidate. |

This correction includes no product implementation. The checkpoint passed all
plan and documentation gates and promoted the owner plan to `active`.
