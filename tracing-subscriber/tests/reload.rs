//! Tests reloadable layer behavior.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
  use core::num::NonZeroU64;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use tracing_core::Dispatch;
  use tracing_core::Event;
  use tracing_core::LevelFilter;
  use tracing_core::Metadata;
  use tracing_core::Subscriber;
  use tracing_core::dispatcher::with_default;
  use tracing_core::span::Attributes;
  use tracing_core::span::Id;
  use tracing_core::span::Record;
  use tracing_core::subscriber::Interest;
  use tracing_core::subscriber::SubscriberResult;
  use tracing_subscriber::layer;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::reload::Layer;

  /// Subscriber used by reload tests when only layer behavior is under test.
  #[derive(Copy, Clone, Debug)]
  struct NopSubscriber;

  fn event() {
    tracing::info!("my event");
  }

  impl Subscriber for NopSubscriber {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> SubscriberResult<Interest> {
      Ok(Interest::never())
    }

    fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
      Ok(false)
    }

    fn new_span(&self, _: &Attributes<'_>) -> SubscriberResult<Id> {
      Ok(Id::from_non_zero_u64(NonZeroU64::MIN))
    }

    fn record(&self, _: Id, _: &Record<'_>) -> SubscriberResult {
      Ok(())
    }

    fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult {
      Ok(())
    }

    fn event(&self, _: &Event<'_>) -> SubscriberResult {
      Ok(())
    }

    fn enter(&self, _: Id) -> SubscriberResult {
      Ok(())
    }

    fn exit(&self, _: Id) -> SubscriberResult {
      Ok(())
    }
  }

  /// Running these two tests in parallel will cause flaky failures, since they
  /// both modify the `MAX_LEVEL` value. `cargo test -- --test-threads=1` fixes
  /// it, but runs all tests in serial. The only way to run tests in serial in a
  /// single file is this way.
  #[test]
  fn run_all_reload_test() -> Result<(), TestFailure> {
    reload_handle()?;
    reload_filter()
  }

  #[allow(
    clippy::single_call_fn,
    reason = "reload tests keep serialized subcases named under one max-level-sensitive test"
  )]
  fn reload_handle() -> Result<(), TestFailure> {
    static FILTER1_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FILTER2_CALLS: AtomicUsize = AtomicUsize::new(0);

    enum Filter {
      One,
      Two,
    }

    impl<S: Subscriber> tracing_subscriber::Layer<S> for Filter {
      fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        Ok(Interest::sometimes())
      }

      fn enabled(&self, _metadata: &Metadata<'_>, _: layer::Context<'_, S>) -> SubscriberResult<bool> {
        let filter_calls = match *self {
          Self::One => &FILTER1_CALLS,
          Self::Two => &FILTER2_CALLS,
        };
        let _previous_count = filter_calls.fetch_add(1, Ordering::SeqCst);
        Ok(true)
      }

      fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(match *self {
          Self::One => Some(LevelFilter::INFO),
          Self::Two => Some(LevelFilter::DEBUG),
        })
      }
    }

    let (layer, handle) = Layer::new(Filter::One);

    let subscriber = Dispatch::new(layer.with_subscriber(NopSubscriber));

    with_default(&subscriber, || -> Result<(), TestFailure> {
      ensure_eq(&FILTER1_CALLS.load(Ordering::SeqCst), &0, "first reload layer starts with no calls")?;
      ensure_eq(
        &FILTER2_CALLS.load(Ordering::SeqCst),
        &0,
        "second reload layer starts with no calls",
      )?;

      event();

      ensure_eq(&FILTER1_CALLS.load(Ordering::SeqCst), &1, "first reload layer sees the first event")?;
      ensure_eq(
        &FILTER2_CALLS.load(Ordering::SeqCst),
        &0,
        "second reload layer does not see the first event",
      )?;

      ensure_eq(
        &LevelFilter::current(),
        &LevelFilter::INFO,
        "initial reload layer max level is current",
      )?;
      ensure_ok(handle.reload(Filter::Two), "reload layer swaps to second filter")?;
      ensure_eq(&LevelFilter::current(), &LevelFilter::DEBUG, "reloaded layer max level is current")?;

      event();

      ensure_eq(
        &FILTER1_CALLS.load(Ordering::SeqCst),
        &1,
        "first reload layer call count is unchanged after reload",
      )?;
      ensure_eq(
        &FILTER2_CALLS.load(Ordering::SeqCst),
        &1,
        "second reload layer sees the event after reload",
      )
    })
  }

  #[allow(
    clippy::single_call_fn,
    reason = "reload tests keep serialized subcases named under one max-level-sensitive test"
  )]
  fn reload_filter() -> Result<(), TestFailure> {
    struct NopLayer;
    impl<S: Subscriber> tracing_subscriber::Layer<S> for NopLayer {
      fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        Ok(Interest::sometimes())
      }

      fn enabled(&self, _metadata: &Metadata<'_>, _: layer::Context<'_, S>) -> SubscriberResult<bool> {
        Ok(true)
      }
    }

    static FILTER1_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FILTER2_CALLS: AtomicUsize = AtomicUsize::new(0);

    enum Filter {
      One,
      Two,
    }

    impl<S: Subscriber> layer::Filter<S> for Filter {
      fn enabled(&self, _metadata: &Metadata<'_>, _: &layer::Context<'_, S>) -> SubscriberResult<bool> {
        let filter_calls = match *self {
          Self::One => &FILTER1_CALLS,
          Self::Two => &FILTER2_CALLS,
        };
        let _previous_count = filter_calls.fetch_add(1, Ordering::SeqCst);
        Ok(true)
      }

      fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(match *self {
          Self::One => Some(LevelFilter::INFO),
          Self::Two => Some(LevelFilter::DEBUG),
        })
      }
    }

    let (filter, handle) = Layer::new(Filter::One);

    let dispatcher = Dispatch::new(tracing_subscriber::registry().with(NopLayer.with_filter(filter)));

    with_default(&dispatcher, || -> Result<(), TestFailure> {
      ensure_eq(
        &FILTER1_CALLS.load(Ordering::SeqCst),
        &0,
        "first reload filter starts with no calls",
      )?;
      ensure_eq(
        &FILTER2_CALLS.load(Ordering::SeqCst),
        &0,
        "second reload filter starts with no calls",
      )?;

      event();

      ensure_eq(
        &FILTER1_CALLS.load(Ordering::SeqCst),
        &1,
        "first reload filter sees the first event",
      )?;
      ensure_eq(
        &FILTER2_CALLS.load(Ordering::SeqCst),
        &0,
        "second reload filter does not see the first event",
      )?;

      ensure_eq(
        &LevelFilter::current(),
        &LevelFilter::INFO,
        "initial reload filter max level is current",
      )?;
      ensure_ok(handle.reload(Filter::Two), "reload filter swaps to second filter")?;
      ensure_eq(&LevelFilter::current(), &LevelFilter::DEBUG, "reloaded filter max level is current")?;

      event();

      ensure_eq(
        &FILTER1_CALLS.load(Ordering::SeqCst),
        &1,
        "first reload filter call count is unchanged after reload",
      )?;
      ensure_eq(
        &FILTER2_CALLS.load(Ordering::SeqCst),
        &1,
        "second reload filter sees the event after reload",
      )
    })
  }
}
