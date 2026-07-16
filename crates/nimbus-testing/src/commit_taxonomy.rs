use std::fmt::Debug;
use std::time::Duration;

use nimbus_core::{CommitErrorClass, Error, MutationCap, SequenceNumber};

/// One canonical core error for every closed commit-path class.
///
/// Adapter conformance tests consume this list so extending the core taxonomy
/// makes every expectation table fail until its wire mapping is deliberate.
pub fn canonical_commit_errors() -> Vec<(CommitErrorClass, Error)> {
    vec![
        (
            CommitErrorClass::Conflict,
            Error::retryable_conflict("canonical write conflict", Some(SequenceNumber(7))),
        ),
        (
            CommitErrorClass::Overloaded,
            Error::overloaded("canonical node pressure"),
        ),
        (
            CommitErrorClass::CommitterFull,
            Error::committer_full("canonical committer pressure", 128),
        ),
        (
            CommitErrorClass::RejectedBeforeExecution,
            Error::rejected_before_execution("canonical admission rejection"),
        ),
        (
            CommitErrorClass::RateLimited,
            Error::rate_limited("canonical tenant rate limit", Duration::from_millis(250)),
        ),
        (
            CommitErrorClass::OutOfRetention,
            Error::out_of_retention("canonical snapshot expiry", Some(SequenceNumber(3))),
        ),
        (
            CommitErrorClass::CapExceeded,
            Error::cap_exceeded(MutationCap::DocumentsWritten, 17, 16),
        ),
    ]
}

/// Asserts that an adapter maps every canonical commit class exactly once.
pub fn assert_commit_taxonomy_mapping<T: PartialEq + Debug>(
    mapper: impl Fn(&Error) -> T,
    expectations: &[(CommitErrorClass, T)],
) {
    let canonical = canonical_commit_errors();
    assert_eq!(
        expectations.len(),
        canonical.len(),
        "commit taxonomy expectation table must contain exactly one row per canonical class"
    );

    for (class, error) in canonical {
        let matching = expectations
            .iter()
            .filter(|(expected_class, _)| *expected_class == class)
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "commit taxonomy expectation table must contain {class:?} exactly once"
        );

        let actual = mapper(&error);
        assert_eq!(
            &actual, &matching[0].1,
            "unexpected mapping for {class:?}: {error}"
        );
    }
}
