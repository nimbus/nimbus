# RRC7 v0.1.47 Public Release

Date: 2026-09-08

Result: the authorized supported release is public. The signed apt installation
passes. This record supersedes the publication blockers in the v0.1.46 proof.
It does not change the historical 46-condition matrix or claim COPR coverage.

## Immutable Identity

| Component | Identity |
|---|---|
| Nimbus | `v0.1.47`, merge `c05b17125a92b79d4faee52eb26d84e2f4f53b42`, PR #329 |
| Deno | `v2.9.6-nimbus.5`, `95413e012ee9f73e7f652e1e7b1ad9e351b9a8df` |
| rusty_v8 | `v150.4.0-nimbus.2`, `a90dfd667605057d648521e36791ef65eb275a9d` |
| OCI | `ghcr.io/nimbus/nimbus@sha256:09f479cbaecd51a1ad894d3d6038f09e5520e3c1b0f9a90a19210930d11c7fa3` |

The v0.1.46 tag remains unchanged. Its release failed before publication because
the V8 archive required newer glibc symbols than Ubuntu 22.04 provides.
The replacement V8 archive preserves the Ubuntu 22.04 compatibility floor.

## Hosted Evidence

| Check | Result | Source |
|---|---|---|
| PR #329 checks | 53 successful checks, four expected skips | [Pull request](https://github.com/nimbus/nimbus/pull/329) |
| V8 release | Seven target jobs passed, 44 payloads and 44 checksum sidecars verified | [Run 34068851857](https://github.com/nimbus/rusty_v8/actions/runs/34068851857) |
| Nimbus release | All 12 jobs passed | [Run 34086200385](https://github.com/nimbus/nimbus/actions/runs/34086200385) |
| machine-os | Exact upstream release run passed | [Run 34092260758](https://github.com/nimbus/machine-os/actions/runs/34092260758) |
| Signed apt publication | All six jobs passed | [Run 34187740409](https://github.com/nimbus/nimbus/actions/runs/34187740409) |

The [public release](https://github.com/nimbus/nimbus/releases/tag/v0.1.47)
contains the macOS arm64 and both Linux archives. Windows and the optional
Bun adapter remain outside this product release matrix.

## Archive and OCI Checks

The live verifier passed archive layout, checksum coverage, license presence,
optional-adapter absence, SBOM evidence, vulnerability evidence, and remote
attestations. Its local OCI smoke stopped with exit 125 because Podman had no
configured machine or socket. The complete local command did not pass.

The unchanged repository OCI smoke then passed on `nimbus@192.168.4.29`.
It used the exact published digest above and expected version `v0.1.47`.
It verified UID and GID 10001, a writable disposable volume, license presence,
token rotation, startup, and health. It also checked for excluded host tools.
The script removed its disposable container and volume.

| Archive | SHA-256, also matched against the public Homebrew cask |
|---|---|
| macOS arm64 | `9aae21cdd452e7c91d942eb8d4f1f86d5e0db0e6c80f1b90891c8c2d4f981b3d` |
| Linux x86_64 | `38bf16d1dbf9d591ba0a1c2490efed45d9fffa822eac136ab6c7104c4f82147f` |
| Linux arm64 | `24040e7a44531dc025ec924b5f0ec76517022271785c18086fe1b30d739d768b` |

## Homebrew Upgrade

`brew upgrade --cask nimbus/tap/nimbus` exited zero and upgraded the local
installation from v0.1.45 to v0.1.47. `/opt/homebrew/bin/nimbus --version`
reported `nimbus 0.1.47`.

Homebrew reported the deprecated `postflight` cask API. The upgrade still
passed. The distribution plan owns the cask and generator cleanup.

## Signed Apt Installation

The owner authorized a dedicated apt signing key. The public fingerprint is
`F6F4B82AB4D0B512AF2282FB63CB0B8B022DCD9B`.
The key uses RSA 3072 and expires after two years. GitHub Actions stores the
private export in `APT_REPOSITORY_SIGNING_KEY`. This proof contains no private
key material.

The key backup and revocation certificate remain outside the repository in
owner-only storage. This public proof does not record their location.

A disposable Debian 13 slim container fetched
`https://nimbus.github.io/nimbus/public/nimbus.gpg`. It checked the exact
fingerprint before it configured this source:

```text
deb [signed-by=/usr/share/keyrings/nimbus.gpg] https://nimbus.github.io/nimbus stable main
```

Normal apt signature verification passed. The fresh install exited zero with
this exact package tuple:

| Package | Version |
|---|---|
| `nimbus` | `0.1.47` |
| `nimbus-crun` | `1.29.1~nimbus.2` |
| `nimbus-libkrun` | `1.19.4~nimbus.3` |

The binary reported `nimbus 0.1.47`. The installed license was nonempty.
The fixture retained Nimbus documentation through an explicit dpkg include.
The first attempt lacked that include and failed the license assertion because
Debian slim excludes documentation. Package inspection confirmed the license
was present in the published DEB. The corrected fixture kept the assertion.

```text
PASS: signed public apt install and exact runtime package tuple
```

## Publication Recovery and Remaining Work

The automatic apt run `34092821400` could not deploy from tag `v0.1.47`.
The Pages environment permits deployments from `main` only.
Dispatch from `main` with explicit input `release_tag=v0.1.47` passed.
The recovery preserved Pages protection and the immutable source tag.

The distribution plan owns these remaining checks and repairs:

- Merge the dispatch repair described below.
- Replace the deprecated Homebrew hook after verification against its current
  API.
- Maintain signing-key backup and rotation procedures.

COPR remains disabled pending credentials and its public-project contract.
Four public-cloud comparison endpoints remain unavailable. These exclusions
remain visible and do not count as passing evidence.

RRC7 must reconcile this proof with the active ledger before RRC99 archives
the campaign. The historical broad NO-GO verdict must remain distinct from
the owner's authorized supported release.

## Dispatch Regression Repair

The local workflow now dispatches from `main` and retains the exact release
tag input. The regression executes the real dispatch body with a recording
GitHub client. It checks the complete argument list and exactly one call.
The prior workflow failed with `FAIL: expected main, got v0.1.47`.
The corrected workflow passes. Hosted CI now includes this regression.

Bash syntax, ShellCheck, actionlint, and whitespace checks pass locally.
The repair passed review and still needs merge. It does not change the published tag
or redeploy the current release.

The first pre-PR review did not start. The harness rejected external transmission
of the private diff to Sol without explicit approval for that destination.
No Opus or Fable review ran. Commit `72f587454` contains the tested repair.

The current apt index contains only `nimbus` version `0.1.47`.
The exact apt upgrade test therefore used the older package artifact below.

## Signed Apt Upgrade

The exact upgrade passed on `nimbus@192.168.4.29` in a disposable Debian 13
slim container. Run `28807490870` retained artifact `8115541752`, named
`linux-packages-amd64`, with the real v0.1.45 packages. All six archived
package checksums passed before transfer. The prior Nimbus DEB hash is
`1036f704e9bf7aa5418e275eaf07bcfacc0a451ac19b6b32260f79bb3ada0007`.

The fixture installed and asserted the older tuple before it configured apt.
It pinned the public signing-key fingerprint above. Normal apt verification
accepted the signed repository and downloaded the upgrade packages.

| Package | Before | After |
|---|---|---|
| `nimbus` | `0.1.45` | `0.1.47` |
| `nimbus-crun` | `1.27.1~nimbus.2` | `1.29.1~nimbus.2` |
| `nimbus-libkrun` | `1.18.1~nimbus.1` | `1.19.4~nimbus.3` |

The test asserted each installed version, executed the binary, and checked
the installed license. The shell returned zero. Podman removed the container.
The host package database did not change.

```text
PASS: signed apt upgrade 0.1.45 to 0.1.47 and exact runtime tuple
```

Local command and raw output remain at
`/private/tmp/nimbus-v0147-apt-upgrade.sh` and
`/private/tmp/nimbus-v0147-apt-upgrade.log`. This test proves package upgrade
behavior. It does not claim application-data migration or a live service
upgrade.

## Homebrew Hook Repair

Both local cask generators now use `postflight_steps`. Homebrew 6.0.22 parses
both actual hook bodies into the same declarative command. The regression is
`brew ruby scripts/verify-nimbus-homebrew-install-steps.rb`.
It asserts the macOS guard, exact command, arguments, and install-time path
token. It also rejects privilege escalation or failure suppression.

The old DSL uses `SystemCommand.run!`. The new command retains its required
success contract. It does not remove quarantine from Linux files.
The installed Homebrew source and `docs/Cask-Cookbook.md` define the new API.

The cask proof helper and Bash syntax pass. ShellCheck reports two existing
SC2016 diagnostics in the unrelated child-shell path probes. Their single
quotes deliberately defer argument expansion to that child shell.
The new hook has no ShellCheck diagnostic.

The public tap contains commit `661071c`. PR #330 contains the reviewed
generator repair. Both apply the same hook.

## Sol Review

The owner explicitly approved the isolated Sol review on 2026-09-08.
GPT-5.6 Sol at xhigh reviewed the six-file committed repair. TruffleHog passed.
The reviewer reported one P3 disclosure of the signing-key backup location.
The correction removes that location from this proof. It changes no key
material or runtime behavior.

The exact follow-up at `1d90ae9a3` exited zero. Sol reported no accepted or
actionable P0 through P3 finding, with correctness confidence `0.94`.
TruffleHog passed. The dispatch, Homebrew parser, actionlint, whitespace, and
docs checks pass. This was a manual branch review, not the pre-PR gate.
No Opus or Fable review ran.

Earlier local commits contain the removed location. Publication must use a
new branch from public main with only the corrected final diff. Do not push
the earlier local commit history.

## Publication Checkpoint

PR #330 contains clean commit `e62e0cf767176a330dee02d211852ac6ec2b6daa`.
Its exact six-file diff passed the Sol xhigh pre-PR gate with no actionable
P0 through P3 finding and correctness confidence `0.95`. TruffleHog passed.
The additional plan checkpoint stays local and was not sent to Sol.

CI run `34190502568` and CodeQL run `34190502684` remain active.
The new dispatch regression passed in hosted Proof Helper Checks.
GitHub reported no failed check at this checkpoint. Merge remains pending.

## Tap Hook Execution

The separate tap worktree contains commit `661071c` on
`codex/nimbus-install-steps`. Its cask keeps version `0.1.47`, all three
archive hashes, and the release URLs unchanged.

Homebrew 6.0.22 loaded the complete cask through `FromContentLoader`.
The actual `Homebrew::InstallSteps::Runner` executed its postflight steps
against a disposable directory with a nested file. The test first asserted
quarantine attributes on all three paths. After execution, those attributes
were absent and the file contents matched the fixture. The runner required
no privilege.

The same runner received a missing staged path. Its command error propagated
instead of reporting success. Ruby syntax and whitespace checks passed.
The execution proof is
`/private/tmp/nimbus-homebrew-postflight-proof.rb`.

This disposable test did not reinstall Nimbus or change the registered tap.
The tap pre-PR Sol xhigh review passed with no actionable P0 through P3
finding and correctness confidence `0.98`. TruffleHog passed.

## Tap Publication

The intended branch push updated remote `main` directly to
`661071c257a563d76fab91879ae77c7c63cd6097`. GitHub then rejected PR creation
because the proposed feature branch did not exist remotely. This published
the reviewed hook before PR #330 merged, contrary to the planned order.

The command used a source-only refspec. The local branch tracks `origin/main`,
and global `push.default` is `upstream`. A disposable local Git 2.55.0 fixture
reproduced this mapping without contacting GitHub. With `push.default=simple`,
the same source-only refspec targeted the feature branch. An explicit source
and destination also targeted the feature branch under `upstream`.
Future pushes must specify both source and destination refs.

No published history was rewritten. `git ls-remote` confirmed the exact commit.

The registered tap had no local changes. Its fast-forward update to that
commit passed. `brew info --cask nimbus/tap/nimbus` accepted the public cask
without the deprecated-hook warning.

`HOMEBREW_NO_AUTO_UPDATE=1 brew reinstall --cask nimbus/tap/nimbus` then
exited zero. It reinstalled the same published v0.1.47 archive with the new
hook and no deprecated-hook warning. `/opt/homebrew/bin/nimbus --version`
reported `nimbus 0.1.47`. The cask has no application-data removal hook.
