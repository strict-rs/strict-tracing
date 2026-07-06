//! Benchmarks for synchronous and non-blocking appenders.

use std::io;
use std::io::Write;
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use tracing::Level;
use tracing::event;
use tracing::subscriber::with_default;
use tracing_appender::non_blocking;
use tracing_subscriber::fmt::MakeWriter;

// a no-op writer is used in order to measure the overhead incurred by
// tracing-subscriber.
#[derive(Clone)]
/// A writer that accepts all bytes without storing them.
struct NoOpWriter;

impl NoOpWriter {
  /// Creates a writer for tracing overhead benchmarks.
  const fn new() -> Self {
    Self
  }
}

impl MakeWriter<'_> for NoOpWriter {
  type Writer = Self;

  fn make_writer(&self) -> Self::Writer {
    self.clone()
  }
}

impl Write for NoOpWriter {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    Ok(buf.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

/// Emits the measured benchmark event `iters` times.
fn record_events(iters: u64) {
  for _ in 0..iters {
    event!(Level::INFO, "event");
  }
}

/// Registers synchronous writer benchmarks.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion macro registration requires named benchmark functions"
)]
fn synchronous_benchmark(criterion: &mut Criterion) {
  let mut group = criterion.benchmark_group("synchronous");
  let _single_thread_benchmark = group.bench_function("single_thread", |bencher| {
    let subscriber = tracing_subscriber::fmt().with_writer(NoOpWriter::new());
    with_default(subscriber.finish(), || {
      bencher.iter(|| event!(Level::INFO, "event"));
    });
  });

  let _multiple_writers_benchmark = group.bench_function("multiple_writers", |bencher| {
    bencher.iter_custom(|iters| {
      let mut handles: Vec<JoinHandle<()>> = Vec::new();

      let start = Instant::now();

      let make_writer = NoOpWriter::new();
      let cloned_make_writer = make_writer.clone();

      handles.push(thread::spawn(move || {
        let subscriber = tracing_subscriber::fmt().with_writer(make_writer);
        with_default(subscriber.finish(), || record_events(iters));
      }));

      handles.push(thread::spawn(move || {
        let subscriber = tracing_subscriber::fmt().with_writer(cloned_make_writer);
        with_default(subscriber.finish(), || record_events(iters));
      }));

      for handle in handles {
        let _join_result = handle.join();
      }

      start.elapsed()
    });
  });
}

/// Registers non-blocking writer benchmarks.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion macro registration requires named benchmark functions"
)]
fn non_blocking_benchmark(criterion: &mut Criterion) {
  let mut group = criterion.benchmark_group("non_blocking");

  let _single_thread_benchmark = group.bench_function("single_thread", |bencher| {
    let (non_blocking, _guard) = non_blocking(NoOpWriter::new());
    let subscriber = tracing_subscriber::fmt().with_writer(non_blocking);

    with_default(subscriber.finish(), || {
      bencher.iter(|| event!(Level::INFO, "event"));
    });
  });

  let _multiple_writers_benchmark = group.bench_function("multiple_writers", |bencher| {
    bencher.iter_custom(|iters| {
      let (non_blocking, _guard) = non_blocking(NoOpWriter::new());

      let mut handles: Vec<JoinHandle<()>> = Vec::new();

      let start = Instant::now();

      let cloned_make_writer = non_blocking.clone();

      handles.push(thread::spawn(move || {
        let subscriber = tracing_subscriber::fmt().with_writer(non_blocking);
        with_default(subscriber.finish(), || record_events(iters));
      }));

      handles.push(thread::spawn(move || {
        let subscriber = tracing_subscriber::fmt().with_writer(cloned_make_writer);
        with_default(subscriber.finish(), || record_events(iters));
      }));

      for handle in handles {
        let _join_result = handle.join();
      }

      start.elapsed()
    });
  });
}

criterion_group!(benches, synchronous_benchmark, non_blocking_benchmark);
criterion_main!(benches);
