use nimbus_core::{Error, Result};

use super::{
    OperatorPolicyDiff, OperatorPolicyDocument, OperatorPolicyEvaluation, OperatorPolicyLifecycle,
};

#[derive(Debug, Clone)]
pub struct OperatorPolicyReloadState {
    current: OperatorPolicyDocument,
    evaluation: OperatorPolicyEvaluation,
}

impl OperatorPolicyReloadState {
    pub fn new(current: OperatorPolicyDocument) -> Result<Self> {
        let evaluation = current.evaluate()?;
        Ok(Self {
            current,
            evaluation,
        })
    }

    pub fn evaluation(&self) -> &OperatorPolicyEvaluation {
        &self.evaluation
    }

    pub fn reload(&mut self, candidate: OperatorPolicyDocument) -> OperatorPolicyReloadOutcome {
        let diff = match OperatorPolicyDiff::between(&self.current, &candidate) {
            Ok(diff) => diff,
            Err(error) => {
                return OperatorPolicyReloadOutcome::rejected(error, self.evaluation.clone());
            }
        };
        let evaluation = match candidate.evaluate() {
            Ok(evaluation) => evaluation,
            Err(error) => {
                return OperatorPolicyReloadOutcome::rejected(error, self.evaluation.clone());
            }
        };
        let lifecycle = diff.lifecycle();
        self.current = candidate;
        self.evaluation = evaluation.clone();
        OperatorPolicyReloadOutcome::applied(lifecycle, evaluation)
    }
}

#[derive(Debug, Clone)]
pub struct OperatorPolicyReloadOutcome {
    pub applied: bool,
    pub lifecycle: Option<OperatorPolicyLifecycle>,
    pub error: Option<String>,
    pub active_decision_ids: Vec<String>,
}

impl OperatorPolicyReloadOutcome {
    fn applied(lifecycle: OperatorPolicyLifecycle, evaluation: OperatorPolicyEvaluation) -> Self {
        Self {
            applied: true,
            lifecycle: Some(lifecycle),
            error: None,
            active_decision_ids: evaluation
                .decisions
                .into_iter()
                .map(|decision| decision.decision_id)
                .collect(),
        }
    }

    fn rejected(error: Error, evaluation: OperatorPolicyEvaluation) -> Self {
        Self {
            applied: false,
            lifecycle: None,
            error: Some(error.to_string()),
            active_decision_ids: evaluation
                .decisions
                .into_iter()
                .map(|decision| decision.decision_id)
                .collect(),
        }
    }
}
