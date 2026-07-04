//! Benchmarks formatting subscriber paths.

use std::io;
use std::time::Duration;

use criterion::BenchmarkGroup;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use criterion::measurement::WallTime;
use tracing::dispatcher::with_default;

/// Benchmark support shared with the filtering benchmarks.
#[path = "support/support.rs"]
pub mod support;
use support::MultithreadedBench;

/// A fake writer that doesn't actually do anything.
///
/// We want to measure the subscriber's overhead, *not* the performance of
/// stdout/file writers. Using a no-op Write implementation lets us only measure
/// the subscriber's overhead.
struct NoWriter;

impl io::Write for NoWriter {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    Ok(buf.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

/// Benchmarks creating spans through a formatting subscriber.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion entry point is referenced through `criterion_group!`"
)]
fn bench_new_span(criterion: &mut Criterion) {
  bench_thrpt(criterion, "new_span", |group, configured_span_count| {
    register_benchmark(group.bench_with_input(
      BenchmarkId::new("single_thread", configured_span_count),
      configured_span_count,
      |bencher, &span_count| {
        with_default(&mk_dispatch(), || {
          bencher.iter(|| {
            for span_index in 0..span_count {
              let _span = tracing::info_span!("span", span_index);
            }
          });
        });
      },
    ));
    register_benchmark(group.bench_with_input(
      BenchmarkId::new("multithreaded", configured_span_count),
      configured_span_count,
      |bencher, &span_count| {
        bencher.iter_custom(|iters| {
          let mut total = Duration::ZERO;
          let dispatch = mk_dispatch();
          for _ in 0..iters {
            let bench = MultithreadedBench::new(dispatch.clone());
            let elapsed = bench
              .thread(move || {
                for span_index in 0..span_count {
                  let _span = tracing::info_span!("span", span_index);
                }
              })
              .thread(move || {
                for span_index in 0..span_count {
                  let _span = tracing::info_span!("span", span_index);
                }
              })
              .thread(move || {
                for span_index in 0..span_count {
                  let _span = tracing::info_span!("span", span_index);
                }
              })
              .thread(move || {
                for span_index in 0..span_count {
                  let _span = tracing::info_span!("span", span_index);
                }
              })
              .run();
            total = add_elapsed(total, elapsed);
          }
          total
        });
      },
    ));
  });
}

/// The `Criterion` group type used by these benchmarks.
type Group<'a> = BenchmarkGroup<'a, WallTime>;

/// Registers a benchmark group for each configured span or event count.
fn bench_thrpt(criterion: &mut Criterion, name: &'static str, mut register: impl FnMut(&mut Group<'_>, &usize)) {
  const N_SPANS: &[(usize, u64)] = &[(1, 1), (10, 10), (50, 50)];

  let mut group = criterion.benchmark_group(name);
  for &(spans, elements) in N_SPANS {
    register_benchmark(group.throughput(Throughput::Elements(elements)));
    register(&mut group, &spans);
  }
  group.finish();
}

/// Consumes Criterion's fluent group return value after registering a benchmark.
const fn register_benchmark(_group: &mut Group<'_>) {}

/// Adds one elapsed worker run to a Criterion custom-iteration total.
const fn add_elapsed(total: Duration, elapsed: Duration) -> Duration {
  total.saturating_add(elapsed)
}

/// Builds a formatting dispatch that writes to `NoWriter`.
fn mk_dispatch() -> tracing::Dispatch {
  let subscriber = tracing_subscriber::FmtSubscriber::builder().with_writer(|| NoWriter).finish();
  tracing::Dispatch::new(subscriber)
}

/// Benchmarks emitting events through a formatting subscriber.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion entry point is referenced through `criterion_group!`"
)]
fn bench_event(criterion: &mut Criterion) {
  bench_thrpt(criterion, "event", |group, event_count| {
    register_root_event_benches(group, *event_count);
    register_unique_parent_event_benches(group, *event_count);
    register_shared_parent_event_bench(group, *event_count);
    register_multi_parent_event_bench(group, *event_count);
  });
}

/// Registers root-event formatting benchmarks.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark registration keeps root event cases grouped"
)]
fn register_root_event_benches(group: &mut Group<'_>, event_count: usize) {
  register_benchmark(group.bench_with_input(
    BenchmarkId::new("root/single_threaded", event_count),
    &event_count,
    |bencher, &count| {
      let dispatch = mk_dispatch();
      with_default(&dispatch, || {
        bencher.iter(|| {
          for event_index in 0..count {
            tracing::info!(event_index);
          }
        });
      });
    },
  ));
  register_benchmark(group.bench_with_input(
    BenchmarkId::new("root/multithreaded", event_count),
    &event_count,
    |bencher, &count| {
      bencher.iter_custom(|iters| {
        let mut total = Duration::ZERO;
        let dispatch = mk_dispatch();
        for _ in 0..iters {
          let bench = MultithreadedBench::new(dispatch.clone());
          let elapsed = bench
            .thread(move || {
              for event_index in 0..count {
                tracing::info!(event_index);
              }
            })
            .thread(move || {
              for event_index in 0..count {
                tracing::info!(event_index);
              }
            })
            .thread(move || {
              for event_index in 0..count {
                tracing::info!(event_index);
              }
            })
            .thread(move || {
              for event_index in 0..count {
                tracing::info!(event_index);
              }
            })
            .run();
          total = add_elapsed(total, elapsed);
        }
        total
      });
    },
  ));
}

