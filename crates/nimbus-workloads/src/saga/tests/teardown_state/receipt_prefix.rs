use super::*;

#[test]
fn teardown_receipt_prefix_tracks_exact_durable_order_and_sparse_history() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let withdrawn = complete_effectful_teardown_step(&withdrawal, "prefix-withdrawn");
    let drained = complete_effectful_teardown_step(&withdrawn, "prefix-drained");
    let (stop_pending, stop_claim) = claim_teardown_step(&drained);
    let prefix = stop_pending
        .teardown_receipt_prefix_for_claim(&stop_claim)
        .expect("current claim should project its exact receipt prefix");

    assert_eq!(prefix.receipts().len(), 2);
    assert_eq!(
        prefix
            .receipts()
            .iter()
            .map(|receipt| receipt.claim().attempt().step())
            .collect::<Vec<_>>(),
        [
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownStep::DrainExecution,
        ]
    );
    assert_eq!(
        prefix.receipt_for(WorkloadTeardownStep::DrainExecution),
        prefix.receipts().get(1)
    );

    let resource_free_withdrawal = withdrawal_record(WorkloadPublicationIntent::Withheld);
    let withdrawn = resource_free_withdrawal
        .record_resource_free_teardown_step(WorkloadTeardownStep::WithdrawPublication)
        .expect("withheld publication should advance without a receipt");
    let (drain_pending, drain_claim) = claim_teardown_step(&withdrawn);
    let sparse = drain_pending
        .teardown_receipt_prefix_for_claim(&drain_claim)
        .expect("resource-free receipt gaps are valid");
    assert!(sparse.receipts().is_empty());
}

#[test]
fn teardown_receipt_prefix_rejects_crossed_and_out_of_order_history() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let withdrawn = complete_effectful_teardown_step(&withdrawal, "prefix-current-withdrawn");
    let drained = complete_effectful_teardown_step(&withdrawn, "prefix-current-drained");
    let (pending, claim) = claim_teardown_step(&drained);
    let prefix = pending
        .teardown_receipt_prefix_for_claim(&claim)
        .expect("current prefix should validate");

    let other_withdrawal = withdrawal_record_for(WorkloadPublicationIntent::PublishWhenReady, 3);
    let other_withdrawn =
        complete_effectful_teardown_step(&other_withdrawal, "prefix-other-withdrawn");
    let other_drained = complete_effectful_teardown_step(&other_withdrawn, "prefix-other-drained");
    let (other_pending, other_claim) = claim_teardown_step(&other_drained);
    let crossed = other_pending
        .teardown_receipt_prefix_for_claim(&other_claim)
        .expect("other prefix should be valid for its own claim");
    assert!(crossed.validate_for_claim(&claim).is_err());

    let mut reordered = serde_json::to_value(&prefix).expect("prefix should encode");
    reordered["receipts"]
        .as_array_mut()
        .expect("receipts should encode as a list")
        .swap(0, 1);
    assert!(
        serde_json::from_value::<WorkloadTeardownReceiptPrefix>(reordered).is_err(),
        "standalone prefix decoding must reject reordered receipts"
    );

    let encoded = serde_json::to_value(&prefix).expect("prefix should encode");
    let mut duplicate = encoded.clone();
    duplicate["receipts"][1] = duplicate["receipts"][0].clone();
    let mut missing = encoded.clone();
    missing
        .as_object_mut()
        .expect("prefix should be an object")
        .remove("receipts");
    let mut null = encoded.clone();
    null["receipts"] = serde_json::Value::Null;
    let mut unknown = encoded;
    unknown["unknown"] = serde_json::Value::Bool(true);
    for (case, value) in [
        ("duplicate", duplicate),
        ("missing", missing),
        ("null", null),
        ("unknown", unknown),
    ] {
        assert!(
            serde_json::from_value::<WorkloadTeardownReceiptPrefix>(value).is_err(),
            "standalone prefix decoding must reject {case} receipt history"
        );
    }
}
