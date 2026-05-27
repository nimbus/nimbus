use std::time::{Duration, Instant};

use tracing::warn;

#[derive(Clone, Copy, Debug)]
pub(crate) enum LatencySegment {
    Auth,
    Storage,
    Runtime,
}

impl LatencySegment {
    fn label(self) -> &'static str {
        match self {
            Self::Auth => "server.auth",
            Self::Storage => "server.storage",
            Self::Runtime => "server.runtime",
        }
    }

    fn budget(self) -> Duration {
        match self {
            Self::Auth => Duration::from_millis(10),
            Self::Storage => Duration::from_millis(50),
            Self::Runtime => Duration::from_millis(100),
        }
    }
}

pub(crate) struct SegmentTimer {
    segment: LatencySegment,
    started: Instant,
    finished: bool,
}

pub(crate) fn budgeted_segment(segment: LatencySegment) -> SegmentTimer {
    SegmentTimer {
        segment,
        started: Instant::now(),
        finished: false,
    }
}

impl SegmentTimer {
    pub(crate) fn finish(mut self) -> Duration {
        self.finish_now()
    }

    fn finish_now(&mut self) -> Duration {
        self.finished = true;
        let elapsed = self.started.elapsed();
        let budget = self.segment.budget();
        if elapsed > budget {
            warn!(
                latency_segment = self.segment.label(),
                budgeted_segment = self.segment.label(),
                elapsed_ms = elapsed.as_secs_f64() * 1000.0,
                budget_ms = budget.as_secs_f64() * 1000.0,
                "latency segment exceeded budget"
            );
        }
        elapsed
    }
}

impl Drop for SegmentTimer {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish_now();
        }
    }
}
