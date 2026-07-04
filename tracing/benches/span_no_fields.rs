//! Span benchmarks without fields.

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tracing::Level;
use tracing::span;

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks constructing spans without fields.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
  shared::Recording.bench(&mut criterion.benchmark_group("span_no_fields"), |bencher| {
    bencher.iter(|| span!(Level::TRACE, "span"));
  });
}

criterion_group!(benches, bench);
criterion_main!(benches);
