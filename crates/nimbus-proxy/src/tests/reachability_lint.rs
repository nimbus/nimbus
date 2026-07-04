/// EE1 reachability lint: the workload map is not referenceable from the
/// request path.
///
/// Today's tenant isolation is a type/ownership property — each accept task
/// closes over its own per-PEP context, so a request handler cannot name
/// another workload's state. The node-scoped `EgressEngine` keeps its
/// `Map<WorkloadId, WorkloadPep>` off the request path by module discipline:
/// within `nimbus-proxy`, only `engine.rs` (the definition) and `lib.rs` (the
/// export) may name `EgressEngine` or `WorkloadId`. Every other module — the
/// worker accept/handler path, the intercept path, the pingora adapter, and
/// all request-processing modules — must be unable to reach the map even by
/// name. A plain "hot path holds no `Map<SandboxId, …>`" grep would be vacuous
/// (`nimbus-proxy` has no `SandboxId` at all); scanning for the engine's own
/// key/type names is the non-vacuous form.
///
/// This is the compensating control the egress-engine plan's isolation
/// argument rests on; the plan verifier (`verify-nimbus-egress-engine.sh`)
/// enforces the same rule from outside the crate.
#[test]
fn ee1_reachability_lint_workload_map_unreachable_from_request_path() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // engine.rs defines the engine; lib.rs exports it; src/tests is test-only
    // code, never part of the request path.
    let allowed = ["engine.rs", "lib.rs"];
    let tests_dir = src_dir.join("tests");
    let needles = ["EgressEngine", "WorkloadId"];

    let mut violations = Vec::new();
    let mut scanned = 0usize;
    // Recursive walk: a future src/ subdirectory (e.g. a request/ split) must
    // not silently escape the scan while the scanned-count floor stays green.
    let mut pending = vec![src_dir.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("nimbus-proxy src dir must be readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                if path == tests_dir {
                    continue;
                }
                pending.push(path);
                continue;
            }
            if path.starts_with(&tests_dir) {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let top_level_allowed = dir == src_dir && allowed.contains(&name);
            if !name.ends_with(".rs") || top_level_allowed {
                continue;
            }
            scanned += 1;
            let contents = std::fs::read_to_string(&path).expect("source file must be readable");
            let display = path
                .strip_prefix(&src_dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            for needle in needles {
                if contents.contains(needle) {
                    violations.push(format!("{display} references {needle}"));
                }
            }
        }
    }

    // Guard the lint itself against vacuousness: 22 production modules are
    // scanned today after excluding engine.rs/lib.rs and src/tests, with a
    // two-file margin for legitimate module consolidation.
    assert!(
        scanned >= 20,
        "reachability lint scanned only {scanned} files; scan set is broken"
    );
    let engine_src =
        std::fs::read_to_string(src_dir.join("engine.rs")).expect("engine.rs must exist (EE1c)");
    for needle in needles {
        assert!(
            engine_src.contains(needle),
            "lint needle {needle} no longer exists in engine.rs; update the lint"
        );
    }

    assert!(
        violations.is_empty(),
        "EE1 reachability violation — the workload map (or its key type) is nameable from \
         request-path modules: {violations:?}"
    );
}
