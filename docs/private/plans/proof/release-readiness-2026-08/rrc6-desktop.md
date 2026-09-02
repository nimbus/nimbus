# RRC6 Desktop Application Evidence

Date: 2026-09-02

Result: provisional pass. The repaired desktop source, automated desktop
matrix, packaged macOS application, real operator workflow, and hosted
cross-platform packaging pass. The hosted macOS lane also proves signing,
notarization, stapling, and validation. RRC6 stays provisional until the
complete application flow passes against the final Nimbus candidate.

## Inputs

- Nimbus campaign branch: `codex/release-readiness-2026-08` at
  `92fbd821f` for the server and embedded-UI repairs.
- Desktop campaign branch: `codex/release-readiness-2026-08` at
  `bbc103f`.
- Provisional integrated Nimbus binary:
  `/private/tmp/nimbus-release-candidate-875c1dc65b4d/nimbus`.
- Provisional binary version: `nimbus 0.1.45`.
- Provisional binary SHA-256:
  `875c1dc65b4dec6a72fda5518628b0c417bb9c3416bf0ed7ab93f6c57cf0df0f`.
- The RRC1 local-Deno worktree and committed release-readiness repair set
  produced the integrated binary. It is not the exact clean candidate because
  the Deno WebSocket egress commits do not yet have reachable immutable
  references.

Cleanup removed the temporary 51.2 GiB Cargo target after verification recorded
the candidate hash and runtime. The preserved binary has only macOS system
dynamic library dependencies and still reports version `0.1.45`.

## Fail-Before and Repairs

1. A fixed server port caused a collision with an existing listener. The
   desktop-owned process now requests port `0` and consumes Nimbus discovery.
2. A desktop-owned server could exit before discovery without an actionable
   shell error. The launcher now reports `ServerStartExitedError` immediately.
3. A packaged GUI launch starts with `/` as its current directory. Nimbus then
   tried to create `/data` and failed with a read-only-filesystem error. The
   desktop now starts Nimbus with an explicit writable data directory under
   the application's `userData` root.
4. A persisted session cookie became invalid after a local-server restart.
   Protected UI navigation returned raw `401` JSON and a connected desktop
   could remain indefinitely on the disconnected overlay. Nimbus now redirects
   invalid protected-page navigation to `/ui/auth`. The overlay probes `/ui/`
   while disconnected and navigates the full page to sign-in after an
   authentication response. API and WebSocket authentication remain
   fail-closed.
5. The command-palette E2E test sent a physical `KeyK` token instead of the
   product shortcut. It now uses the platform modifier plus `k`.
6. The DS1 probe expected an obsolete placeholder instead of the current
   `/ui/` loopback contract. The expectation now follows the product route.
7. The fuse verifier did not invoke the implemented audit, and DS6 omitted the
   fuse gate. Both paths now inspect the packaged Electron fuse strip.
8. Native branding did not have one explicit product and executable contract.
   The package now uses product name `Nimbus Desktop` and executable name
   `nimbus-desktop`. The built product confirms the bundle, executable, helper
   names, native menu, About, and Quit behavior.
9. DS3 could pass without proving that the desktop-owned Nimbus child stopped.
   The probe now isolates `HOME`, `TMPDIR`, and `userData`. It discovers the
   exact child record and verifies command identity. It cleans the process
   group and direct PID within fixed bounds. It fails and preserves diagnostics
   when the cleanup proof is absent.
10. Launcher tests did not pin the full server argument contract. The unit
    test now asserts the literal host, port, and data-directory arguments.

## Automated Evidence

### Nimbus Server and Embedded UI

| Check | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `git diff --check` | pass |
| Nimbus UI lint | pass, 235 files |
| Nimbus UI typecheck | pass |
| Nimbus UI unit suite | pass, 97 files and 845 tests |
| `nimbus-server` local-UI suite | pass, 14 of 14 tests |
| Restart regression | pass. The cookie is authorized before restart, and stale navigation redirects after restart. |

The embedded-UI regression set covers revoked, expired, missing, and otherwise
invalid session navigation. It covers unreachable-server retry,
reauthentication, in-flight probe cancellation after reconnection, and the
initial no-connection state.

### Desktop Repository

