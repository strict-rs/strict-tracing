//! Benchmarks span enter paths.

use criterion::{
    BenchmarkGroup, Criterion, criterion_group, criterion_main, measurement::WallTime,
};
use tracing_subscriber::prelude::*;

/// The `Criterion` group type used by these benchmarks.
type Group<'a> = BenchmarkGroup<'a, WallTime>;

/// Consumes Criterion's fluent group return value after registering a benchmark.
const fn register_benchmark(_group: &mut Group<'_>) {}

/// Benchmarks entering enabled and disabled spans while keeping the guard alive.
#[allow(
    clippy::single_call_fn,
    reason = "Criterion entry point is referenced through `criterion_group!`"
)]
fn enter(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("enter");
    let _subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .finish()
        .set_default();
    register_benchmark(group.bench_function("enabled", |bencher| {
        let span = tracing::info_span!("foo");
        bencher.iter_with_large_drop(|| span.enter());
    }));
    register_benchmark(group.bench_function("disabled", |bencher| {
        let span = tracing::debug_span!("foo");
        bencher.iter_with_large_drop(|| span.enter());
    }));
}

/// Benchmarks entering enabled and disabled spans and immediately dropping the guard.
#[allow(
    clippy::single_call_fn,
    reason = "Criterion entry point is referenced through `criterion_group!`"
)]
fn enter_exit(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("enter_exit");
    let _subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .finish()
        .set_default();
    register_benchmark(group.bench_function("enabled", |bencher| {
        let span = tracing::info_span!("foo");
        bencher.iter(|| span.enter());
    }));
    register_benchmark(group.bench_function("disabled", |bencher| {
        let span = tracing::debug_span!("foo");
        bencher.iter(|| span.enter());
    }));
}

/// Benchmarks entering spans while several parent spans are already entered.
#[allow(
    clippy::single_call_fn,
    reason = "Criterion entry point is referenced through `criterion_group!`"
)]
fn enter_many(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("enter_many");
    let _subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .finish()
        .set_default();
    register_benchmark(group.bench_function("enabled", |bencher| {
        let span1 = tracing::info_span!("span1");
        let _enter1 = span1.enter();
        let span2 = tracing::info_span!("span2");
        let _enter2 = span2.enter();
        let span3 = tracing::info_span!("span3");
        let _enter3 = span3.enter();
        let span = tracing::info_span!("foo");
        bencher.iter_with_large_drop(|| span.enter());
    }));
    register_benchmark(group.bench_function("disabled", |bencher| {
        let span1 = tracing::info_span!("span1");
        let _enter1 = span1.enter();
        let span2 = tracing::info_span!("span2");
        let _enter2 = span2.enter();
        let span3 = tracing::info_span!("span3");
        let _enter3 = span3.enter();
        let span = tracing::debug_span!("foo");
        bencher.iter_with_large_drop(|| span.enter());
    }));
}
criterion_group!(benches, enter, enter_exit, enter_many);
criterion_main!(benches);
