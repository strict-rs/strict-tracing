//! Span field benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tracing::{Level, field, span};

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks constructing spans with fields.
#[allow(
    clippy::single_call_fn,
    reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
    shared::Recording.bench(&mut criterion.benchmark_group("span_fields"), |bencher| {
        bencher.iter(|| {
            let span = span!(
                Level::TRACE,
                "span",
                foo = "foo",
                bar = "bar",
                baz = 3,
                quuux = field::debug(0.99)
            );
            let _value = black_box(span);
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
