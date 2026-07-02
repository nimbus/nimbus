# Third-Party Dependency Patches

This directory contains local crates.io patches used only when an upstream crate
has no released version carrying a required fix.

- `object_store-0.14.0` and `s3s-0.14.0` are copied from crates.io releases and
  keep their upstream source unchanged except for the `quick-xml` dependency
  floor. Nimbus pins them locally so `quick-xml >= 0.41.0` resolves for
  RUSTSEC-2026-0194 while upstream releases still require `quick-xml 0.40.x`.

Remove these patches once the upstream crates publish compatible releases with
the fixed `quick-xml` dependency.
