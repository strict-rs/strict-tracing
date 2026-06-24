#![allow(dead_code)]
//! Shared helpers for `tracing` benchmarks.

use criterion::{Bencher, measurement::WallTime};
use std::hint::black_box;
use tracing::{Event, Id, Metadata, field, span};

use std::{
    fmt::{self, Write},
    sync::{Mutex, MutexGuard},
};

pub(crate) fn for_all_recording(
    group: &mut criterion::BenchmarkGroup<'_, WallTime>,
    mut iter: impl FnMut(&mut Bencher<'_, WallTime>),
) {
    // first, run benchmarks with no subscriber
    let _benchmark = group.bench_function("none", &mut iter);

    // then, run benchmarks with a scoped default subscriber
    tracing::subscriber::with_default(EnabledSubscriber, || {
        let _benchmark = group.bench_function("scoped", &mut iter);
    });

    let subscriber = VisitingSubscriber(Mutex::new(String::from("")));
    tracing::subscriber::with_default(subscriber, || {
        let _benchmark = group.bench_function("scoped_recording", &mut iter);
    });

    // finally, set a global default subscriber, and run the benchmarks again.
    tracing::subscriber::set_global_default(EnabledSubscriber)
        .expect("global default should not have already been set!");
    let _ = log::set_logger(&NOP_LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
    let _benchmark = group.bench_function("global", &mut iter);
}

pub(crate) fn for_all_dispatches(
    group: &mut criterion::BenchmarkGroup<'_, WallTime>,
    mut iter: impl FnMut(&mut Bencher<'_, WallTime>),
) {
    // first, run benchmarks with no subscriber
    let _benchmark = group.bench_function("none", &mut iter);

    // then, run benchmarks with a scoped default subscriber
    tracing::subscriber::with_default(EnabledSubscriber, || {
        let _benchmark = group.bench_function("scoped", &mut iter);
    });

    // finally, set a global default subscriber, and run the benchmarks again.
    tracing::subscriber::set_global_default(EnabledSubscriber)
        .expect("global default should not have already been set!");
    let _ = log::set_logger(&NOP_LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
    let _benchmark = group.bench_function("global", &mut iter);
}

const NOP_LOGGER: NopLogger = NopLogger;

struct NopLogger;

impl log::Log for NopLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            let mut this = self;
            let _ = write!(this, "{}", record.args());
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
struct VisitingSubscriber(Mutex<String>);

struct Visitor<'a>(MutexGuard<'a, String>);

impl field::Visit for Visitor<'_> {
    fn record_debug(&mut self, _field: &field::Field, value: &dyn fmt::Debug) {
        let _ = write!(&mut *self.0, "{:?}", value);
    }
}

impl tracing::Subscriber for VisitingSubscriber {
    fn new_span(&self, span: &span::Attributes<'_>) -> Id {
        let mut visitor = Visitor(self.0.lock().unwrap());
        span.record(&mut visitor);
        Id::from_u64(0xDEAD_FACE)
    }

    fn record(&self, _span: &Id, values: &span::Record<'_>) {
        let mut visitor = Visitor(self.0.lock().unwrap());
        values.record(&mut visitor);
    }

    fn event(&self, event: &Event<'_>) {
        let mut visitor = Visitor(self.0.lock().unwrap());
        event.record(&mut visitor);
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        let _ = (span, follows);
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let _ = metadata;
        true
    }

    fn enter(&self, span: &Id) {
        let _ = span;
    }

    fn exit(&self, span: &Id) {
        let _ = span;
    }
}

/// A subscriber that is enabled but otherwise does nothing.
struct EnabledSubscriber;

impl tracing::Subscriber for EnabledSubscriber {
    fn new_span(&self, span: &span::Attributes<'_>) -> Id {
        let _ = span;
        Id::from_u64(0xDEAD_FACE)
    }

    fn event(&self, event: &Event<'_>) {
        let _ = event;
    }

    fn record(&self, span: &Id, values: &span::Record<'_>) {
        let _ = (span, values);
    }

    fn record_follows_from(&self, span: &Id, follows: &Id) {
        let _ = (span, follows);
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let _ = metadata;
        true
    }

    fn enter(&self, span: &Id) {
        let _ = span;
    }

    fn exit(&self, span: &Id) {
        let _ = span;
    }
}
