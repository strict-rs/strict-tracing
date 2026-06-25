//! `Dispatch::get_default` clone benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tracing::dispatcher::get_default;

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks cloning the current default dispatch.
#[allow(
    clippy::single_call_fn,
    reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
    shared::Dispatches.bench(
        &mut criterion.benchmark_group("Dispatch::get_clone"),
        |bencher| {
            bencher.iter(|| {
                let current = get_default(Clone::clone);
                let _value = black_box(current);
            });
        },
    );
}

criterion_group!(benches, bench);
criterion_main!(benches);
