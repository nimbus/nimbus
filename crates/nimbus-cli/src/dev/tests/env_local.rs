use super::*;

#[test]
fn env_local_created_when_absent() {
    let temp = tempdir().expect("tempdir should build");
    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(content, "NIMBUS_DEPLOYMENT=local:myapp-abcd1234\n");
}

#[test]
fn env_local_appends_when_no_deployment_var() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(temp.path().join(".env.local"), "OTHER_VAR=hello\n").unwrap();
    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content,
        "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n"
    );
}

#[test]
fn env_local_noop_when_correct_value() {
    let temp = tempdir().expect("tempdir should build");
    let original = "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n";
    fs::write(temp.path().join(".env.local"), original).unwrap();
    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content, original,
        "file must not be rewritten when already correct"
    );
}

#[test]
fn env_local_overwrites_different_deployment_value() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join(".env.local"),
        "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:old-slug-12345678\nANOTHER=world\n",
    )
    .unwrap();
    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content,
        "OTHER_VAR=hello\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\nANOTHER=world\n"
    );
}

#[test]
fn env_local_deduplicates_deployment_entries() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join(".env.local"),
        "FIRST=1\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\nSECOND=2\nNIMBUS_DEPLOYMENT=local:old-slug-12345678\nTHIRD=3\n",
    )
    .unwrap();

    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();

    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content,
        "FIRST=1\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\nSECOND=2\nTHIRD=3\n"
    );
}

#[test]
fn env_local_preserves_other_content() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join(".env.local"),
        "FIRST=1\nSECOND=2\nTHIRD=3\n",
    )
    .unwrap();
    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content,
        "FIRST=1\nSECOND=2\nTHIRD=3\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n"
    );
}

#[test]
fn env_local_handles_file_without_trailing_newline() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(temp.path().join(".env.local"), "OTHER=val").unwrap();
    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();
    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content,
        "OTHER=val\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\n"
    );
}

#[test]
fn env_local_preserves_crlf_when_rewriting() {
    let temp = tempdir().expect("tempdir should build");
    fs::write(
        temp.path().join(".env.local"),
        "FIRST=1\r\nNIMBUS_DEPLOYMENT=local:old-slug-12345678\r\nSECOND=2\r\n",
    )
    .unwrap();

    write_env_local_deployment(temp.path(), "myapp-abcd1234").unwrap();

    let content = fs::read_to_string(temp.path().join(".env.local")).unwrap();
    assert_eq!(
        content,
        "FIRST=1\r\nNIMBUS_DEPLOYMENT=local:myapp-abcd1234\r\nSECOND=2\r\n"
    );
}
