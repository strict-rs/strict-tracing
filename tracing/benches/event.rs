//! Event dispatch benchmarks.

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tracing::info;

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks emitting an event.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
  shared::Recording.bench(&mut criterion.benchmark_group("event"), |bencher| {
    bencher.iter(|| info!("hello world!"));
  });
}

criterion_group!(benches, bench);
criterion_main!(benches);
