#[test]
fn node_compat_manifest_report_schema_is_versioned_and_serializes_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let report = build_plan_report(plan);
    let json: Value = serde_json::to_value(&report).expect("plan report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["family"], "networking");
    assert!(
        json.as_object()
            .expect("plan report should serialize as an object")
            .keys()
            .all(|key| !key.contains("plan_item")),
        "plan report should not expose plan-era item fields",
    );
    assert_eq!(json["slice"], "dns-net-foundation");
    assert_eq!(json["execution_class"], "sequential");
    assert_eq!(json["presets"], serde_json::json!(["Application"]));
    assert_eq!(
        json["capabilities"],
        serde_json::json!(["bundle-root-fs", "loopback-net"])
    );
    assert_eq!(json["slice_summary"]["unique_fixture_count"], 10);
    assert_eq!(json["slice_summary"]["lane_count"], 3);
    assert_eq!(json["slice_summary"]["total_lane_fixture_entries"], 29);
    assert_eq!(json["slice_summary"]["min_lane_fixture_count"], 9);
    assert_eq!(json["slice_summary"]["max_lane_fixture_count"], 10);
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([{
            "preset": "Application",
            "unique_fixture_count": 10,
            "lane_count": 3,
            "total_lane_fixture_entries": 29
        }])
    );

    let lane_summaries = json["lane_summaries"]
        .as_array()
        .expect("lane summaries should serialize as an array");
    assert_eq!(lane_summaries.len(), 3);
    assert_eq!(lane_summaries[0]["lane"], "node20");
    assert_eq!(lane_summaries[0]["upstream_fixture_line"], "Node20");
    assert_eq!(lane_summaries[0]["lane_role"], "supported");
    assert_eq!(
        lane_summaries[0]["public_contract_role"],
        "supported_contract"
    );
    assert_eq!(lane_summaries[1]["lane"], "node22");
    assert_eq!(lane_summaries[1]["lane_role"], "default");
    assert_eq!(lane_summaries[2]["lane"], "node24");
    assert_eq!(
        lane_summaries[2]["public_contract_role"],
        "supported_contract"
    );
}

#[test]
fn node_compat_manifest_report_summarizes_lane_slice_and_preset_counts_from_execution_plan() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let report = build_plan_report(plan);

    assert_eq!(report.slice_summary.unique_fixture_count, 10);
    assert_eq!(report.slice_summary.lane_count, 3);
    assert_eq!(report.slice_summary.total_lane_fixture_entries, 29);
    assert_eq!(report.slice_summary.min_lane_fixture_count, 9);
    assert_eq!(report.slice_summary.max_lane_fixture_count, 10);
    assert_eq!(report.preset_summaries.len(), 1);
    assert_eq!(report.preset_summaries[0].preset, "Application");
    assert_eq!(report.preset_summaries[0].unique_fixture_count, 10);
    assert_eq!(report.preset_summaries[0].lane_count, 3);
    assert_eq!(report.preset_summaries[0].total_lane_fixture_entries, 29);
    assert_eq!(report.lane_summaries.len(), 3);
    assert_eq!(report.lane_summaries[0].upstream_fixture_line, "Node20");
    assert_eq!(report.lane_summaries[0].lane_role, "supported");
    assert_eq!(
        report.lane_summaries[1].public_contract_role,
        "default_contract"
    );
    assert_eq!(report.lane_summaries[0].fixture_count, 10);
    assert_eq!(report.lane_summaries[1].fixture_count, 10);
    assert_eq!(report.lane_summaries[2].fixture_count, 9);
    assert_eq!(
        report.lane_summaries[0].fixture_ids[0],
        "test/parallel/test-dns-get-server.js"
    );
    assert_eq!(
        report.lane_summaries[1].fixture_ids[6],
        "test/parallel/test-stream-pipeline.js"
    );
    assert!(
        !report.lane_summaries[2]
            .fixture_ids
            .contains(&"test/parallel/test-stream-pipeline.js"),
        "report should preserve explicit lane omissions instead of flattening them",
    );
}

