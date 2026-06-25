//! Repeated span benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tracing::{Level, span};

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks constructing repeated spans.
#[allow(
    clippy::single_call_fn,
    reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
    shared::Recording.bench(&mut criterion.benchmark_group("span_repeated"), |bencher| {
        let span_count = black_box(N_SPANS);
        bencher.iter(|| {
            (0..span_count).fold(span!(Level::TRACE, "span", i = 0), |_, index| {
                span!(Level::TRACE, "span", i = index)
            })
        });
    });
}

/// Number of spans to create per repeated-span iteration.
const N_SPANS: u64 = 100;
criterion_group!(benches, bench);
criterion_main!(benches);
