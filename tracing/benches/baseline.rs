//! Baseline tracing benchmarks.

use std::hint::black_box;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;

/// Benchmarks baseline operations used for comparison with tracing operations.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;

  let mut group = criterion.benchmark_group("comparison");
  let _relaxed_load_benchmark = group.bench_function("relaxed_load", |bencher| {
    let foo = AtomicUsize::new(1);
    bencher.iter(|| black_box(foo.load(Ordering::Relaxed)));
  });
  let _acquire_load_benchmark = group.bench_function("acquire_load", |bencher| {
    let foo = AtomicUsize::new(1);
    bencher.iter(|| black_box(foo.load(Ordering::Acquire)));
  });
  let _log_benchmark = group.bench_function("log", |bencher| {
    bencher.iter(|| {
      log::log!(log::Level::Info, "log");
    });
  });
  group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