#[test]
fn node_compat_manifest_report_builds_catalog_summary_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let report = build_catalog_plan_report(&resolved).expect("catalog plan report should resolve");
    let json: Value = serde_json::to_value(&report).expect("catalog plan report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["family_count"], 11);
    assert_eq!(json["slice_count"], 12);
    assert_eq!(json["total_unique_fixture_seed_count"], 57);
    assert_eq!(json["total_lane_fixture_entries"], 167);
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_unique_fixture_seed_count": 57,
                "total_lane_fixture_entries": 167,
                "slice_count_with_entries": 12
            }
        ])
    );
    assert_eq!(
        json["lane_summaries"],
        serde_json::json!([
            {
                "lane": "node20",
                "upstream_fixture_line": "Node20",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node20",
                "runtime_limits_preset": "application_node20",
                "total_fixture_entries": 54,
                "slice_count_with_entries": 12
            },
            {
                "lane": "node22",
                "upstream_fixture_line": "Node22",
                "lane_role": "default",
                "public_contract_role": "default_contract",
                "runtime_execution_target": "Node22",
                "runtime_limits_preset": "application_node22",
                "total_fixture_entries": 57,
                "slice_count_with_entries": 12
            },
            {
                "lane": "node24",
                "upstream_fixture_line": "Node24",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node24",
                "runtime_limits_preset": "application_node24",
                "total_fixture_entries": 56,
                "slice_count_with_entries": 12
            }
        ])
    );
    let slice_reports = json["slice_reports"]
        .as_array()
        .expect("catalog slice reports should serialize as an array");
    assert_eq!(slice_reports.len(), 12);
    assert_eq!(slice_reports[0]["family"], "core-semantics");
    assert_eq!(slice_reports[11]["family"], "streams-and-local-io");
}

#[test]
fn node_compat_manifest_report_aggregates_observed_results_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let observed_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-set-default-order.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
    ];
    let report = build_observed_plan_report(&plan, &observed_results)
        .expect("observed plan report should build");
    let json: Value = serde_json::to_value(&report).expect("observed plan report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["slice_summary"]["unique_fixture_count"], 10);
    assert_eq!(json["slice_summary"]["lane_count"], 3);
    assert_eq!(json["slice_summary"]["total_expected_results"], 29);
    assert_eq!(json["slice_summary"]["total_observed_results"], 4);
    assert_eq!(
        json["slice_summary"]["counts"],
        serde_json::json!({
            "passed": 2,
            "skipped": 1,
            "failed": 1,
            "missing": 25
        })
    );
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([{
            "preset": "Application",
            "total_expected_results": 29,
            "total_observed_results": 4,
            "counts": {
                "passed": 2,
                "skipped": 1,
                "failed": 1,
                "missing": 25
            }
        }])
    );
    assert_eq!(
        json["lane_summaries"],
        serde_json::json!([
            {
                "lane": "node20",
                "upstream_fixture_line": "Node20",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node20",
                "runtime_limits_preset": "application_node20",
                "subset_test": "runtime::tests::node_compat::node20_supported_lane_executes_official_networking_subset",
                "expected_fixture_count": 10,
                "observed_fixture_count": 2,
                "counts": {
                    "passed": 1,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 8
                }
            },
            {
                "lane": "node22",
                "upstream_fixture_line": "Node22",
                "lane_role": "default",
                "public_contract_role": "default_contract",
                "runtime_execution_target": "Node22",
                "runtime_limits_preset": "application_node22",
                "subset_test": "runtime::tests::node_compat::node22_default_lane_executes_manifested_networking_subset",
                "expected_fixture_count": 10,
                "observed_fixture_count": 1,
                "counts": {
                    "passed": 0,
                    "skipped": 1,
                    "failed": 0,
                    "missing": 9
                }
            },
            {
                "lane": "node24",
                "upstream_fixture_line": "Node24",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node24",
                "runtime_limits_preset": "application_node24",
                "subset_test": "runtime::tests::node_compat::node24_supported_lane_networking_watchpoint",
                "expected_fixture_count": 9,
                "observed_fixture_count": 1,
                "counts": {
                    "passed": 1,
                    "skipped": 0,
                    "failed": 0,
                    "missing": 8
                }
            }
        ])
    );
}