/// Registers benchmarks for events under unique parent spans.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark registration keeps unique-parent event cases grouped"
)]
fn register_unique_parent_event_benches(group: &mut Group<'_>, event_count: usize) {
  register_benchmark(group.bench_with_input(
    BenchmarkId::new("unique_parent/single_threaded", event_count),
    &event_count,
    |bencher, &count| {
      with_default(&mk_dispatch(), || {
        let span = tracing::info_span!("unique_parent", foo = false);
        let _guard = span.enter();
        bencher.iter(|| {
          for event_index in 0..count {
            tracing::info!(event_index);
          }
        });
      });
    },
  ));
  register_benchmark(group.bench_with_input(
    BenchmarkId::new("unique_parent/multithreaded", event_count),
    &event_count,
    |bencher, &count| {
      bencher.iter_custom(|iters| {
        let mut total = Duration::ZERO;
        let dispatch = mk_dispatch();
        for _ in 0..iters {
          let bench = MultithreadedBench::new(dispatch.clone());
          let elapsed = bench
            .thread_with_setup(move |start| {
              let span = tracing::info_span!("unique_parent", foo = false);
              let _guard = span.enter();
              let _wait = start.wait();
              emit_events(count);
            })
            .thread_with_setup(move |start| {
              let span = tracing::info_span!("unique_parent", foo = false);
              let _guard = span.enter();
              let _wait = start.wait();
              emit_events(count);
            })
            .thread_with_setup(move |start| {
              let span = tracing::info_span!("unique_parent", foo = false);
              let _guard = span.enter();
              let _wait = start.wait();
              emit_events(count);
            })
            .thread_with_setup(move |start| {
              let span = tracing::info_span!("unique_parent", foo = false);
              let _guard = span.enter();
              let _wait = start.wait();
              emit_events(count);
            })
            .run();
          total = add_elapsed(total, elapsed);
        }
        total
      });
    },
  ));
}

/// Registers the shared-parent multithreaded event benchmark.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark registration keeps the shared-parent event case named"
)]
fn register_shared_parent_event_bench(group: &mut Group<'_>, event_count: usize) {
  register_benchmark(group.bench_with_input(
    BenchmarkId::new("shared_parent/multithreaded", event_count),
    &event_count,
    |bencher, &count| {
      bencher.iter_custom(|iters| {
        let dispatch = mk_dispatch();
        let mut total = Duration::ZERO;
        for _ in 0..iters {
          let parent = with_default(&dispatch, || tracing::info_span!("shared_parent", foo = "hello world"));
          let bench = MultithreadedBench::new(dispatch.clone());
          register_shared_parent_worker(&bench, parent.clone(), count);
          register_shared_parent_worker(&bench, parent.clone(), count);
          register_shared_parent_worker(&bench, parent.clone(), count);
          register_shared_parent_worker(&bench, parent.clone(), count);
          let elapsed = bench.run();
          total = add_elapsed(total, elapsed);
        }
        total
      });
    },
  ));
}

/// Registers one worker for the shared-parent benchmark.
fn register_shared_parent_worker(bench: &MultithreadedBench, parent: tracing::Span, event_count: usize) {
  let _worker = bench.thread_with_setup(move |start| {
    let _guard = parent.enter();
    let _wait = start.wait();
    emit_events(event_count);
  });
}

/// Registers the chained-parent multithreaded event benchmark.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark registration keeps the multi-parent event case named"
)]
fn register_multi_parent_event_bench(group: &mut Group<'_>, event_count: usize) {
  register_benchmark(group.bench_with_input(
    BenchmarkId::new("multi-parent/multithreaded", event_count),
    &event_count,
    |bencher, &count| {
      bencher.iter_custom(|iters| {
        let dispatch = mk_dispatch();
        let mut total = Duration::ZERO;
        for _ in 0..iters {
          let parent = with_default(&dispatch, || tracing::info_span!("multiparent", foo = "hello world"));
          let bench = MultithreadedBench::new(dispatch.clone());
          register_multi_parent_worker(&bench, parent.clone(), count);
          register_multi_parent_worker(&bench, parent.clone(), count);
          register_multi_parent_worker(&bench, parent.clone(), count);
          register_multi_parent_worker(&bench, parent.clone(), count);
          let elapsed = bench.run();
          total = add_elapsed(total, elapsed);
        }
        total
      });
    },
  ));
}

/// Registers one worker for the chained-parent benchmark.
fn register_multi_parent_worker(bench: &MultithreadedBench, parent: tracing::Span, event_count: usize) {
  let _worker = bench.thread_with_setup(move |start| {
    let _guard = parent.enter();
    let _wait = start.wait();
    let mut span = tracing::info_span!("parent");
    for event_index in 0..event_count {
      let next_span = tracing::info_span!(parent: &span, "parent2", event_index, event_count);
      next_span.in_scope(|| {
        tracing::info!(event_index);
      });
      span = next_span;
    }
  });
}

/// Emits the configured number of `info` events.
fn emit_events(event_count: usize) {
  for event_index in 0..event_count {
    tracing::info!(event_index);
  }
}

criterion_group!(benches, bench_new_span, bench_event);
criterion_main!(benches);
