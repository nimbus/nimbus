use std::time::{Duration, Instant};

use tracing::warn;

#[derive(Clone, Copy, Debug)]
pub(crate) enum LatencySegment {
    TenantLoad,
    WaitVisibility,
    QueryPrepare,
    QueryExecute,
    QueryCache,
}

impl LatencySegment {
    fn label(self) -> &'static str {
        match self {
            Self::TenantLoad => "engine.tenant_load",
            Self::WaitVisibility => "engine.wait_visibility",
            Self::QueryPrepare => "engine.query_prepare",
            Self::QueryExecute => "engine.query_execute",
            Self::QueryCache => "engine.query_cache",
        }
    }

    fn budget(self) -> Duration {
        match self {
            Self::TenantLoad => Duration::from_millis(50),
            Self::WaitVisibility => Duration::from_millis(25),
            Self::QueryPrepare => Duration::from_millis(5),
            Self::QueryExecute => Duration::from_millis(50),
            Self::QueryCache => Duration::from_millis(5),
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
