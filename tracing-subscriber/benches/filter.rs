//! Benchmarks subscriber filtering paths.

use core::num::NonZeroU64;
use std::error::Error;
use std::fmt;
use std::time::Duration;

use criterion::BenchmarkGroup;
use criterion::Criterion;
use criterion::measurement::WallTime;
use tracing::Event;
use tracing::Id;
use tracing::Metadata;
use tracing::dispatcher::Dispatch;
use tracing::dispatcher::with_default;
use tracing::span;
use tracing_core::subscriber::SubscriberResult;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

/// Benchmark support shared with the formatting benchmarks.
#[path = "support/support.rs"]
pub mod support;
use support::MultithreadedBench;

/// The fixed span ID returned by the no-op benchmark subscriber.
const ENABLED_SPAN_ID: Id = match Id::try_from_u64(0xDEAD_FACE) {
  Some(id) => id,
  None => Id::from_non_zero_u64(NonZeroU64::MIN),
};

/// The `Criterion` group type used by these benchmarks.
type Group<'a> = BenchmarkGroup<'a, WallTime>;

/// A setup error from static benchmark definitions.
#[derive(Debug)]
struct BenchSetupError {
  /// The failing setup operation.
  operation: &'static str,
  /// The input that could not be configured.
  input:     &'static str,
  /// The concrete error message returned by the setup API.
  source:    String,
}

impl fmt::Display for BenchSetupError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "failed to {} `{}`: {}", self.operation, self.input, self.source)
  }
}

impl Error for BenchSetupError {}

/// Parses an `EnvFilter` directive used to define a benchmark.
///
/// # Errors
///
/// Returns an error if the static directive is no longer accepted by
/// `EnvFilter`.
fn parse_filter(directive: &'static str) -> Result<EnvFilter, BenchSetupError> {
  directive.parse::<EnvFilter>().map_err(|source| BenchSetupError {
    operation: "parse EnvFilter directive",
    input:     directive,
    source:    source.to_string(),
  })
}

/// Adds one elapsed worker run to a Criterion custom-iteration total.
const fn add_elapsed(total: Duration, elapsed: Duration) -> Duration {
  total.saturating_add(elapsed)
}

/// Consumes Criterion's fluent group return value after registering a benchmark.
const fn register_benchmark(_group: &mut Group<'_>) {}

/// A subscriber that is enabled but otherwise does nothing.
struct EnabledSubscriber;

impl tracing::Subscriber for EnabledSubscriber {
  fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<Id> {
    Ok(ENABLED_SPAN_ID)
  }

  fn event(&self, _event: &Event<'_>) -> SubscriberResult {
    Ok(())
  }

  fn record(&self, _span: Id, _values: &span::Record<'_>) -> SubscriberResult {
    Ok(())
  }

  fn record_follows_from(&self, _span: Id, _follows: Id) -> SubscriberResult {
    Ok(())
  }

  fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
    Ok(true)
  }

  fn enter(&self, _span: Id) -> SubscriberResult {
    Ok(())
  }

  fn exit(&self, _span: Id) -> SubscriberResult {
    Ok(())
  }
}

/// Benchmarks static target-and-level filters.
///
/// # Errors
///
/// Returns an error if any static `EnvFilter` directive used by the benchmark no
/// longer parses.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark group function preserves the static filter benchmark matrix"
)]
fn bench_static(criterion: &mut Criterion) -> Result<(), BenchSetupError> {
  {
    let mut group = criterion.benchmark_group("static");
    bench_static_single_threaded(&mut group)?;
    bench_static_multithreaded(&mut group)?;
    group.finish();
  };
  Ok(())
}

