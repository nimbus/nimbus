# RRC6 Desktop Application Evidence

Date: 2026-08-28

Result: provisional pass. The repaired desktop source, automated desktop
matrix, packaged macOS application, and real operator workflow pass. RRC6
cannot become an exact-candidate pass until RRC1 has reachable immutable Deno
references. Apple notarization and stapling also remain unverified because the
required Apple API credentials are not available in the local environment.

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
- The integrated binary was built from the RRC1 local-Deno worktree plus the
  committed release-readiness repair set. It is not the exact clean candidate
  because the Deno WebSocket egress commits do not yet have reachable
  immutable references.

The temporary 51.2 GiB Cargo target was removed after the candidate hash and
runtime were verified. The preserved binary has only macOS system dynamic
library dependencies and still reports version `0.1.45`.

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
   while disconnected and performs a full-page sign-in navigation for an
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
   `nimbus-desktop`. The bundle, executable, helper names, native menu, About,
   and Quit behavior were verified from the built product.
9. DS3 could pass without proving that the desktop-owned Nimbus child stopped.
   The probe now isolates `HOME`, `TMPDIR`, and `userData`, discovers the exact
   child record, verifies command identity, performs bounded process-group and
   direct-PID cleanup, and fails while preserving diagnostics when cleanup is
   not proved.
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
| Restart regression | pass; the cookie is authorized before restart and stale navigation redirects after restart |

The embedded-UI regression set covers revoked, expired, missing, and otherwise
invalid session navigation; unreachable-server retry; reauthentication;
in-flight probe cancellation after reconnection; and the initial no-connection
state.

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
passed, and a new complete 5-of-5 run passed. No expectation was weakened.

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

The clean universal DMG was mounted and its exact application was launched
with local computer control.

1. The native application menu displayed `Nimbus Desktop`.
2. The desktop started its owned server with:
   `/private/tmp/nimbus-ws-test.0rXOFY/worktree/target/debug/nimbus start
   --host 127.0.0.1 --port 0 --data-dir /Users/jack/Library/Application
   Support/Nimbus Desktop/server`.
3. The application rendered the sign-in page on the discovered ephemeral
   loopback port.
4. Sign-in reached the connected operator UI. The overview reported Nimbus
   `0.1.45` and rendered all six overview panels.
5. Developer routes, operator routes, both palettes, tenant selection, and the
   system-tenant lens were exercised.
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
- The claimed discovery relocation defect was refuted by source inspection:
  Nimbus local authentication and discovery use platform-native roots, not the
  persistence data directory. The real package launch also proved discovery.
- The claimed product-name/executable mismatch was refuted by the actual
  universal bundle, executable, helper names, native menu, and clean quit.

## RRC6 Decision

The desktop application and its repair set have a provisional pass. The two
remaining release conditions are not desktop implementation defects:

1. Replay this complete matrix against a clean exact candidate after RRC1 has
   reachable immutable Deno references.
2. Run Apple notarization and stapling with the authorized release
   credentials, then verify the stapled DMG and application.

Until both conditions have direct evidence, RRC6 stays blocked and the release
verdict cannot be GO.