#[test]
fn node_compat_manifest_report_rejects_unknown_or_duplicate_observed_results() {
    let resolved = load_family_catalogs_from_disk();
    let plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");

    let duplicate_error = build_observed_plan_report(
        &plan,
        &[
            NodeCompatObservedLaneFixtureResult {
                lane: "node20",
                fixture_id: "test/parallel/test-dns-get-server.js",
                state: NodeCompatObservedFixtureState::Pass,
            },
            NodeCompatObservedLaneFixtureResult {
                lane: "node20",
                fixture_id: "test/parallel/test-dns-get-server.js",
                state: NodeCompatObservedFixtureState::Fail,
            },
        ],
    )
    .expect_err("duplicate observed results should fail");
    assert!(
        duplicate_error.contains(
            "duplicate observed result for lane fixture node20:test/parallel/test-dns-get-server.js"
        ),
        "duplicate observed result error should mention the duplicate lane fixture: {duplicate_error}",
    );

    let unknown_error = build_observed_plan_report(
        &plan,
        &[NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-stream-pipeline.js",
            state: NodeCompatObservedFixtureState::Pass,
        }],
    )
    .expect_err("unknown lane fixture result should fail");
    assert!(
        unknown_error.contains(
            "observed result references unknown lane fixture node24:test/parallel/test-stream-pipeline.js"
        ),
        "unknown observed result error should mention the offending lane fixture: {unknown_error}",
    );
}

#[test]
fn node_compat_manifest_report_aggregates_observed_catalog_results_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let networking_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-set-default-order.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
    ];
    let process_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-process-default.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
    ];
    let report = build_observed_catalog_report(
        &resolved,
        &[
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &networking_results,
            },
            NodeCompatObservedSliceInput {
                family: "process-and-timing",
                slice: "process-foundation",
                observed_results: &process_results,
            },
        ],
    )
    .expect("observed catalog report should build");
    let json: Value =
        serde_json::to_value(&report).expect("observed catalog report should serialize");

    assert_eq!(
        json["schema_version"],
        NODE_COMPAT_PLAN_REPORT_SCHEMA_VERSION
    );
    assert_eq!(json["family_count"], 2);
    assert_eq!(json["slice_count"], 2);
    assert_eq!(json["total_unique_fixture_seed_count"], 20);
    assert_eq!(json["total_expected_results"], 58);
    assert_eq!(json["total_observed_results"], 7);
    assert_eq!(
        json["counts"],
        serde_json::json!({
            "passed": 4,
            "skipped": 1,
            "failed": 2,
            "missing": 51
        })
    );
    assert_eq!(
        json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_expected_results": 58,
                "total_observed_results": 7,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 4,
                    "skipped": 1,
                    "failed": 2,
                    "missing": 51
                }
            }
        ])
    );
    assert_eq!(
        json["lane_summaries"],
        serde_json::json!([
            {
                "lane": "node20",
                "upstream_fixture_line": "Node20",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node20",
                "runtime_limits_preset": "application_node20",
                "total_expected_results": 19,
                "total_observed_results": 3,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 2,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 16
                }
            },
            {
                "lane": "node22",
                "upstream_fixture_line": "Node22",
                "lane_role": "default",
                "public_contract_role": "default_contract",
                "runtime_execution_target": "Node22",
                "runtime_limits_preset": "application_node22",
                "total_expected_results": 20,
                "total_observed_results": 2,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 1,
                    "skipped": 1,
                    "failed": 0,
                    "missing": 18
                }
            },
            {
                "lane": "node24",
                "upstream_fixture_line": "Node24",
                "lane_role": "supported",
                "public_contract_role": "supported_contract",
                "runtime_execution_target": "Node24",
                "runtime_limits_preset": "application_node24",
                "total_expected_results": 19,
                "total_observed_results": 2,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 1,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 17
                }
            }
        ])
    );
    let slice_reports = json["slice_reports"]
        .as_array()
        .expect("observed catalog slice reports should serialize as an array");
    assert_eq!(slice_reports.len(), 2);
    assert_eq!(slice_reports[0]["family"], "networking");
    assert_eq!(slice_reports[1]["family"], "process-and-timing");
}

