//! `Dispatch::get_default` reference benchmarks.

use std::hint::black_box;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tracing::dispatcher::get_default;

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks borrowing the current default dispatch.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
  shared::Dispatches.bench(&mut criterion.benchmark_group("Dispatch::get_ref"), |bencher| {
    bencher.iter(|| {
      get_default(|current| {
        let _value = black_box(&current);
      });
    });
  });
}

criterion_group!(benches, bench);
criterion_main!(benches);
