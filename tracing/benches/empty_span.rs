//! Empty span benchmarks.

use std::hint::black_box;
use std::sync::Arc;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tracing::span::Span;

pub mod shared;
use shared::BenchmarkMatrix as _;

/// Benchmarks constructing disabled spans and an equivalent local structure.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion invokes this benchmark entrypoint through criterion_group"
)]
fn bench(criterion: &mut Criterion) {
  let mut group = criterion.benchmark_group("empty_span");
  shared::Dispatches.bench(&mut group, |bencher| {
    bencher.iter(|| {
      let span = Span::none();
      let _value = black_box(&span);
    });
  });
  let _baseline_struct_benchmark = group.bench_function("baseline_struct", |bencher| {
    bencher.iter(|| {
      let span = FakeEmptySpan {
        inner: None, meta: None
      };
      let _value = black_box(&span);
    });
  });
}

/// Local structure matching the shape of a disabled span.
struct FakeEmptySpan {
  /// Optional span identifier and shared dispatch handle.
  inner: Option<(usize, Arc<()>)>,

  /// Optional static metadata marker.
  meta: Option<&'static ()>,
}

impl Drop for FakeEmptySpan {
  fn drop(&mut self) {
    let _inner = black_box(&self.inner);
    let _meta = black_box(&self.meta);
  }
}

criterion_group!(benches, bench);
criterion_main!(benches);
