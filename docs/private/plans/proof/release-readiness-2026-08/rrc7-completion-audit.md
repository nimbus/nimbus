# RRC7 Supported Release Completion Audit

Date: 2026-09-08
Status: incomplete pending PR #330 merge and RRC99 cleanup.

## Scope

The owner authorized the supported release after the broader QA campaign.
This audit covers the immutable release graph, published artifacts, channel
repairs, and available installation evidence. It does not convert an absent
external-provider proof into a pass.

The [publication proof](rrc7-v0.1.47-publication.md) records exact component
identities, archive hashes, install commands, reviews, and exclusions.

## Requirements and Evidence

| Requirement | Evidence inspected | Result |
|---|---|---|
| Preserve published tags | Remote v0.1.46 peels to `67a7f3ffa7aa7a226551a4f1f977dd0e7056ddc3`; v0.1.47 peels to `c05b17125a92b79d4faee52eb26d84e2f4f53b42` | Pass |
| Publish the verified Nimbus candidate | Release run `34086200385` uses exact `c05b17125` and passed all 12 jobs | Pass |
| Publish supported archives and support assets | Public v0.1.47 is neither draft nor prerelease and contains ten assets | Pass |
| Preserve archive integrity | All three public archive digests match the publication proof and public cask | Pass |
| Retain license and supply-chain evidence | Release assets include `LICENSE`, checksums, installer, OCI attestation, image report, SBOM, and vulnerability report; the live verifier passed these checks | Pass |
| Smoke-test the public OCI image | Unchanged repository smoke passed on minicloud against the exact published digest; the local no-Podman exit remains recorded separately | Pass |
| Publish downstream machine-os | Run `34092260758` passed its Linux arm64 publication job at machine-os `4e7f7ee67fe29788e0c39247b43da7dd200174e0` | Pass |
| Publish signed apt packages | Run `34187740409` passed all six jobs from Nimbus `c05b17125`; the fresh install verified the pinned public-key fingerprint | Pass |
| Verify the apt upgrade | Real v0.1.45 packages upgraded through signed public apt to the exact Nimbus, crun, and libkrun tuple | Pass |
| Verify Homebrew install and upgrade | Real v0.1.45 upgrade and v0.1.47 reinstall passed; public hook commit `661071c` passed direct execution and Sol review | Pass |
| Repair automatic apt dispatch | Exact regression and Sol review pass for `e62e0cf76`; main CI passes, but PR #330 awaits Rust CodeQL and merge | Incomplete |
| Keep review and privacy constraints | Isolated Sol reviews found no remaining actionable issue; no Opus or Fable review ran; additional private plan text stayed local | Pass |
| Close the control plane | RRC7 remains active and RRC99 awaits the final repair merge | Incomplete |

The ten release assets comprise three archives and seven support files.
The seven files are `install.sh`, `LICENSE`, `checksums-sha256.txt`,
`nimbus_oci_attestation.json`, `nimbus_oci_image.txt`, `nimbus_oci_sbom.json`,
and `nimbus_oci_vulns.sarif.json`.

## Boundaries and Residual Owners

- `distribution-plan.md` owns COPR credentials, its public-project contract,
  the first public Fedora install, and future apt signing-key rotation.
- The four absent public-cloud comparison endpoints remain evidence debt
  recorded in the [historical verdict](rrc8-release-verdict.md).
- Windows Nimbus archives and the optional Bun adapter remain outside this
  product release matrix. Their skips are not installation passes.
- Desktop packaging has a separate release cadence. RRC6 and the exact
  desktop UI run `34030233538` retain this campaign's desktop validation.
- `runtime-strategy-lifecycle-plan.md` stays proposed. This release does not
  activate it or start a new runtime experiment.

## Next Action

CI run `34190502568` passed 48 jobs and declared four expected skips.
Its full job roster is [rrc7-channel-ci.json](rrc7-channel-ci.json).
CodeQL run `34190502684` still has an active Rust analysis job.

Merge PR #330 only after its required checks pass. Record the exact merge,
verify the public workflow repair, and complete RRC99 in a docs-only change.
