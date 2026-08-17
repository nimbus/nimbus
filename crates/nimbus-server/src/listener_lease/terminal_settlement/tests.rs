use super::{TerminalListenerSettlementError, TerminalListenerSettlementFailureKind};

#[test]
fn terminal_error_classes_remain_distinct() {
    let cases = [
        (
            TerminalListenerSettlementError::invalid("invalid"),
            TerminalListenerSettlementFailureKind::InvalidBatch,
        ),
        (
            TerminalListenerSettlementError::withdrawal("withdrawal"),
            TerminalListenerSettlementFailureKind::DurableWithdrawalAmbiguous,
        ),
        (
            TerminalListenerSettlementError::stop("stop"),
            TerminalListenerSettlementFailureKind::StopOrJoinAmbiguous,
        ),
        (
            TerminalListenerSettlementError::release("release"),
            TerminalListenerSettlementFailureKind::DurableReleaseAmbiguous,
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.kind(), expected);
    }
}