/// Registers single-threaded static filter benchmarks.
///
/// # Errors
///
/// Returns an error if any static `EnvFilter` directive used by the benchmark no
/// longer parses.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark registration keeps single-threaded static cases grouped"
)]
fn bench_static_single_threaded(group: &mut Group<'_>) -> Result<(), BenchSetupError> {
  let baseline = Dispatch::new(EnabledSubscriber);
  register_benchmark(group.bench_function("baseline_single_threaded", |bencher| {
    with_default(&baseline, || {
      bencher.iter(|| {
        tracing::info!(target: "static_filter", "hi");
        tracing::debug!(target: "static_filter", "hi");
        tracing::warn!(target: "static_filter", "hi");
        tracing::trace!(target: "foo", "hi");
      });
    });
  }));

  let single_threaded = Dispatch::new(EnabledSubscriber.with(parse_filter("static_filter=info")?));
  register_benchmark(group.bench_function("single_threaded", |bencher| {
    with_default(&single_threaded, || {
      bencher.iter(|| {
        tracing::info!(target: "static_filter", "hi");
        tracing::debug!(target: "static_filter", "hi");
        tracing::warn!(target: "static_filter", "hi");
        tracing::trace!(target: "foo", "hi");
      });
    });
  }));

  let enabled_one = Dispatch::new(EnabledSubscriber.with(parse_filter("static_filter=info")?));
  register_benchmark(group.bench_function("enabled_one", |bencher| {
    with_default(&enabled_one, || {
      bencher.iter(|| {
        tracing::info!(target: "static_filter", "hi");
      });
    });
  }));

  let enabled_many = Dispatch::new(EnabledSubscriber.with(parse_filter("foo=debug,bar=trace,baz=error,quux=warn,static_filter=info")?));
  register_benchmark(group.bench_function("enabled_many", |bencher| {
    with_default(&enabled_many, || {
      bencher.iter(|| {
        tracing::info!(target: "static_filter", "hi");
      });
    });
  }));

  let disabled_level_one = Dispatch::new(EnabledSubscriber.with(parse_filter("static_filter=info")?));
  register_benchmark(group.bench_function("disabled_level_one", |bencher| {
    with_default(&disabled_level_one, || {
      bencher.iter(|| {
        tracing::debug!(target: "static_filter", "hi");
      });
    });
  }));

  let disabled_level_many =
    Dispatch::new(EnabledSubscriber.with(parse_filter("foo=debug,bar=info,baz=error,quux=warn,static_filter=info")?));
  register_benchmark(group.bench_function("disabled_level_many", |bencher| {
    with_default(&disabled_level_many, || {
      bencher.iter(|| {
        tracing::trace!(target: "static_filter", "hi");
      });
    });
  }));

  let disabled_one = Dispatch::new(EnabledSubscriber.with(parse_filter("foo=info")?));
  register_benchmark(group.bench_function("disabled_one", |bencher| {
    with_default(&disabled_one, || {
      bencher.iter(|| {
        tracing::info!(target: "static_filter", "hi");
      });
    });
  }));

  let disabled_many = Dispatch::new(EnabledSubscriber.with(parse_filter("foo=debug,bar=trace,baz=error,quux=warn,whibble=info")?));
  register_benchmark(group.bench_function("disabled_many", |bencher| {
    with_default(&disabled_many, || {
      bencher.iter(|| {
        tracing::info!(target: "static_filter", "hi");
      });
    });
  }));

  Ok(())
}

/// Registers multithreaded static filter benchmarks.
///
/// # Errors
///
/// Returns an error if any static `EnvFilter` directive used by the benchmark no
/// longer parses.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark registration keeps multithreaded static cases grouped"
)]
fn bench_static_multithreaded(group: &mut Group<'_>) -> Result<(), BenchSetupError> {
  register_benchmark(group.bench_function("baseline_multithreaded", |bencher| {
    let dispatch = Dispatch::new(EnabledSubscriber);
    bencher.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      for _ in 0..iters {
        let bench = MultithreadedBench::new(dispatch.clone());
        let elapsed = bench
          .thread(|| {
            tracing::info!(target: "static_filter", "hi");
          })
          .thread(|| {
            tracing::debug!(target: "static_filter", "hi");
          })
          .thread(|| {
            tracing::warn!(target: "static_filter", "hi");
          })
          .thread(|| {
            tracing::warn!(target: "foo", "hi");
          })
          .run();
        total = add_elapsed(total, elapsed);
      }
      total
    });
  }));

  let dispatch = Dispatch::new(EnabledSubscriber.with(parse_filter("static_filter=info")?));
  register_benchmark(group.bench_function("multithreaded", |bencher| {
    bencher.iter_custom(|iters| {
      let mut total = Duration::ZERO;
      for _ in 0..iters {
        let bench = MultithreadedBench::new(dispatch.clone());
        let elapsed = bench
          .thread(|| {
            tracing::info!(target: "static_filter", "hi");
          })
          .thread(|| {
            tracing::debug!(target: "static_filter", "hi");
          })
          .thread(|| {
            tracing::warn!(target: "static_filter", "hi");
          })
          .thread(|| {
            tracing::warn!(target: "foo", "hi");
          })
          .run();
        total = add_elapsed(total, elapsed);
      }
      total
    });
  }));

  Ok(())
}

