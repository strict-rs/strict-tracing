//! Maximum-level hint coverage.

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::AtomicBool;
  use std::sync::atomic::Ordering;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::subscriber::set_global_default;
  use tracing_mock::*;

  #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
  #[test]
  fn max_level_hints() -> Result<(), TestFailure> {
    // This test asserts that when a subscriber provides us with the global
    // maximum level that it will enable (by implementing the
    // `Subscriber::max_level_hint` method), we will never call
    // `Subscriber::enabled` for events above that maximum level.
    //
    // In this case, we test that by making the `enabled` method assert that no
    // `Metadata` for spans or events at the `TRACE` or `DEBUG` levels.
    let saw_over_hint = Arc::new(AtomicBool::new(false));
    let saw_over_hint_filter = Arc::clone(&saw_over_hint);
    let (subscriber, handle) = subscriber::mock()
      .with_max_level_hint(Level::INFO)
      .with_filter(move |meta| {
        saw_over_hint_filter.store(meta.level() > &Level::INFO, Ordering::Relaxed);
        true
      })
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();

    ensure_ok(set_global_default(subscriber), "global subscriber should install")?;

    tracing::info!("doing a thing that you might care about");
    tracing::debug!("charging turboencabulator with interocitor");
    tracing::warn!("extremely serious warning, pay attention");
    tracing::trace!("interocitor charge level is 10%");
    tracing::error!("everything is on fire");
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    ensure(
      !saw_over_hint.load(Ordering::Relaxed),
      "TRACE and DEBUG metadata should not be dynamically filtered",
    )
  }
}
