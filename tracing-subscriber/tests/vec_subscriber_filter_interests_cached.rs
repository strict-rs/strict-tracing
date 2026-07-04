//! Tests interest caching for vector subscriber layers.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::sync::Arc;

  use parking_lot::Mutex;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing::Subscriber as _;
  use tracing::subscriber::set_default;
  use tracing_mock::expect;
  use tracing_mock::layer;
  use tracing_mock::layer::MockLayer;
  use tracing_subscriber::filter;
  use tracing_subscriber::prelude::*;

  type SeenLevels = Arc<Mutex<BTreeMap<Level, usize>>>;

  fn events() {
    tracing::trace!("hello trace");
    tracing::debug!("hello debug");
    tracing::info!("hello info");
    tracing::warn!("hello warn");
    tracing::error!("hello error");
  }

  #[allow(
    clippy::single_call_fn,
    reason = "interest-cache tests isolate locked counter mutation from filter assertions"
  )]
  fn increment_seen_level(seen: &SeenLevels, level: Level) {
    let mut seen_levels = seen.lock();
    let count = seen_levels.entry(level).or_insert(0);
    *count = count.saturating_add(1);
    drop(seen_levels);
  }

  fn ensure_cached_counts(seen: &SeenLevels, context: &'static str) -> Result<(), TestFailure> {
    let seen_levels = seen.lock();
    ensure(
      seen_levels
        .iter()
        .filter(|entry| *entry.0 != Level::INFO)
        .all(|entry| *entry.1 == 1),
      context,
    )
  }

  #[test]
  fn vec_layer_filter_interests_are_cached() -> Result<(), TestFailure> {
    let filtered_layer = |level: Level, subscriber: MockLayer| {
      let seen = Arc::new(Mutex::new(BTreeMap::new()));
      let seen_filter = Arc::clone(&seen);
      let filter = filter::filter_fn(move |meta| {
        increment_seen_level(&seen_filter, *meta.level());
        meta.level() <= &level
      });
      (subscriber.with_filter(filter).boxed(), seen)
    };

    let (raw_info_layer, info_handle) = layer::named("info")
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let (info_layer, seen_info) = filtered_layer(Level::INFO, raw_info_layer);

    let (raw_warn_layer, warn_handle) = layer::named("warn")
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();
    let (warn_layer, seen_warn) = filtered_layer(Level::WARN, raw_warn_layer);

    let subscriber = tracing_subscriber::registry().with(vec![warn_layer, info_layer]);
    ensure(
      subscriber.max_level_hint().is_none(),
      "vector dynamic filters do not provide a max level hint",
    )?;

    let _subscriber = set_default(subscriber);

    events();
    ensure_cached_counts(&seen_info, "INFO subscriber interests are cached after first event set")?;
    ensure_cached_counts(&seen_warn, "WARN subscriber interests are cached after first event set")?;

    events();
    ensure_cached_counts(&seen_info, "INFO subscriber interests remain cached after second event set")?;
    ensure_cached_counts(&seen_warn, "WARN subscriber interests remain cached after second event set")?;

    ensure_ok(info_handle.finished(), "mock expectations should finish")?;
    ensure_ok(warn_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
