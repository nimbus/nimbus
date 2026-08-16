use std::fs;
use std::path::Path;

use nimbus_network::NetworkResourcePhase;

#[cfg(target_os = "linux")]
use super::super::super::netns::create_persistent_network_namespace;
use super::super::super::netns::{
    ExactRegularArtifactObservation, inspect_exact_regular_artifact,
    inspect_exact_regular_artifact_with_parent_open_hook,
    inspect_exact_regular_artifact_with_target_inspected_hook,
    read_exact_regular_artifact_with_target_inspected_hook, remove_persistent_network_namespace,
    remove_persistent_network_namespace_with_target_inspected_hook,
};
use super::require_retained_detach_phase;

#[test]
fn nnc6_5d3_k10_retained_detach_requires_deleting_before_host_effects() {
    assert!(
        require_retained_detach_phase(NetworkResourcePhase::Deleting, false).is_ok(),
        "only exact portable Deleting authority can enter retained host teardown"
    );
    for phase in [
        NetworkResourcePhase::Reserved,
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Active,
        NetworkResourcePhase::CleanupPending,
        NetworkResourcePhase::Released,
    ] {
        assert!(
            require_retained_detach_phase(phase, phase == NetworkResourcePhase::Released).is_err(),
            "retained detach must reject {phase:?} before a host effect"
        );
    }
}

#[test]
fn nnc6_5d3_k14_missing_target_requires_readable_exact_parent() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let readable_parent = root.path().join("readable");
    fs::create_dir(&readable_parent).expect("readable parent should create");
    assert_eq!(
        inspect_exact_regular_artifact(
            &readable_parent,
            &readable_parent.join("missing"),
            "namespace",
        ),
        Ok(ExactRegularArtifactObservation::ExplicitlyAbsent)
    );

    let missing_parent = root.path().join("missing-parent");
    assert!(
        inspect_exact_regular_artifact(
            &missing_parent,
            &missing_parent.join("namespace"),
            "namespace",
        )
        .expect_err("missing parent must preserve ambiguity")
        .contains("cannot inspect parent")
    );

    let file_parent = root.path().join("file-parent");
    fs::write(&file_parent, b"not a directory").expect("file parent should write");
    assert!(
        inspect_exact_regular_artifact(&file_parent, &file_parent.join("namespace"), "namespace",)
            .expect_err("wrong parent type must preserve ambiguity")
            .contains("not a non-symlink directory")
    );
}

#[test]
fn nnc6_5d3_k14_wrong_target_types_are_ambiguous() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let regular = root.path().join("regular");
    fs::write(&regular, b"provider evidence").expect("regular artifact should write");
    assert_eq!(
        inspect_exact_regular_artifact(root.path(), &regular, "namespace"),
        Ok(ExactRegularArtifactObservation::Present)
    );

    let directory = root.path().join("directory");
    fs::create_dir(&directory).expect("wrong-type artifact should create");
    assert!(
        inspect_exact_regular_artifact(root.path(), &directory, "namespace")
            .expect_err("directory artifact must preserve ambiguity")
            .contains("not an exact regular provider artifact")
    );
}

