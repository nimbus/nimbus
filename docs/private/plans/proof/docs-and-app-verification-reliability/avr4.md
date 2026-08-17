# AVR4 Validated Manifest and Disposable Workspaces

Date: 2026-08-17

## Result

AVR4 is complete in work commit
`827877d061d83baa2278176ef97d521c4b01b9eb`. One versioned JSON manifest now
owns the nine application identities, 100 declared inputs, boot modes, smoke
commands, 20 surface entries, and update semantics.

The workspace adapter validates the manifest before effects. It copies each
case into an owned workspace and creates a case-local dependency forest.
Codegen, provisioning, boot, and smoke operations run only in that workspace.
The runner captures tracked worktree bytes and the Git index before effects.
Its exit finalizer compares both on success and failure without a Git restore
operation.

## Fail-before evidence

A disposable export ran the five app cases that used the old
in-place preparation. The cases passed, but six tracked paths changed. Every
recorded SHA-256 digest changed.

| Path | Before | After |
| --- | --- | --- |
| `package-lock.json` | `4d30cf33efc0cac080a3e058f59b5484dc178908ee4ccf1dd36a02bcf162705c` | `d9dace82b0a8d99716d0fedafb508c02ea6229123eebbff57d42a8a6f1df048e` |
| `examples/nimbus/agent-chat/package.json` | `767be431eed706fb28124abc83a07309c837fdc6902e79cd7a41893330e4e985` | `985105d1f8b8f0eea31d215697c326b8d4e19bf1aff08b33aecc2d27357a49ba` |
| `examples/nimbus/agent-worker/package.json` | `4d9922252cebe8d023f9827147c2c586d4b55da8d6fd0f5d3584caf6179680b5` | `668332261f6bf2eb68b5bd0e9fe2125d7936afe89865fa74a194555c737c9c90` |
| `examples/convex/tasks/package.json` | `b6c7162a4807f8bb15523f683d5363a6836f53920b69933c5ea67a2dde948a5c` | `794fbe5c24ab6dedfaf96c5480b2c93334117a28bac2de02e30cda85ed01e8dd` |
| `examples/convex/runtimes/package.json` | `2b33919446910c42013a713ff2ad0a39317e042013cef39b3ae4a89c6b9ed447` | `ccd0b049bb8aa200fa4c3e84db2e27e93079d4ded000348b909536e639d1e868` |
| `examples/firebase/tasks/package.json` | `40bc97b957a1f8f3be95c4217fc53bb54f87e64cbd4f92c775700ff6e169ef61` | `e462a5bacc69720916699c1e9bc2b8e096213195d519f9a256a76232a153e7d8` |

The application manifests changed their workspace dependencies to disposable
`file:./.nimbus/packages/...` paths. The root lockfile recorded those changes.
This proved that a green application result did not prove source immutability.

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR4.1 Define nine cases. | Pass. | The manifest validates nine unique names, workspaces, and application paths. It records 100 inputs, 11 unique surfaces, four codegen cases, two dev cases, and three update modes. |
| AVR4.2 Copy declared inputs. | Pass. | All nine preparation fixtures compare every copied file or link byte-for-byte. The validator rejects missing, extra, duplicate, escaping, and shell-unsafe fields. |
| AVR4.3 Run codegen in the owned workspace. | Pass. | The final staged-candidate `nimbus/agent-chat` run passed 4/4 assertions. Its finalizer reported `source byte manifest matches`. |
| AVR4.4 Record source bytes and the index. | Pass. | Git mode hashes every tracked worktree file and the exact staged index. Export mode hashes the manifest, guarded root files, and all declared inputs. |
| AVR4.5 Compare on every exit. | Pass. | Success, dirty-source failure, staged-source failure, deliberate mutation, and an invalid live selector all execute the compare contract. No source restoration occurs. |

## Behavioral evidence

| Case | Result |
| --- | --- |
| `manifest_rejects_duplicate_or_incomplete_case` | Pass, including escaping workspace and shell-delimiter rejection. |
| All nine preparation fixtures | Pass. All declared bytes match. |
| `dirty_source_bytes_survive_success` | Pass. |
| `dirty_source_bytes_survive_failure` | Pass. |
| `staged_source_bytes_survive_failure` | Pass. The index tree and worktree bytes match the pre-failure state. |
| `source_byte_manifest_detects_mutation_without_restore` | Pass. The verifier names the changed file and leaves its changed bytes present. |
| Live `nimbus/agent-chat` under Node.js 22 | Pass. Codegen plus 4/4 smoke assertions completed in the disposable workspace. |
| Live `nimbus/tasks` under Node.js 22 | Pass. The plain start path completed 5/5 smoke assertions. |
| Live `firebase/tasks` under Node.js 22 | Pass. The dev and provision path completed 5/5 smoke assertions. |
| Invalid live selector | Expected failure. It named all nine valid cases and reported `source byte manifest matches`. |

## Verification evidence

| Command or check | Result |
| --- | --- |
| `bash -n scripts/examples-verify.sh scripts/examples-verify-contract-test.sh` | Pass. |
| `node --check scripts/examples-verify-workspace.mjs` and its test | Pass. |
| `shellcheck scripts/examples-verify.sh scripts/examples-verify-contract-test.sh` | Pass with no diagnostics. |
| `node scripts/examples-verify-workspace-test.mjs` | Pass. 6/6 behavior tests and all nine preparation fixtures. |
| `bash scripts/examples-verify-contract-test.sh --task AVR4` | Pass. AVRC13-AVRC15 are 3/3, plus 6/6 behavior tests and nine fixtures. |
| `bash scripts/verify-docs-app-verification.sh --task AVR4` | Pass. AVRC13-AVRC15 are 3/3. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 mutations fail closed. |
| Three representative live success shapes | Pass. 14/14 smoke assertions and three matching source-byte finalizers. |
| Live failure finalizer | Pass. The expected nonzero selector failure retained status 1 and matched source bytes. |
| Root dependency links after live work | Pass. `@nimbus/nimbus`, `convex`, and `firebase` remain present. |
| `git diff --cached --check` | Pass. |
| Biome path check | `UNVERIFIED`: repository configuration excludes these scripts. Node syntax, behavioral tests, JSON parsing, Bash syntax, and ShellCheck cover the owned paths. |

## Resolved implementation finding

An early adapter linked the complete root `node_modules` directory into each
case. Nimbus provisioning followed that directory link and removed package
links from the owner dependency tree. The final adapter creates a real
case-local `node_modules`, real scope directories, and a real `.bin` directory.
It links each dependency entry separately. The fixture deletes and refreshes a
case-local package link and proves that the source dependency remains present.

The current runner still renames `compose.yaml` for the two dev cases. AVR5
owns its replacement with an explicit Compose-discovery opt-out.
