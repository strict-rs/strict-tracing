//! Tests interest caching for optional filters.

// A separate test crate for `Option<Filter>` for isolation from other tests
// that may influence the interest cache.

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use tracing::subscriber::set_default;
  use tracing_mock::expect;
  use tracing_mock::layer;
  use tracing_subscriber::Layer as _;
  use tracing_subscriber::filter;
  use tracing_subscriber::prelude::*;

  /// A `None` filter should always be interested in events, and it should not
  /// needlessly degrade the caching of other filters.
  #[test]
  fn none_interest_cache() -> Result<(), TestFailure> {
    let (raw_none_layer, handle_none) = layer::mock()
      .event(expect::event())
      .event(expect::event())
      .only()
      .run_with_handle();
    let none_layer = raw_none_layer.with_filter(None::<filter::DynFilterFn<_>>);

    let times_filtered = Arc::new(AtomicUsize::new(0));
    let (raw_filter_fn_layer, handle_filter_fn) = layer::mock()
      .event(expect::event())
      .event(expect::event())
      .only()
      .run_with_handle();
    let filter_counter = Arc::clone(&times_filtered);
    let filter_fn_layer = raw_filter_fn_layer.with_filter(filter::filter_fn(move |_| {
      let _previous_count = filter_counter.fetch_add(1, Ordering::Relaxed);
      true
    }));

    let subscriber = tracing_subscriber::registry().with(none_layer).with(filter_fn_layer);

    let _guard = set_default(subscriber);
    for _ in 0..2 {
      tracing::debug!(target: "always_interesting", x="bar");
    }

    ensure_eq(&times_filtered.load(Ordering::Relaxed), &1, "cached filter function is called once")?;
    ensure_ok(handle_none.finished(), "mock expectations should finish")?;
    ensure_ok(handle_filter_fn.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
