use super::*;

#[test]
fn refusal_leaves_package_json_byte_identical() {
    let temp = tempdir().expect("tempdir should build");
    // Deliberately non-canonical formatting: a refusal must not even
    // reformat the manifest, let alone rewire it.
    let manifest = "{\n    \"name\": \"client-app\",\n\n    \"dependencies\": {\"firebase\": \"^11.0.0\"}\n}\n";
    fs::write(temp.path().join("package.json"), manifest).unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/auth.ts"),
        "import { getAuth } from \"firebase/auth\";\n",
    )
    .unwrap();

    let error = wire_firestore_client_app(temp.path())
        .expect_err("uncovered firebase/auth import must refuse wiring");

    assert!(
        error.to_string().contains("refusing to wire"),
        "unexpected refusal error: {error}"
    );
    assert_eq!(
        fs::read(temp.path().join("package.json")).unwrap(),
        manifest.as_bytes(),
        "refusal must leave package.json byte-identical"
    );
    assert!(
        !temp.path().join(".nimbus").exists(),
        "refusal must not provision anything into the app"
    );
}

#[test]
fn covered_app_is_rewired_to_the_provisioned_drop_in() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join("package.json"),
        "{\n  \"name\": \"client-app\",\n  \"dependencies\": {\n    \"firebase\": \"^11.0.0\"\n  }\n}\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/db.ts"),
        "import { initializeApp } from \"firebase/app\";\n\
         import { getFirestore, collection, addDoc } from \"firebase/firestore\";\n",
    )
    .unwrap();
    // A registry-installed copy and a recorded install fingerprint: wiring
    // must drop the stale copy and clear the fingerprint so the next
    // dependency install refreshes from the provisioned payload.
    fs::create_dir_all(temp.path().join("node_modules/firebase")).unwrap();
    fs::write(
        temp.path().join("node_modules/firebase/package.json"),
        r#"{"name": "firebase", "version": "11.0.0"}"#,
    )
    .unwrap();
    let state_path = temp.path().join(".nimbus/cache/node/dependency-state.json");
    fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    fs::write(&state_path, "{}").unwrap();

    wire_firestore_client_app(temp.path()).expect("covered app must wire");

    let rewritten = fs::read_to_string(temp.path().join("package.json")).unwrap();
    assert!(
        rewritten.contains("\"firebase\": \"file:./.nimbus/packages/firebase\""),
        "package.json must point firebase at the provisioned copy: {rewritten}"
    );
    assert!(
        temp.path()
            .join(".nimbus/packages/firebase/package.json")
            .is_file(),
        "the drop-in package payload must be provisioned"
    );
    assert!(
        !temp.path().join("node_modules/firebase").exists(),
        "the stale registry-installed copy must be dropped"
    );
    assert!(
        !state_path.exists(),
        "the install fingerprint must be cleared so the next install reinstalls"
    );
}

#[test]
fn refusal_report_names_every_blocking_finding_with_file_line() {
    let temp = tempdir().expect("tempdir should build");
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/auth.ts"),
        "import { getAuth } from \"firebase/auth\";\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("src/lazy.ts"),
        "const flavor = pick();\nconst mod = await import(`firebase/${flavor}`);\n",
    )
    .unwrap();

    let covered = CoveredSet::from_embedded_manifest().expect("embedded manifest");
    let scan = firebase_scan::scan_app(temp.path(), &covered).expect("scan should run");
    let lines = firestore_wiring_refusal_lines(&scan, &covered);
    let report = lines.join("\n");

    assert!(
        report.contains("src/auth.ts:1") && report.contains("firebase/auth"),
        "report must name the uncovered import with file:line: {report}"
    );
    assert!(
        report.contains("src/lazy.ts:2"),
        "report must name the dynamic import with file:line: {report}"
    );
    assert!(
        report.contains("firebase/app") && report.contains("firebase/firestore"),
        "report must list the covered set: {report}"
    );
    assert!(
        report.contains("https://nimbusdocs.com/reference/firebase/compatibility/"),
        "report must point at the compatibility reference: {report}"
    );
}
