use serde::{Deserialize, Serialize};

use crate::SequenceNumber;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RequiredSequence(pub SequenceNumber);

impl RequiredSequence {
    pub fn new(sequence: SequenceNumber) -> Self {
        Self(sequence)
    }

    pub fn sequence(self) -> SequenceNumber {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedServingSnapshot {
    covered_sequence: SequenceNumber,
}

impl PinnedServingSnapshot {
    pub fn new(covered_sequence: SequenceNumber) -> Self {
        Self { covered_sequence }
    }

    pub fn covered_sequence(self) -> SequenceNumber {
        self.covered_sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ReadVisibility {
    Latest,
    AtLeast(RequiredSequence),
    Pinned(PinnedServingSnapshot),
}

impl ReadVisibility {
    pub fn required_sequence(self, latest_applied: SequenceNumber) -> RequiredSequence {
        match self {
            Self::Latest => RequiredSequence(latest_applied),
            Self::AtLeast(required) => required,
            Self::Pinned(snapshot) => RequiredSequence(snapshot.covered_sequence()),
        }
    }
}
