//! Span enter benchmarks.

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tracing::Level;
use tracing::span;

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks entering an existing span.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
  shared::Dispatches.bench(&mut criterion.benchmark_group("enter_span"), |bencher| {
    let span = span!(Level::TRACE, "span");
    bencher.iter(|| {
      let _span = span.enter();
    });
  });
}

criterion_group!(benches, bench);
criterion_main!(benches);
