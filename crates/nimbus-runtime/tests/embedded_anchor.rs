//! Exercises the EMBEDDED NodeFull(Node22) anchor snapshot install — the serving path the cfg(test)
//! cage oracle cannot reach.
//!
//! Under cfg(test), `snapshot_extensions` adds a test-only extension, so the bootstrap snapshot's
//! provenance no longer matches the committed production blob and `try_embedded` correctly falls back
//! to a runtime build. As an INTEGRATION test, nimbus-runtime is linked NON-cfg(test): provenance is
//! the production value, it matches the committed `.bin`/`.pc.bin`, and the anchor installs FROM the
//! embedded blob (a ~19ms deserialize instead of a ~4.18s build).
//!
//! Run feature-off (default lane) this installs a per-isolate read-only heap; run feature-on
//! (`--features v8-pointer-compression`, the `rust-runtime-ptrcomp-check` CI job) it installs the
//! embedded NodeFull superset into the real shared cage. Either way the committed blob must
//! deserialize into a working NodeFull isolate — if it were stale or V8-incompatible, this aborts
//! loud (provenance `Err`, or a V8 read-only-heap `V8_Fatal`).

#[test]
fn embedded_nodefull_anchor_installs_from_committed_blob() {
    nimbus_runtime::smoke_install_committed_embedded_anchor().expect(
        "the committed embedded NodeFull(Node22) anchor snapshot should deserialize and install a \
         working isolate on the serving path",
    );
}
