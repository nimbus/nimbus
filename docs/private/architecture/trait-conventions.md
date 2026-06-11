# Trait Conventions

Nimbus uses traits for two different jobs:

1. Static capability boundaries where the compiler knows the concrete backend.
2. Runtime extension boundaries where the service stores a `dyn Trait`.

These jobs have different rules. Keeping them separate avoids the two common
failure modes: forcing every capability through `BoxFuture`, or accidentally
making an object-erased trait impossible to call through `dyn`.

## Static Capability Traits

Static-dispatch traits may use `async fn` when the trait is not stored behind
`dyn`. This is the preferred shape for storage capability traits that are
selected by provider type, such as tenant lifecycle, point reads/writes, range
scans, durable journal access, scheduler storage, control-plane surfaces, and
key-provider surfaces.

Rules:

- Use `async fn` only when no call site needs `Box<dyn Trait>`,
  `Arc<dyn Trait>`, or `&dyn Trait`.
- Do not add object-safe wrappers for style. Add them only when an actual
  runtime boundary needs type erasure.
- Use composite traits only where a caller genuinely requires the full surface.
  Backend capability groups should stay focused.

## Object-Erased Traits

Any trait used as `dyn Trait` must be object-safe. Async work at that boundary
returns a named boxed future alias or `BoxFuture`, not `async fn`.

Rules:

- A `dyn` trait method that is asynchronous returns
  `Pin<Box<dyn Future<Output = T> + Send + 'a>>` or a local alias such as
  `BoxFuture<'a, T>`.
- Synchronous `dyn` traits are acceptable when the operation is intentionally
  synchronous, for example observers, catalogs, clocks, fault injection, and
  local key providers.
- Factory traits may return `Box<dyn ChildTrait>` when the child trait owns
  runtime state or thread-affine state.
- Add `Box<dyn Trait>` or `Arc<dyn Trait>` ergonomic impls only when they
  remove repeated glue at real call sites.
- Standard library/provider trait objects such as `dyn FnMut`,
  `dyn std::error::Error`, `dyn Read`, `dyn Stream`, and SQL provider
  parameter traits follow their owning crate's conventions and are not Nimbus
  architecture traits.

## Review Checklist

When adding or changing a trait:

- Search for `dyn TraitName` before choosing `async fn`.
- If the trait is object-erased and async, introduce a named future alias near
  the trait.
- If the trait is static-dispatch only, keep the simpler static shape.
- Document any new runtime/plugin boundary in the owning architecture or plan
  proof.
