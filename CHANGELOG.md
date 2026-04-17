# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.9] - 2026-04-17

### CI/CD

- Verify release tags against crate, package, lockfile, and changelog versions by @jackspirou

### Documentation

- Archive the completed macOS machine support plan and promote the stable macOS flow reference by @jackspirou

### Fixed

- Align shipped `neovex --version` metadata with the next release tag across Rust crates and JS workspaces by @jackspirou



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.8...v0.1.9

## [0.1.8] - 2026-04-16

### CI/CD

- Opt release workflow into node24 actions by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.7 by @github-actions[bot]



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.7...v0.1.8

## [0.1.7] - 2026-04-16

### CI/CD

- Make machine-os watcher attempt-aware by @jackspirou
- Document rerun-safe artifact naming by @jackspirou
- Stabilize machine-os staged artifact naming by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.5 by @github-actions[bot]



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.6...v0.1.7

## [0.1.6] - 2026-04-16

### CI/CD

- Release machine-os before neovex by @jackspirou



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.5...v0.1.6

## [0.1.5] - 2026-04-15

### CI/CD

- Dispatch machine-os publish workflow by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.4 by @github-actions[bot]



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.4...v0.1.5

## [0.1.4] - 2026-04-15

### Build

- Use stable machine-os workflow ref by @jackspirou
- Repin machine-os workflow refs by @jackspirou
- Cache rusty_v8 artifacts by @jackspirou
- Repin machine-os performance updates by @jackspirou
- Shorten release critical path by @jackspirou
- Fix machine-os workflow pin by @jackspirou
- Reuse staged machine-os release bundles by @jackspirou
- Switch machine-os release flow to app auth by @jackspirou
- Repin machine-os reusable workflow by @jackspirou
- Use reusable machine-os release workflow by @jackspirou
- Dispatch native machine-os releases by @jackspirou

### CI/CD

- Harden workflow timeouts and permissions by @jackspirou

### Documentation

- Update CHANGELOG.md for v0.1.3 by @github-actions[bot]

### Fixed

- Grant reusable machine-os workflow write access by @jackspirou
- Pin valid machine-os workflow commit by @jackspirou
- Use valid release workflow step ids by @jackspirou
- Match machine-os release run names by @jackspirou
- Account worker load before dispatch send by @jackspirou

### Testing

- Invoke fake buildah via shell launcher by @jackspirou
- Close fake buildah temp path before exec by @jackspirou
- Harden fake buildah helper creation by @jackspirou

### New Contributors
* @github-actions[bot] made their first contribution


**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.3...v0.1.4

## [0.1.3] - 2026-04-15

### Build

- Bump workspace to v0.1.3 by @jackspirou
- Pin machine-os release workflow contract by @jackspirou



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.2...v0.1.3

## [0.1.2] - 2026-04-15

### Build

- Bump workspace to v0.1.2 by @jackspirou

### Fixed

- Narrow windows machine compilation seams by @jackspirou



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.1...v0.1.2

## [0.1.1] - 2026-04-15

### Build

- Bump workspace to v0.1.1 by @jackspirou
- Patch rustls-webpki advisory by @jackspirou

### Fixed

- Gate machine module on unix hosts by @jackspirou



**Full Changelog**: https://github.com/agentstation/neovex/compare/v0.1.0...v0.1.1

## [0.1.0] - 2026-04-15

### Documentation

- Harden machine image release contract by @jackspirou

### Testing

- Derive machine image version from crate version by @jackspirou



**Full Changelog**: https://github.com/agentstation/neovex/compare/machine-os/v0.1.2...v0.1.0

## [machine-os/v0.1.2] - 2026-04-14



**Full Changelog**: https://github.com/agentstation/neovex/compare/machine-os/v0.1.1...machine-os/v0.1.2

## [machine-os/v0.1.1] - 2026-04-14



**Full Changelog**: https://github.com/agentstation/neovex/compare/machine-os/v0.1.0...machine-os/v0.1.1

## [machine-os/v0.1.0] - 2026-04-14

### CI/CD

- Use authenticated googlesource path and update Cargo.lock by @jackspirou
- Add googlesource auth and cache-on-failure to all Rust jobs by @jackspirou
- Add Rust toolchain and cargo cache to deny job by @jackspirou
- Mark all workspace crates as unpublished for cargo-deny by @jackspirou
- Fix deny.toml for workspace custom license and path deps by @jackspirou
- Fix deny.toml for cargo-deny 0.19.0 by @jackspirou
- Fix deny.toml config, add weekly audit schedule, dependabot, and codecov config by @jackspirou

### Documentation

- Add macos machine support control plane by @jackspirou
- Archive external SQL provider plan by @jackspirou
- Restructure repo guidance and codex roadmap control plane by @jackspirou

### Fixed

- Isolate cooperative locker tests and annotate V8 reset repro by @jackspirou
- **deps**: Update Cargo.lock to submodule-free rusty_v8 tag by @jackspirou

### Miscellaneous

- Checkpoint remaining workspace changes by @jackspirou

### Testing

- Ignore snapshot-aware reset repro that SIGABRTs on cycle 2 by @jackspirou

### New Contributors
* @jackspirou made their first contribution


<!-- generated by git-cliff -->
