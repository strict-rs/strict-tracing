//! Public layer-filter combinator contracts.

#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing::subscriber::set_default;
  use tracing_mock::expect;
  use tracing_mock::layer;
  use tracing_subscriber::filter;
  use tracing_subscriber::filter::FilterExt;
  use tracing_subscriber::filter::LevelFilter;
  use tracing_subscriber::prelude::*;

  #[test]
  fn and_filter_requires_both_sides_to_enable_metadata() -> Result<(), TestFailure> {
    let (mock_layer, handle) = layer::mock()
      .event(expect::event().at_level(tracing::Level::INFO).with_target("and_allowed"))
      .only()
      .run_with_handle();
    let target_filter = filter::filter_fn(|meta| meta.target() == "and_allowed");
    let filter = target_filter.and(LevelFilter::INFO);
    let subscriber = tracing_subscriber::registry().with(mock_layer.with_filter(filter));
    let _guard = set_default(subscriber);

    tracing::info!(target: "and_blocked", "target side rejects");
    tracing::debug!(target: "and_allowed", "level side rejects");
    tracing::info!(target: "and_allowed", "both sides accept");

    ensure_ok(handle.finished(), "and filter emits only when both filters accept")
  }

  #[test]
  fn or_filter_enables_when_either_side_accepts_metadata() -> Result<(), TestFailure> {
    let (mock_layer, handle) = layer::mock()
      .event(expect::event().at_level(tracing::Level::WARN).with_target("or_blocked"))
      .event(expect::event().at_level(tracing::Level::DEBUG).with_target("or_allowed"))
      .only()
      .run_with_handle();
    let target_filter = filter::filter_fn(|meta| meta.target() == "or_allowed");
    let filter = target_filter.or(LevelFilter::WARN);
    let subscriber = tracing_subscriber::registry().with(mock_layer.with_filter(filter));
    let _guard = set_default(subscriber);

    tracing::info!(target: "or_blocked", "neither side accepts");
    tracing::warn!(target: "or_blocked", "level side accepts");
    tracing::debug!(target: "or_allowed", "target side accepts");

    ensure_ok(handle.finished(), "or filter emits when either filter accepts")
  }

  #[test]
  fn not_filter_inverts_metadata_enabled_result() -> Result<(), TestFailure> {
    let (mock_layer, handle) = layer::mock()
      .event(expect::event().at_level(tracing::Level::INFO).with_target("not_allowed"))
      .only()
      .run_with_handle();
    let blocked_target = filter::filter_fn(|meta| meta.target() == "not_blocked");
    let filter = blocked_target.not();
    let subscriber = tracing_subscriber::registry().with(mock_layer.with_filter(filter));
    let _guard = set_default(subscriber);

    tracing::info!(target: "not_blocked", "inner filter accepts so not rejects");
    tracing::info!(target: "not_allowed", "inner filter rejects so not accepts");

    ensure_ok(handle.finished(), "not filter inverts the inner decision")
  }

  #[test]
  fn boxed_filter_preserves_enabled_result() -> Result<(), TestFailure> {
    let (mock_layer, handle) = layer::mock()
      .event(expect::event().at_level(tracing::Level::INFO).with_target("boxed_allowed"))
      .only()
      .run_with_handle();
    let filter = FilterExt::boxed(filter::filter_fn(|meta| meta.target() == "boxed_allowed"));
    let subscriber = tracing_subscriber::registry().with(mock_layer.with_filter(filter));
    let _guard = set_default(subscriber);

    tracing::info!(target: "boxed_blocked", "boxed filter rejects");
    tracing::info!(target: "boxed_allowed", "boxed filter accepts");

    ensure_ok(handle.finished(), "boxed filter preserves the wrapped enabled result")
  }
}