#[test]
fn nnc6_5d3_k14_namespace_inspection_is_byte_stable() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");
    fs::write(&artifact, b"exact namespace evidence").expect("artifact should write");
    let before = directory_snapshot(root.path());

    assert_eq!(
        inspect_exact_regular_artifact(root.path(), &artifact, "namespace"),
        Ok(ExactRegularArtifactObservation::Present)
    );
    assert_eq!(directory_snapshot(root.path()), before);
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_parent_replacement_during_inspection_is_ambiguous() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let expected_parent = root.path().join("netns");
    let archived_parent = root.path().join("netns-opened");
    let replacement_parent = root.path().join("netns-replacement");
    fs::create_dir(&expected_parent).expect("expected parent should create");
    fs::create_dir(&replacement_parent).expect("replacement parent should create");
    let artifact = expected_parent.join("namespace");
    fs::write(&artifact, b"retained provider namespace").expect("artifact should write");

    let error = inspect_exact_regular_artifact_with_parent_open_hook(
        &expected_parent,
        &artifact,
        "namespace",
        || {
            fs::rename(&expected_parent, &archived_parent)
                .expect("opened parent should move without changing its identity");
            fs::rename(&replacement_parent, &expected_parent)
                .expect("replacement parent should occupy the ambient path");
        },
    )
    .expect_err("a parent replacement during inspection must preserve ambiguity");

    assert!(error.contains("changed during inspection"), "{error}");
    assert_eq!(
        fs::read(archived_parent.join("namespace"))
            .expect("the original provider namespace must remain present"),
        b"retained provider namespace"
    );
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_final_creation_after_absence_is_ambiguous() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");

    let error = inspect_exact_regular_artifact_with_target_inspected_hook(
        root.path(),
        &artifact,
        "namespace",
        || fs::write(&artifact, b"late provider namespace").expect("late artifact should write"),
    )
    .expect_err("a final entry created after absence inspection must preserve ambiguity");

    assert!(error.contains("changed during inspection"), "{error}");
    assert_eq!(
        fs::read(&artifact).expect("the late provider artifact must remain present"),
        b"late provider namespace"
    );
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_final_replacement_during_inspection_is_ambiguous() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");
    let archived = root.path().join("namespace-opened");
    fs::write(&artifact, b"original provider namespace").expect("artifact should write");

    let error = inspect_exact_regular_artifact_with_target_inspected_hook(
        root.path(),
        &artifact,
        "namespace",
        || replace_artifact(&artifact, &archived),
    )
    .expect_err("a final entry replacement during inspection must preserve ambiguity");

    assert!(error.contains("changed during inspection"), "{error}");
    assert_replacement_artifacts_are_intact(&artifact, &archived);
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_final_replacement_during_read_is_ambiguous() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");
    let archived = root.path().join("namespace-opened");
    fs::write(&artifact, b"original provider namespace").expect("artifact should write");

    let error = read_exact_regular_artifact_with_target_inspected_hook(
        root.path(),
        &artifact,
        "namespace",
        || replace_artifact(&artifact, &archived),
    )
    .expect_err("a final entry replacement during read must preserve ambiguity");

    assert!(error.contains("changed during inspection"), "{error}");
    assert_replacement_artifacts_are_intact(&artifact, &archived);
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_final_replacement_before_remove_is_ambiguous_and_non_destructive() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");
    let archived = root.path().join("namespace-opened");
    fs::write(&artifact, b"original provider namespace").expect("artifact should write");

    let error = remove_persistent_network_namespace_with_target_inspected_hook(&artifact, || {
        replace_artifact(&artifact, &archived);
    })
    .expect_err("a final entry replacement before removal must preserve ambiguity");

    assert!(
        error.to_string().contains("changed during inspection"),
        "{error}"
    );
    assert_replacement_artifacts_are_intact(&artifact, &archived);
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_final_creation_before_absent_remove_is_ambiguous_and_non_destructive() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");

    let error = remove_persistent_network_namespace_with_target_inspected_hook(&artifact, || {
        fs::write(&artifact, b"late provider namespace").expect("late artifact should write");
    })
    .expect_err("a final entry created before absent removal must preserve ambiguity");

    assert!(
        error.to_string().contains("changed during inspection"),
        "{error}"
    );
    assert_eq!(
        fs::read(&artifact).expect("the late provider artifact must remain present"),
        b"late provider namespace"
    );
}