/// Benchmarks dynamic span-context filters.
///
/// # Errors
///
/// Returns an error if any static `EnvFilter` directive used by the benchmark no
/// longer parses.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark group function preserves the dynamic filter benchmark matrix"
)]
fn bench_dynamic(criterion: &mut Criterion) -> Result<(), BenchSetupError> {
  {
    let mut group = criterion.benchmark_group("dynamic");
    let baseline = Dispatch::new(EnabledSubscriber);
    register_benchmark(group.bench_function("baseline_single_threaded", |bencher| {
      with_default(&baseline, || {
        bencher.iter(|| {
          tracing::info_span!("foo").in_scope(|| {
            tracing::info!("hi");
            tracing::debug!("hi");
          });
          tracing::info_span!("bar").in_scope(|| {
            tracing::warn!("hi");
          });
          tracing::trace!("hi");
        });
      });
    }));

    let single_threaded = Dispatch::new(EnabledSubscriber.with(parse_filter("[foo]=trace")?));
    register_benchmark(group.bench_function("single_threaded", |bencher| {
      with_default(&single_threaded, || {
        bencher.iter(|| {
          tracing::info_span!("foo").in_scope(|| {
            tracing::info!("hi");
            tracing::debug!("hi");
          });
          tracing::info_span!("bar").in_scope(|| {
            tracing::warn!("hi");
          });
          tracing::trace!("hi");
        });
      });
    }));
    register_benchmark(group.bench_function("baseline_multithreaded", |bencher| {
      let dispatch = Dispatch::new(EnabledSubscriber);
      bencher.iter_custom(|iters| {
        let mut total = Duration::ZERO;
        for _ in 0..iters {
          let bench = MultithreadedBench::new(dispatch.clone());
          let elapsed = bench
            .thread(|| {
              let span = tracing::info_span!("foo");
              let _entered = span.enter();
              tracing::info!("hi");
            })
            .thread(|| {
              let span = tracing::info_span!("foo");
              let _entered = span.enter();
              tracing::debug!("hi");
            })
            .thread(|| {
              let span = tracing::info_span!("bar");
              let _entered = span.enter();
              tracing::debug!("hi");
            })
            .thread(|| {
              tracing::trace!("hi");
            })
            .run();
          total = add_elapsed(total, elapsed);
        }
        total
      });
    }));

    let dispatch = Dispatch::new(EnabledSubscriber.with(parse_filter("[foo]=trace")?));
    register_benchmark(group.bench_function("multithreaded", |bencher| {
      bencher.iter_custom(|iters| {
        let mut total = Duration::ZERO;
        for _ in 0..iters {
          let bench = MultithreadedBench::new(dispatch.clone());
          let elapsed = bench
            .thread(|| {
              let span = tracing::info_span!("foo");
              let _entered = span.enter();
              tracing::info!("hi");
            })
            .thread(|| {
              let span = tracing::info_span!("foo");
              let _entered = span.enter();
              tracing::debug!("hi");
            })
            .thread(|| {
              let span = tracing::info_span!("bar");
              let _entered = span.enter();
              tracing::debug!("hi");
            })
            .thread(|| {
              tracing::trace!("hi");
            })
            .run();
          total = add_elapsed(total, elapsed);
        }
        total
      });
    }));

    group.finish();
  };
  Ok(())
}

/// Benchmarks filters with mixed static and dynamic directives.
///
/// # Errors
///
/// Returns an error if any static `EnvFilter` directive used by the benchmark no
/// longer parses.
#[allow(
  clippy::single_call_fn,
  reason = "Criterion benchmark group function preserves the mixed filter benchmark matrix"
)]
fn bench_mixed(criterion: &mut Criterion) -> Result<(), BenchSetupError> {
  {
    let mut group = criterion.benchmark_group("mixed");
    let disabled = Dispatch::new(EnabledSubscriber.with(parse_filter("[foo]=trace,bar[quux]=debug,[{baz}]=debug,asdf=warn,wibble=info")?));
    register_benchmark(group.bench_function("disabled", |bencher| {
      with_default(&disabled, || {
        bencher.iter(|| {
          tracing::info!(target: "static_filter", "hi");
        });
      });
    }));

    let disabled_by_level = Dispatch::new(EnabledSubscriber.with(parse_filter("[foo]=info,bar[quux]=debug,asdf=warn,static_filter=info")?));
    register_benchmark(group.bench_function("disabled_by_level", |bencher| {
      with_default(&disabled_by_level, || {
        bencher.iter(|| {
          tracing::trace!(target: "static_filter", "hi");
        });
      });
    }));
    group.finish();
  };
  Ok(())
}

/// Runs the filter benchmarks.
///
/// # Errors
///
/// Returns an error if any static benchmark setup no longer succeeds.
fn main() -> Result<(), BenchSetupError> {
  {
    let mut criterion = Criterion::default().configure_from_args();
    bench_static(&mut criterion)?;
    bench_dynamic(&mut criterion)?;
    bench_mixed(&mut criterion)?;
    criterion.final_summary();
  };
  Ok(())
}
