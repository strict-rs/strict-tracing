//! Shared helpers for `tracing` benchmarks.

use core::num::NonZeroU64;
use criterion::{Bencher, BenchmarkGroup, measurement::WallTime};
use log::{LevelFilter, set_logger, set_max_level};
use std::{
    fmt::{self, Write},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
};
use tracing::{
    Event, Id, Metadata, Subscriber, field, span,
    subscriber::{SubscriberResult, set_global_default, with_default},
};

/// Subscriber matrix used by a benchmark group.
pub trait BenchmarkMatrix {
    /// Runs one benchmark body across this matrix's subscriber configurations.
    fn bench<I>(&self, group: &mut BenchmarkGroup<'_, WallTime>, iter: I)
    where
        I: FnMut(&mut Bencher<'_, WallTime>);
}

/// Matrix for benchmarks that should visit recorded fields.
#[derive(Clone, Copy, Debug)]
pub struct Recording;

impl BenchmarkMatrix for Recording {
    fn bench<I>(&self, group: &mut BenchmarkGroup<'_, WallTime>, mut iter: I)
    where
        I: FnMut(&mut Bencher<'_, WallTime>),
    {
        let _none_benchmark = group.bench_function("none", &mut iter);

        with_default(EnabledSubscriber, || {
            let _scoped_benchmark = group.bench_function("scoped", &mut iter);
        });

        let subscriber = VisitingSubscriber::default();
        with_default(subscriber, || {
            let _scoped_recording_benchmark = group.bench_function("scoped_recording", &mut iter);
        });

        let _global_default_already_set = set_global_default(EnabledSubscriber).is_err();
        let _logger_already_set = set_logger(&NOP_LOGGER).is_err();
        set_max_level(LevelFilter::Trace);
        let _global_benchmark = group.bench_function("global", &mut iter);
    }
}

/// Matrix for benchmarks that exercise default dispatch lookup.
#[derive(Clone, Copy, Debug)]
pub struct Dispatches;

impl BenchmarkMatrix for Dispatches {
    fn bench<I>(&self, group: &mut BenchmarkGroup<'_, WallTime>, mut iter: I)
    where
        I: FnMut(&mut Bencher<'_, WallTime>),
    {
        let _none_benchmark = group.bench_function("none", &mut iter);

        with_default(EnabledSubscriber, || {
            let _scoped_benchmark = group.bench_function("scoped", &mut iter);
        });

        let _global_default_already_set = set_global_default(EnabledSubscriber).is_err();
        let _logger_already_set = set_logger(&NOP_LOGGER).is_err();
        set_max_level(LevelFilter::Trace);
        let _global_benchmark = group.bench_function("global", &mut iter);
    }
}

/// Logger used when benchmarks install a global default subscriber.
const NOP_LOGGER: NopLogger = NopLogger;

/// Logger implementation that formats records into `black_box`.
struct NopLogger;

impl log::Log for NopLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let mut this = self;
            let _write_failed = write!(this, "{}", record.args()).is_err();
        }
    }

    fn flush(&self) {}
}

impl Write for &NopLogger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let _value = black_box(s);
        Ok(())
    }
}

/// Simulates a subscriber that records span data.
#[derive(Default)]
struct VisitingSubscriber {
    /// Total number of debug fields visited.
    recorded_fields: AtomicUsize,
}

impl VisitingSubscriber {
    /// Creates a visitor for one span or event recording pass.
    const fn visitor(&self) -> Visitor<'_> {
        Visitor {
            recorded_fields: &self.recorded_fields,
            local_fields: 0,
        }
    }
}

/// Field visitor that counts values observed by the subscriber.
struct Visitor<'a> {
    /// Shared field counter updated after a visit finishes.
    recorded_fields: &'a AtomicUsize,

    /// Fields observed during this visit.
    local_fields: usize,
}

impl Drop for Visitor<'_> {
    fn drop(&mut self) {
        let _previous_fields = self
            .recorded_fields
            .fetch_add(self.local_fields, Ordering::Relaxed);
    }
}

impl field::Visit for Visitor<'_> {
    fn record_debug(&mut self, _field: &field::Field, value: &dyn fmt::Debug) {
        let _value = black_box(value);
        self.local_fields = self.local_fields.saturating_add(1);
    }
}

impl Subscriber for VisitingSubscriber {
    fn new_span(&self, span: &span::Attributes<'_>) -> SubscriberResult<Id> {
        let mut visitor = self.visitor();
        span.record(&mut visitor);
        Ok(benchmark_span_id())
    }

    fn record(&self, _span: Id, values: &span::Record<'_>) -> SubscriberResult {
        let mut visitor = self.visitor();
        values.record(&mut visitor);
        Ok(())
    }

    fn event(&self, event: &Event<'_>) -> SubscriberResult {
        let mut visitor = self.visitor();
        event.record(&mut visitor);
        Ok(())
    }

    fn record_follows_from(&self, _span: Id, _follows: Id) -> SubscriberResult {
        Ok(())
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
        Ok(true)
    }

    fn enter(&self, _span: Id) -> SubscriberResult {
        Ok(())
    }

    fn exit(&self, _span: Id) -> SubscriberResult {
        Ok(())
    }
}

/// A subscriber that is enabled but otherwise does nothing.
struct EnabledSubscriber;

impl Subscriber for EnabledSubscriber {
    fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<Id> {
        Ok(benchmark_span_id())
    }

    fn event(&self, _event: &Event<'_>) -> SubscriberResult {
        Ok(())
    }

    fn record(&self, _span: Id, _values: &span::Record<'_>) -> SubscriberResult {
        Ok(())
    }

    fn record_follows_from(&self, _span: Id, _follows: Id) -> SubscriberResult {
        Ok(())
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
        Ok(true)
    }

    fn enter(&self, _span: Id) -> SubscriberResult {
        Ok(())
    }

    fn exit(&self, _span: Id) -> SubscriberResult {
        Ok(())
    }
}

/// Returns the stable span ID used by the synthetic benchmark subscribers.
fn benchmark_span_id() -> Id {
    Id::try_from_u64(0xDEAD_FACE).unwrap_or_else(|| Id::from_non_zero_u64(NonZeroU64::MIN))
}
