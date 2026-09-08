//! Per-run spans: the function invocation, every host call the function
//! made (database operations, scheduler commands, nested function calls),
//! and the nested functions themselves, each with its offset from the run
//! start, its duration, and its outcome. The run row stores the spans, and
//! the console draws them as a waterfall.

use std::sync::Mutex;
use std::time::Instant;

use serde_json::{Value, json};

/// The most spans one run keeps. Past it the recorder counts what it
/// dropped, so the row says the waterfall is partial.
pub const RUN_SPAN_LIMIT: usize = 500;

/// One recorded span. `parent` is the index of the enclosing span in the
/// run's span list; the function's own span has none.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSpan {
    pub name: String,
    pub kind: &'static str,
    pub parent: Option<usize>,
    pub start_ms: f64,
    pub duration_ms: f64,
    pub status: &'static str,
}

impl RunSpan {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "kind": self.kind,
            "parent": self.parent,
            "startMs": self.start_ms,
            "durationMs": self.duration_ms,
            "status": self.status,
        })
    }
}

/// The span kind of a host operation, from its dotted operation name.
pub fn span_kind_for_operation(operation: &str) -> &'static str {
    if operation.contains(".db.") {
        "db"
    } else if operation.contains(".scheduler.") {
        "scheduler"
    } else if operation.contains(".ctx.run_")
        || operation.ends_with(".ctx.query")
        || operation.ends_with(".ctx.paginated_query")
        || operation.ends_with(".ctx.mutation")
        || operation.ends_with(".ctx.action")
    {
        "function"
    } else {
        "host"
    }
}

/// A span that is open. Hand it back to [`RunSpanRecorder::finish`].
#[derive(Debug, Clone, Copy)]
pub struct OpenSpan(Option<usize>);

struct RecorderState {
    spans: Vec<RunSpan>,
    open: Vec<usize>,
    dropped: usize,
    started: Vec<Instant>,
}

/// Records the spans of one run. Shared across the nested host bridges of
/// the run, so a nested function's operations land under the call that
/// invoked it: a span's parent is the innermost span still open when it
/// starts.
pub struct RunSpanRecorder {
    origin: Instant,
    state: Mutex<RecorderState>,
}

impl Default for RunSpanRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl RunSpanRecorder {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            state: Mutex::new(RecorderState {
                spans: Vec::new(),
                open: Vec::new(),
                dropped: 0,
                started: Vec::new(),
            }),
        }
    }

    /// Open a span under the innermost open span.
    pub fn start(&self, kind: &'static str, name: impl Into<String>) -> OpenSpan {
        let now = Instant::now();
        let mut state = self.lock();
        if state.spans.len() >= RUN_SPAN_LIMIT {
            state.dropped += 1;
            return OpenSpan(None);
        }
        let index = state.spans.len();
        let parent = state.open.last().copied();
        state.spans.push(RunSpan {
            name: name.into(),
            kind,
            parent,
            start_ms: now.duration_since(self.origin).as_secs_f64() * 1000.0,
            duration_ms: 0.0,
            status: "open",
        });
        state.started.push(now);
        state.open.push(index);
        OpenSpan(Some(index))
    }

    /// Close a span with its outcome. Any span opened under it and left
    /// open closes with it.
    pub fn finish(&self, span: OpenSpan, ok: bool) {
        let Some(index) = span.0 else {
            return;
        };
        let now = Instant::now();
        let mut state = self.lock();
        if let Some(position) = state.open.iter().rposition(|open| *open == index) {
            state.open.truncate(position);
        }
        let started = state.started[index];
        let recorded = &mut state.spans[index];
        recorded.duration_ms = now.duration_since(started).as_secs_f64() * 1000.0;
        recorded.status = if ok { "ok" } else { "error" };
    }

    /// The spans recorded so far, and how many were dropped past the limit.
    pub fn snapshot(&self) -> (Vec<RunSpan>, usize) {
        let state = self.lock();
        (state.spans.clone(), state.dropped)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RecorderState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_nest_under_the_innermost_open_span() {
        let recorder = RunSpanRecorder::new();
        let root = recorder.start("function", "messages:send");
        let call = recorder.start("function", "convex.ctx.run_query");
        let nested = recorder.start("function", "messages:list");
        let read = recorder.start("db", "convex.ctx.db.query.collect");
        recorder.finish(read, true);
        recorder.finish(nested, true);
        recorder.finish(call, true);
        let write = recorder.start("db", "convex.ctx.db.insert");
        recorder.finish(write, false);
        recorder.finish(root, false);

        let (spans, dropped) = recorder.snapshot();
        assert_eq!(dropped, 0);
        let parents: Vec<Option<usize>> = spans.iter().map(|span| span.parent).collect();
        assert_eq!(parents, vec![None, Some(0), Some(1), Some(2), Some(0)]);
        let statuses: Vec<&str> = spans.iter().map(|span| span.status).collect();
        assert_eq!(statuses, vec!["error", "ok", "ok", "ok", "error"]);
        assert!(spans.iter().all(|span| span.start_ms >= 0.0));
        assert!(spans[0].duration_ms >= spans[1].duration_ms);
    }

    #[test]
    fn an_unfinished_child_closes_with_its_parent_for_nesting_purposes() {
        let recorder = RunSpanRecorder::new();
        let root = recorder.start("function", "f");
        let _left_open = recorder.start("db", "convex.ctx.db.get");
        recorder.finish(root, true);
        let sibling = recorder.start("db", "convex.ctx.db.get");
        recorder.finish(sibling, true);
        let (spans, _) = recorder.snapshot();
        assert_eq!(spans[2].parent, None, "the closed root no longer parents");
        assert_eq!(spans[1].status, "open");
    }

    #[test]
    fn the_limit_counts_dropped_spans() {
        let recorder = RunSpanRecorder::new();
        for _ in 0..(RUN_SPAN_LIMIT + 3) {
            let span = recorder.start("db", "convex.ctx.db.get");
            recorder.finish(span, true);
        }
        let (spans, dropped) = recorder.snapshot();
        assert_eq!(spans.len(), RUN_SPAN_LIMIT);
        assert_eq!(dropped, 3);
    }

    #[test]
    fn operation_names_map_to_span_kinds() {
        assert_eq!(span_kind_for_operation("convex.ctx.db.insert"), "db");
        assert_eq!(span_kind_for_operation("convex.ctx.db.query.collect"), "db");
        assert_eq!(
            span_kind_for_operation("convex.ctx.scheduler.run_after"),
            "scheduler"
        );
        assert_eq!(span_kind_for_operation("convex.ctx.run_query"), "function");
        assert_eq!(span_kind_for_operation("convex.ctx.mutation"), "function");
        assert_eq!(span_kind_for_operation("cloudflare.kv.get"), "host");
    }
}