#[test]
fn node_compat_manifest_report_supports_multi_preset_catalog_summaries() {
    let mut resolved = load_family_catalogs_from_disk();
    let process_family = resolved
        .family_catalogs
        .iter_mut()
        .find(|catalog| catalog.family == "process-and-timing")
        .expect("process-and-timing family catalog should exist");
    process_family.presets.push(NodeCompatPreset::Tooling);

    let plan_report =
        build_catalog_plan_report(&resolved).expect("catalog plan report should resolve");
    let plan_json: Value =
        serde_json::to_value(&plan_report).expect("catalog plan report should serialize");
    assert_eq!(
        plan_json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_unique_fixture_seed_count": 57,
                "total_lane_fixture_entries": 167,
                "slice_count_with_entries": 12
            },
            {
                "preset": "Tooling",
                "total_unique_fixture_seed_count": 10,
                "total_lane_fixture_entries": 29,
                "slice_count_with_entries": 1
            }
        ])
    );

    let networking_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-set-default-order.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
    ];
    let process_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-process-default.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node24",
            fixture_id: "test/parallel/test-process-features.js",
            state: NodeCompatObservedFixtureState::Fail,
        },
    ];
    let observed_report = build_observed_catalog_report(
        &resolved,
        &[
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &networking_results,
            },
            NodeCompatObservedSliceInput {
                family: "process-and-timing",
                slice: "process-foundation",
                observed_results: &process_results,
            },
        ],
    )
    .expect("observed catalog report should build");
    let observed_json: Value =
        serde_json::to_value(&observed_report).expect("observed catalog report should serialize");
    assert_eq!(
        observed_json["preset_summaries"],
        serde_json::json!([
            {
                "preset": "Application",
                "total_expected_results": 58,
                "total_observed_results": 7,
                "slice_count_with_entries": 2,
                "counts": {
                    "passed": 4,
                    "skipped": 1,
                    "failed": 2,
                    "missing": 51
                }
            },
            {
                "preset": "Tooling",
                "total_expected_results": 29,
                "total_observed_results": 3,
                "slice_count_with_entries": 1,
                "counts": {
                    "passed": 2,
                    "skipped": 0,
                    "failed": 1,
                    "missing": 26
                }
            }
        ])
    );
}

#[test]
fn node_compat_manifest_report_writes_artifacts_deterministically() {
    let resolved = load_family_catalogs_from_disk();
    let slice_plan = resolved
        .resolve_lane_execution_plan("networking", "dns-net-foundation")
        .expect("networking execution plan should resolve");
    let slice_report = build_plan_report(slice_plan);
    let catalog_plan_report =
        build_catalog_plan_report(&resolved).expect("catalog plan report should resolve");
    let networking_results = [
        NodeCompatObservedLaneFixtureResult {
            lane: "node20",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Pass,
        },
        NodeCompatObservedLaneFixtureResult {
            lane: "node22",
            fixture_id: "test/parallel/test-dns-get-server.js",
            state: NodeCompatObservedFixtureState::Skip,
        },
    ];
    let observed_catalog_report = build_observed_catalog_report(
        &resolved,
        &[NodeCompatObservedSliceInput {
            family: "networking",
            slice: "dns-net-foundation",
            observed_results: &networking_results,
        }],
    )
    .expect("observed catalog report should resolve");
    let output_root = unique_report_artifact_test_root();

    let slice_path = write_plan_report_artifact(&output_root, &slice_report)
        .expect("slice plan artifact should write");
    let catalog_plan_path = write_catalog_plan_report_artifact(&output_root, &catalog_plan_report)
        .expect("catalog plan artifact should write");
    let observed_catalog_path =
        write_observed_catalog_report_artifact(&output_root, &observed_catalog_report)
            .expect("observed catalog artifact should write");

    assert_eq!(
        slice_path.file_name().and_then(|name| name.to_str()),
        Some("slice-plan-networking-dns-net-foundation.json")
    );
    assert_eq!(
        catalog_plan_path.file_name().and_then(|name| name.to_str()),
        Some("catalog-plan.json")
    );
    assert_eq!(
        observed_catalog_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("catalog-observed.json")
    );

    let slice_json: Value = serde_json::from_slice(
        &std::fs::read(&slice_path).expect("slice artifact should be readable"),
    )
    .expect("slice artifact should parse as json");
    let catalog_plan_json: Value = serde_json::from_slice(
        &std::fs::read(&catalog_plan_path).expect("catalog plan artifact should be readable"),
    )
    .expect("catalog plan artifact should parse as json");
    let observed_catalog_json: Value = serde_json::from_slice(
        &std::fs::read(&observed_catalog_path)
            .expect("observed catalog artifact should be readable"),
    )
    .expect("observed catalog artifact should parse as json");

    assert_eq!(slice_json["family"], "networking");
    assert_eq!(slice_json["slice"], "dns-net-foundation");
    assert_eq!(catalog_plan_json["family_count"], 11);
    assert_eq!(
        catalog_plan_json["preset_summaries"][0]["preset"],
        "Application"
    );
    assert_eq!(observed_catalog_json["slice_count"], 1);
    assert_eq!(
        observed_catalog_json["counts"],
        serde_json::json!({
            "passed": 1,
            "skipped": 1,
            "failed": 0,
            "missing": 27
        })
    );

    std::fs::remove_dir_all(&output_root).expect("temporary report artifact root should clean up");
}

