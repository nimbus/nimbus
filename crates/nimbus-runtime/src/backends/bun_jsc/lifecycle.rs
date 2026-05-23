use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BunJscLifecycleState {
    Created,
    BootstrapReady,
    GuestEntered,
    CancelRequested,
    Terminated,
    ResetOrDiscarded,
    TeardownComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BunJscLifecycleAck {
    BootstrapReady,
    GuestEntered,
    CancelRequested,
    Terminated,
    ResetOrDiscarded,
    TeardownComplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BunJscLifecycleTransition {
    pub(crate) from: BunJscLifecycleState,
    pub(crate) ack: BunJscLifecycleAck,
    pub(crate) to: BunJscLifecycleState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BunJscLifecycleTrace {
    state: BunJscLifecycleState,
    transitions: Vec<BunJscLifecycleTransition>,
}

impl BunJscLifecycleTrace {
    pub(crate) fn new() -> Self {
        Self {
            state: BunJscLifecycleState::Created,
            transitions: Vec::new(),
        }
    }

    pub(crate) fn state(&self) -> BunJscLifecycleState {
        self.state
    }

    pub(crate) fn transitions(&self) -> &[BunJscLifecycleTransition] {
        &self.transitions
    }

    pub(crate) fn acknowledge(
        &mut self,
        ack: BunJscLifecycleAck,
    ) -> Result<BunJscLifecycleState, BunJscLifecycleError> {
        let to = self.state.next(ack)?;
        self.transitions.push(BunJscLifecycleTransition {
            from: self.state,
            ack,
            to,
        });
        self.state = to;
        Ok(to)
    }
}

impl Default for BunJscLifecycleTrace {
    fn default() -> Self {
        Self::new()
    }
}

impl BunJscLifecycleState {
    fn next(self, ack: BunJscLifecycleAck) -> Result<BunJscLifecycleState, BunJscLifecycleError> {
        match (self, ack) {
            (Self::Created, BunJscLifecycleAck::BootstrapReady) => Ok(Self::BootstrapReady),
            (Self::BootstrapReady, BunJscLifecycleAck::GuestEntered) => Ok(Self::GuestEntered),
            (Self::GuestEntered, BunJscLifecycleAck::CancelRequested) => Ok(Self::CancelRequested),
            (Self::GuestEntered, BunJscLifecycleAck::Terminated)
            | (Self::CancelRequested, BunJscLifecycleAck::Terminated) => Ok(Self::Terminated),
            (Self::Terminated, BunJscLifecycleAck::ResetOrDiscarded) => Ok(Self::ResetOrDiscarded),
            (Self::ResetOrDiscarded, BunJscLifecycleAck::TeardownComplete) => {
                Ok(Self::TeardownComplete)
            }
            (from, ack) => Err(BunJscLifecycleError { from, ack }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BunJscLifecycleError {
    pub(crate) from: BunJscLifecycleState,
    pub(crate) ack: BunJscLifecycleAck,
}

impl std::fmt::Display for BunJscLifecycleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid Bun/JSC lifecycle ack {:?} from {:?}",
            self.ack, self.from
        )
    }
}

impl std::error::Error for BunJscLifecycleError {}
