//! Exercises the EMBEDDED NodeFull(Node22) anchor snapshot install — the serving path the cfg(test)
//! cage oracle cannot reach.
//!
//! Under cfg(test), `snapshot_extensions` adds a test-only extension, so the bootstrap snapshot's
//! provenance no longer matches the generated production blob and `try_embedded` correctly falls back
//! to a runtime build. As an INTEGRATION test, nimbus-runtime is linked NON-cfg(test): provenance is
//! the production value, it matches the generated `.bin`/`.pc.bin`, and the anchor installs FROM the
//! embedded blob (a ~19ms deserialize instead of a ~4.18s build).
//!
//! Run feature-off (default lane) this installs a per-isolate read-only heap; run feature-on
//! (`--features v8-pointer-compression`, the `rust-runtime-ptrcomp-check` CI job) it installs the
//! embedded NodeFull superset into the real shared cage. Either way the generated blob must
//! deserialize into a working NodeFull isolate — if it were stale or V8-incompatible, this aborts
//! loud (provenance `Err`, or a V8 read-only-heap `V8_Fatal`). The same process then builds and
//! restores the service-bearing NodeFull snapshot. Running this executable under a filesystem
//! sandbox that denies Deno source checkouts proves both service-snapshot source consumers use the
//! packaged table.

#[tokio::test(flavor = "current_thread")]
async fn embedded_nodefull_anchor_installs_from_generated_blob() {
    nimbus_runtime::smoke_install_generated_embedded_anchor().expect(
        "the generated embedded NodeFull(Node22) anchor snapshot should deserialize and install a \
         working isolate on the serving path",
    );
    nimbus_runtime::smoke_build_packaged_node_service_snapshot().expect(
        "the packaged source table should build and restore the service-bearing NodeFull snapshot",
    );
}