#[test]
fn node_compat_manifest_report_emits_seeded_slice_artifact_bundle_under_stable_root() {
    let output_root = unique_report_artifact_test_root();
    let artifacts =
        emit_seeded_slice_report_artifacts(&output_root, "networking", "dns-net-foundation")
            .expect("seeded slice artifact bundle should emit");

    assert_eq!(
        artifacts.artifact_root,
        output_root.join("networking").join("dns-net-foundation")
    );
    assert_eq!(
        artifacts
            .slice_plan_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("slice-plan-networking-dns-net-foundation.json")
    );
    assert_eq!(
        artifacts
            .slice_observed_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("slice-observed-networking-dns-net-foundation.json")
    );
    assert_eq!(
        artifacts
            .catalog_plan_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("catalog-plan.json")
    );
    assert_eq!(
        artifacts
            .catalog_observed_path
            .file_name()
            .and_then(|name| name.to_str()),
        Some("catalog-observed.json")
    );

    let slice_plan_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.slice_plan_path).expect("slice plan artifact should be readable"),
    )
    .expect("slice plan artifact should parse as json");
    let slice_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.slice_observed_path)
            .expect("slice observed artifact should be readable"),
    )
    .expect("slice observed artifact should parse as json");
    let catalog_plan_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.catalog_plan_path)
            .expect("catalog plan artifact should be readable"),
    )
    .expect("catalog plan artifact should parse as json");
    let catalog_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.catalog_observed_path)
            .expect("catalog observed artifact should be readable"),
    )
    .expect("catalog observed artifact should parse as json");

    assert_eq!(slice_plan_json["family"], "networking");
    assert_eq!(slice_observed_json["slice"], "dns-net-foundation");
    assert_eq!(
        slice_observed_json["slice_summary"]["counts"]["missing"],
        29
    );
    assert_eq!(catalog_plan_json["family_count"], 11);
    assert_eq!(catalog_observed_json["slice_count"], 1);
    assert_eq!(catalog_observed_json["counts"]["missing"], 29);

    std::fs::remove_dir_all(&output_root).expect("temporary seeded artifact root should clean up");
}

