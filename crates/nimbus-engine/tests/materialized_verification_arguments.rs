#[path = "../benches/materialized_verification/arguments.rs"]
mod arguments;

use std::path::PathBuf;

use arguments::parse_arguments_from;
use nimbus_core::Error;

fn argument_strings<'a>(values: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    values.iter().map(|value| (*value).to_string())
}

#[test]
fn candidate_only_rejects_explicit_full_sample_count() {
    let error = parse_arguments_from(argument_strings(&[
        "--candidate-only",
        "--full-samples",
        "7",
    ]))
    .expect_err("candidate-only mode must reject an ignored full sample count");
    assert!(matches!(
        error,
        Error::InvalidInput(message)
            if message == "--candidate-only accepts only --output"
    ));
}

#[test]
fn candidate_only_accepts_only_its_optional_output() {
    let parsed = parse_arguments_from(argument_strings(&[
        "--candidate-only",
        "--output",
        "candidate.json",
    ]))
    .expect("candidate-only output should parse");
    assert!(parsed.candidate_only);
    assert_eq!(parsed.output, Some(PathBuf::from("candidate.json")));
    assert!(!parsed.quick);
    assert_eq!(parsed.documents, None);
    assert_eq!(parsed.payload_bytes, None);
    assert_eq!(parsed.full_samples, 3);
    assert!(parsed.child.is_none());
}

#[test]
fn child_mode_retains_every_required_argument() {
    let parsed = parse_arguments_from(argument_strings(&[
        "--child-full",
        "child-data",
        "--documents",
        "10",
        "--payload-bytes",
        "20",
        "--churn-basis-points",
        "30",
    ]))
    .expect("complete child arguments should parse");
    let child = parsed
        .child
        .expect("child mode should retain its arguments");
    assert_eq!(child.data_dir, PathBuf::from("child-data"));
    assert_eq!(child.documents, 10);
    assert_eq!(child.payload_bytes, 20);
    assert_eq!(child.churn_basis_points, 30);
}