| Check | Result |
|---|---|
| `npm run lint` | pass, 40 files |
| `npm run typecheck` | pass |
| `npm test` | pass, 17 files and 186 tests |
| `npm run test:e2e` | pass, 5 of 5 tests |
| `npm run verify:ds1` | pass, 8 of 8 checks |
| `npm run verify:ds2` | pass, 8 of 8 checks |
| `npm run verify:ds3` | pass, six fuses plus main, renderer, spawned-server discovery, and exact cleanup |
| `npm run verify:ds4` | pass, 11 of 11 checks |
| `npm run verify:ds5` | pass, 20 of 20 checks |
| `npm run verify:ds6` | pass for a clean universal package, signing, fuse audit, and size bounds |
| `codesign --verify --deep --strict` | pass |
| `lipo -archs` | pass, `x86_64 arm64` |
| `hdiutil verify` | pass |

One full E2E run had a command-palette focus timing failure. Its isolated retry
passed, and a new complete 5-of-5 run passed. No expectation changed.

## Packaged Artifact Evidence

The final clean package output contained only the intended universal DMG, ZIP,
their update metadata, and the unpacked application used by the gates.

| Artifact | Size | SHA-256 |
|---|---:|---|
| `release/nimbus-desktop-0.1.0-universal.dmg` | 220,175,135 bytes | `04b761f73159232cb5f153efe33cdde14c6ac14265233a7fc7a76121f3b74949` |
| `release/nimbus-desktop-0.1.0-universal-mac.zip` | 219,613,531 bytes | `a03ae7a87b78f456af835b617abf8147c45b32b14dcb399ad0ace9d69bb1e585` |

The application is Developer ID signed. Local packaging skipped notarization
and stapling because the Apple API issuer, key identifier, and key material
were not in the environment. The release workflow contains the expected secret
names, but configuration presence is not notarization evidence.

## Real macOS Operation

Local computer control mounted the clean universal DMG and launched its exact
application.

1. The native application menu displayed `Nimbus Desktop`.
2. The desktop started its owned server with:
   `/private/tmp/nimbus-ws-test.0rXOFY/worktree/target/debug/nimbus start
   --host 127.0.0.1 --port 0 --data-dir /Users/jack/Library/Application
   Support/Nimbus Desktop/server`.
3. The application rendered the sign-in page on the discovered ephemeral
   loopback port.
4. Sign-in reached the connected operator UI. The overview reported Nimbus
   `0.1.45` and rendered all six overview panels.
5. The test exercised developer routes, operator routes, both palettes, tenant
   selection, and the system-tenant lens.
6. Stopping the server displayed `Reconnecting` with the explicit stale-data
   and mutation-disabled state.
7. Restarting on the same port caused the disconnected-session probe to route
   to sign-in in approximately 2.8 seconds. It did not expose raw JSON or stay
   disconnected indefinitely.
8. Native-menu Quit stopped both the desktop application and its owned Nimbus
   child. No tested process, mounted DMG, or generated database remained.

## Independent Review

Opus 5 reviewed the exact desktop commits and the Nimbus UI/server repair.

- Nimbus review of `9fb527763` found no implementation defect and requested
  two P3 lifecycle tests. Commit `92fbd821f` added both. The follow-up review
  accepted no P0 through P3 finding.
- Desktop review of `618707d` produced two accepted P3 test/cleanup findings
  and two refuted findings. Commits `21800be`, `de63d23`, and `bbc103f`
  strengthened exact child cleanup, bounded joint discovery, literal spawn
  arguments, and preserved failure evidence. The final review of `bbc103f`
  accepted no P0 through P3 finding.
- Source inspection refuted the claimed discovery relocation defect. Nimbus
  local authentication and discovery use platform-native roots, not the
  persistence data directory. The real package launch also proved discovery.
- The universal bundle refuted the claimed product-name and executable
  mismatch. The evidence includes helper names, the native menu, and clean
  quit behavior.

## RRC6 Decision

The desktop application and its repair set have a provisional pass. Hosted run
`33593752690` passes at Desktop commit
`8dc9eaa7b858e50f40751b51526e585a87953b83` on macOS 14, Windows 2022, and
Ubuntu 24.04. It uses publish mode `never`. macOS signing, notarization,
stapling, and validation pass. Windows produces x64, arm64, and universal
installers. Linux produces AppImage, deb, and RPM packages. All platform fuse,
size, and artifact-upload gates pass. No new GitHub Release exists.

RRC6 has one remaining condition: repeat the complete application and recovery
flow against the final Nimbus candidate. Until that condition has direct
evidence, RRC6 stays blocked and the release verdict cannot be GO.
