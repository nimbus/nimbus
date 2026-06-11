# Sandbox Architecture

Start with
[`service-sandbox-session-model.md`](service-sandbox-session-model.md) for the
visual primer and resource vocabulary used across the SDK, service-control, and
sandbox plans.

- [`service-sandbox-session-model.md`](service-sandbox-session-model.md) --
  concept maps plus the rules: services are named tenant dependencies with
  sandbox-backed, built-in, or external implementations; sandboxes are isolated
  execution resources; future sessions are scoped interaction leases; runtime
  isolates are not SDK sandboxes.
- [`microvm-service-baseline.md`](microvm-service-baseline.md) -- landed
  Compose-backed service-control and microVM baseline.
- [`macos-machine-flow.md`](macos-machine-flow.md) -- macOS outer-machine flow
  and guest service execution.
- [`krun-sandbox-backend-smoke.md`](krun-sandbox-backend-smoke.md) and
  [`krun-vmm-host-validation.md`](krun-vmm-host-validation.md) -- host
  validation and smoke-test evidence.