#[test]
#[ignore = "manual harness report artifact entrypoint"]
fn node_compat_manifest_report_entrypoint_emits_slice_artifacts() {
    let family = std::env::var("NIMBUS_NODE_COMPAT_REPORT_FAMILY")
        .expect("NIMBUS_NODE_COMPAT_REPORT_FAMILY should be set");
    let slice = std::env::var("NIMBUS_NODE_COMPAT_REPORT_SLICE")
        .expect("NIMBUS_NODE_COMPAT_REPORT_SLICE should be set");
    let output_root = std::env::var("NIMBUS_NODE_COMPAT_REPORT_OUTPUT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_report_artifact_output_root());
    let capture_mode = std::env::var("NIMBUS_NODE_COMPAT_REPORT_CAPTURE_MODE")
        .unwrap_or_else(|_| "seeded".to_string());
    let observed_results_path = std::env::var("NIMBUS_NODE_COMPAT_REPORT_OBSERVED_RESULTS")
        .ok()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let artifacts = match capture_mode.as_str() {
        "seeded" => {
            let observed_result_records = observed_results_path
                .as_ref()
                .map(|path| read_observed_result_records(path))
                .transpose()
                .expect("observed results path should parse when present")
                .unwrap_or_default();
            let observed_results = borrow_observed_result_records(&observed_result_records);
            emit_slice_report_artifacts_with_observed_results(
                &output_root,
                &family,
                &slice,
                &observed_results,
            )
            .expect("seeded slice artifacts should emit from manual entrypoint")
        }
        "live" => {
            assert!(
                observed_results_path.is_none(),
                "live capture mode cannot also consume NIMBUS_NODE_COMPAT_REPORT_OBSERVED_RESULTS",
            );
            emit_live_seeded_slice_report_artifacts(&output_root, &family, &slice)
                .expect("live slice artifacts should emit from manual entrypoint")
        }
        other => panic!("unsupported node-compat report capture mode `{other}`"),
    };

    println!("artifact_root={}", artifacts.artifact_root.display());
    println!("slice_plan={}", artifacts.slice_plan_path.display());
    println!("slice_observed={}", artifacts.slice_observed_path.display());
    println!("catalog_plan={}", artifacts.catalog_plan_path.display());
    println!(
        "catalog_observed={}",
        artifacts.catalog_observed_path.display()
    );
}

#[test]
fn node_compat_manifest_report_emits_observed_results_from_json_input() {
    let output_root = unique_report_artifact_test_root();
    let observed_results_path = output_root.join("observed-results.json");
    std::fs::create_dir_all(&output_root).expect("temporary observed-results root should create");
    std::fs::write(
        &observed_results_path,
        serde_json::to_vec_pretty(&serde_json::json!([
            {
                "lane": "node20",
                "fixture_id": "test/parallel/test-dns-get-server.js",
                "state": "pass"
            },
            {
                "lane": "node22",
                "fixture_id": "test/parallel/test-dns-get-server.js",
                "state": "skip"
            }
        ]))
        .expect("observed results fixture should serialize"),
    )
    .expect("observed results fixture should write");
    let observed_result_records = read_observed_result_records(&observed_results_path)
        .expect("observed results file should parse");
    let observed_results = borrow_observed_result_records(&observed_result_records);
    let artifacts = emit_slice_report_artifacts_with_observed_results(
        &output_root,
        "networking",
        "dns-net-foundation",
        &observed_results,
    )
    .expect("observed artifact bundle should emit");

    let slice_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.slice_observed_path)
            .expect("slice observed artifact should be readable"),
    )
    .expect("slice observed artifact should parse as json");
    let catalog_observed_json: Value = serde_json::from_slice(
        &std::fs::read(&artifacts.catalog_observed_path)
            .expect("catalog observed artifact should be readable"),
    )
    .expect("catalog observed artifact should parse as json");

    assert_eq!(
        slice_observed_json["slice_summary"]["total_observed_results"],
        2
    );
    assert_eq!(
        slice_observed_json["slice_summary"]["counts"],
        serde_json::json!({
            "passed": 1,
            "skipped": 1,
            "failed": 0,
            "missing": 27
        })
    );
    assert_eq!(catalog_observed_json["slice_count"], 1);
    assert_eq!(catalog_observed_json["total_observed_results"], 2);

    std::fs::remove_dir_all(&output_root)
        .expect("temporary observed-results artifact root should clean up");
}

#[test]
fn node_compat_manifest_report_rejects_duplicate_observed_catalog_slice_inputs() {
    let resolved = load_family_catalogs_from_disk();
    let error = build_observed_catalog_report(
        &resolved,
        &[
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &[],
            },
            NodeCompatObservedSliceInput {
                family: "networking",
                slice: "dns-net-foundation",
                observed_results: &[],
            },
        ],
    )
    .expect_err("duplicate observed slice inputs should fail");
    assert!(
        error.contains("duplicate observed slice input networking:dns-net-foundation"),
        "duplicate observed slice input error should mention the duplicate key: {error}",
    );
}
