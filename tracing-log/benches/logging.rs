//! Benchmarks for log-to-tracing forwarding.

use std::fmt;
use std::io;
use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use log::SetLoggerError;
use log::trace;
use tracing::subscriber;
use tracing::subscriber::SetGlobalDefaultError;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::FmtSubscriber;
use tracing_subscriber::filter::ParseError;

/// Number of threads used by the multi-threaded log forwarding benchmark.
const THREAD_COUNT: usize = 8;
/// `THREAD_COUNT` represented as `u128` for checked duration averaging.
const THREAD_COUNT_U128: u128 = 8;
/// Non-zero timing returned to `Criterion` when an iteration cannot be measured.
const FAILURE_FALLBACK_DURATION: Duration = Duration::from_secs(1);

/// Error returned when benchmark-wide global setup cannot be completed.
enum BenchmarkSetupError {
  /// The hard-coded `EnvFilter` directive string could not be parsed.
  EnvFilter(ParseError),
  /// The process-global `LogTracer` could not be installed.
  LogTracer(SetLoggerError),
  /// The process-global tracing subscriber could not be installed.
  GlobalSubscriber(SetGlobalDefaultError),
}

impl fmt::Display for BenchmarkSetupError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::EnvFilter(ref error) => {
        write!(formatter, "failed to parse `EnvFilter`: {error}")
      }
      Self::LogTracer(ref error) => {
        write!(formatter, "failed to install `LogTracer`: {error}")
      }
      Self::GlobalSubscriber(ref error) => {
        write!(formatter, "failed to install tracing subscriber: {error}")
      }
    }
  }
}

/// Failure mode that prevents a benchmark iteration from producing a real time.
#[derive(Clone, Copy)]
enum BenchmarkTimingFailure {
  /// At least one worker thread panicked before it could be joined.
  WorkerPanicked,
  /// The total worker duration could not be divided by the worker count.
  AverageDurationUnavailable,
  /// The average duration could not be represented as nanoseconds.
  AverageDurationOverflow,
}

impl fmt::Display for BenchmarkTimingFailure {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::WorkerPanicked => formatter.write_str("worker thread panicked"),
      Self::AverageDurationUnavailable => formatter.write_str("average duration could not be computed"),
      Self::AverageDurationOverflow => formatter.write_str("average duration overflowed nanosecond storage"),
    }
  }
}

/// Write a benchmark diagnostic to locked standard error.
fn report_benchmark_failure(message: fmt::Arguments<'_>) {
  let standard_error = io::stderr();
  let mut output = standard_error.lock();
  match writeln!(output, "tracing-log benchmark `log_from_multiple_threads`: {message}") {
    Ok(()) => {}
    Err(error) => drop(error),
  }
}

/// Prepare process-global benchmark state before registering `Criterion` work.
#[allow(
  clippy::single_call_fn,
  reason = "typed benchmark setup separates fallible global initialization from Criterion registration"
)]
fn prepare_benchmark() -> Result<(), BenchmarkSetupError> {
  let env_filter =
    EnvFilter::try_new("info,ws=off,yamux=off,regalloc=off,cranelift_codegen=off,cranelift_wasm=warn,hyper=warn,dummy=trace")
      .map_err(BenchmarkSetupError::EnvFilter)?;

  #[cfg(feature = "interest-cache")]
  let log_tracer_init = tracing_log::LogTracer::builder()
    .with_max_level(log::LevelFilter::Trace)
    .with_interest_cache(tracing_log::InterestCacheConfig::default())
    .init();
  #[cfg(not(feature = "interest-cache"))]
  let log_tracer_init = tracing_log::LogTracer::builder().with_max_level(log::LevelFilter::Trace).init();
  log_tracer_init.map_err(BenchmarkSetupError::LogTracer)?;

  let subscriber_builder = FmtSubscriber::builder().with_env_filter(env_filter).with_filter_reloading();
  let tracing_subscriber = subscriber_builder.finish();
  subscriber::set_global_default(tracing_subscriber).map_err(BenchmarkSetupError::GlobalSubscriber)
}

/// Run the traced logging workload on several threads after every worker reaches a spin barrier.
#[allow(
  clippy::single_call_fn,
  reason = "benchmark helper isolates worker barrier and join error handling from timing closure"
)]
fn run_on_many_threads(thread_count: usize, count: u64) -> Result<Vec<Duration>, BenchmarkTimingFailure> {
  let started_count = Arc::new(AtomicUsize::new(0));
  let barrier = Arc::new(AtomicBool::new(false));
  let mut threads: Vec<JoinHandle<Duration>> = Vec::with_capacity(thread_count);
  for _ in 0..thread_count {
    let thread_started_count = Arc::clone(&started_count);
    let thread_barrier = Arc::clone(&barrier);

    threads.push(thread::spawn(move || {
      let _previous = thread_started_count.fetch_add(1, Ordering::SeqCst);
      while !thread_barrier.load(Ordering::SeqCst) {
        thread::yield_now();
      }

      let start = Instant::now();
      for _ in 0..count {
        trace!("A dummy log!");
      }
      start.elapsed()
    }));
  }

  while started_count.load(Ordering::SeqCst) != thread_count {
    thread::yield_now();
  }
  barrier.store(true, Ordering::SeqCst);

  threads
    .into_iter()
    .map(JoinHandle::join)
    .collect::<Result<Vec<Duration>, _>>()
    .map_err(|panic_payload| {
      drop(panic_payload);
      BenchmarkTimingFailure::WorkerPanicked
    })
}

/// Benchmark `log` records forwarded through `tracing-log` from several threads.
#[allow(
  clippy::single_call_fn,
  reason = "criterion_group registers this benchmark entry point by name"
)]
fn bench_logger(criterion: &mut Criterion) {
  if let Err(error) = prepare_benchmark() {
    report_benchmark_failure(format_args!("skipping benchmark registration after setup failure: {error}"));
    return;
  }

  let _benchmark = criterion.bench_function("log_from_multiple_threads", |bencher| {
    bencher.iter_custom(|count| {
      run_on_many_threads(THREAD_COUNT, count)
        .and_then(|durations| {
          let total_time: Duration = durations.into_iter().sum();
          let average_nanos = total_time
            .as_nanos()
            .checked_div(THREAD_COUNT_U128)
            .ok_or(BenchmarkTimingFailure::AverageDurationUnavailable)?;
          let duration_nanos = u64::try_from(average_nanos).map_err(|_error| BenchmarkTimingFailure::AverageDurationOverflow)?;
          Ok(Duration::from_nanos(duration_nanos))
        })
        .inspect_err(|failure| {
          report_benchmark_failure(format_args!(
            "returning conservative fallback duration after timing failure: {failure}"
          ));
        })
        .unwrap_or(FAILURE_FALLBACK_DURATION)
    });
  });
}

criterion_group!(benches, bench_logger);
criterion_main!(benches);