#[test]
fn nnc6_5d3_k14_exact_present_removal_deletes_only_the_pinned_artifact() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");
    fs::write(&artifact, b"exact provider namespace").expect("artifact should write");

    remove_persistent_network_namespace(&artifact)
        .expect("the exact regular namespace artifact should remove");

    assert!(
        !artifact.exists(),
        "successful namespace removal must leave the exact target absent"
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires Linux CAP_SYS_ADMIN to create and detach a persistent network namespace"]
fn nnc6_5d3_k14_mounted_namespace_removal_detaches_nsfs_and_deletes_mount_point() {
    let root = tempfile::tempdir().expect("artifact root should exist");
    let artifact = root.path().join("namespace");
    create_persistent_network_namespace(&artifact)
        .expect("the exact persistent namespace should mount");

    remove_persistent_network_namespace(&artifact)
        .expect("the exact mounted namespace and its mount point should remove");

    assert!(
        !artifact.exists(),
        "successful mounted namespace removal must leave the exact target absent"
    );
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_present_target_rejects_symlinked_parent_and_final_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("artifact root should exist");
    let real_parent = root.path().join("real-parent");
    fs::create_dir(&real_parent).expect("real parent should create");
    let foreign_target = real_parent.join("namespace");
    fs::write(&foreign_target, b"foreign namespace").expect("foreign target should write");
    let symlink_parent = root.path().join("symlink-parent");
    symlink(&real_parent, &symlink_parent).expect("parent symlink should create");
    assert!(
        inspect_exact_regular_artifact(
            &symlink_parent,
            &symlink_parent.join("namespace"),
            "namespace",
        )
        .expect_err("present target through symlinked parent must preserve ambiguity")
        .contains("not a non-symlink directory")
    );
    assert!(
        remove_persistent_network_namespace(&symlink_parent.join("namespace")).is_err(),
        "namespace removal must reject the crossed parent before an effect"
    );
    assert_eq!(
        fs::read(&foreign_target).expect("crossed target must remain present"),
        b"foreign namespace"
    );

    let final_symlink = root.path().join("final-symlink");
    symlink(real_parent.join("namespace"), &final_symlink)
        .expect("final artifact symlink should create");
    assert!(
        inspect_exact_regular_artifact(root.path(), &final_symlink, "namespace")
            .expect_err("final symlink must preserve ambiguity")
            .contains("not an exact regular provider artifact")
    );
}

#[cfg(unix)]
#[test]
fn nnc6_5d3_k14_unreadable_parent_is_ambiguous_for_unprivileged_owner() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("artifact root should exist");
    let unreadable_parent = root.path().join("unreadable-parent");
    fs::create_dir(&unreadable_parent).expect("unreadable parent should create");
    fs::set_permissions(&unreadable_parent, fs::Permissions::from_mode(0o000))
        .expect("parent permissions should change");
    let result = inspect_exact_regular_artifact(
        &unreadable_parent,
        &unreadable_parent.join("namespace"),
        "namespace",
    );
    fs::set_permissions(&unreadable_parent, fs::Permissions::from_mode(0o700))
        .expect("parent permissions should restore");
    // Root can bypass mode bits; all ordinary test users must preserve the
    // permission failure as ambiguous.
    if unsafe { libc::geteuid() } != 0 {
        assert!(result.is_err(), "unreadable parent must preserve ambiguity");
    }
}

fn directory_snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = fs::read_dir(root)
        .expect("snapshot root should read")
        .map(|entry| {
            let entry = entry.expect("snapshot entry should read");
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = if entry.file_type().expect("entry type should read").is_file() {
                fs::read(entry.path()).expect("snapshot file should read")
            } else {
                Vec::new()
            };
            (name, bytes)
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[cfg(unix)]
fn replace_artifact(artifact: &Path, archived: &Path) {
    fs::rename(artifact, archived).expect("opened artifact should move without changing identity");
    fs::write(artifact, b"replacement provider namespace")
        .expect("replacement artifact should write");
}

#[cfg(unix)]
fn assert_replacement_artifacts_are_intact(artifact: &Path, archived: &Path) {
    assert_eq!(
        fs::read(archived).expect("the opened artifact must remain present"),
        b"original provider namespace"
    );
    assert_eq!(
        fs::read(artifact).expect("the replacement artifact must remain present"),
        b"replacement provider namespace"
    );
}
