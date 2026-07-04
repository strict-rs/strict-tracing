#![cfg(feature = "std")]
//! Multiple maximum-level hint coverage.

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::AtomicBool;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::dispatcher::with_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn multiple_max_level_hints() -> Result<(), TestFailure> {
    // This test ensures that when multiple subscribers are active, their max
    // level hints are handled correctly. The global max level should be the
    // maximum of the level filters returned by the two `Subscriber`'s
    // `max_level_hint` method.
    //
    // In this test, we create a subscriber whose max level is `INFO`, and
    // another whose max level is `DEBUG`. We then add an assertion to both of
    // those subscribers' `enabled` method that no metadata for `TRACE` spans or
    // events are filtered, since they are disabled by the global max filter.

    fn do_events() {
      tracing::info!("doing a thing that you might care about");
      tracing::debug!("charging turboencabulator with interocitor");
      tracing::warn!("extremely serious warning, pay attention");
      tracing::trace!("interocitor charge level is 10%");
      tracing::error!("everything is on fire");
    }

    let subscriber1_saw_trace = Arc::new(AtomicBool::new(false));
    let subscriber1_saw_trace_filter = Arc::clone(&subscriber1_saw_trace);
    let (subscriber1, handle1) = subscriber::mock()
      .named("subscriber1")
      .with_max_level_hint(Level::INFO)
      .with_filter(move |meta| {
        let level = meta.level();
        subscriber1_saw_trace_filter.store(level > &Level::DEBUG, Ordering::Relaxed);
        level <= &Level::INFO
      })
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let subscriber2_saw_trace = Arc::new(AtomicBool::new(false));
    let subscriber2_saw_trace_filter = Arc::clone(&subscriber2_saw_trace);
    let (subscriber2, handle2) = subscriber::mock()
      .named("subscriber2")
      .with_max_level_hint(Level::DEBUG)
      .with_filter(move |meta| {
        let level = meta.level();
        subscriber2_saw_trace_filter.store(level > &Level::DEBUG, Ordering::Relaxed);
        level <= &Level::DEBUG
      })
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::DEBUG))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();

    let dispatch1 = tracing::Dispatch::new(subscriber1);

    with_default(&dispatch1, do_events);
    ensure_ok(handle1.finished(), "mock expectations should finish")?;
    ensure(
      !subscriber1_saw_trace.load(Ordering::Relaxed),
      "TRACE metadata should not be dynamically filtered by subscriber1",
    )?;

    let dispatch2 = tracing::Dispatch::new(subscriber2);
    with_default(&dispatch2, do_events);
    ensure_ok(handle2.finished(), "mock expectations should finish")?;
    ensure(
      !subscriber2_saw_trace.load(Ordering::Relaxed),
      "TRACE metadata should not be dynamically filtered by subscriber2",
    )
  }
}
