//! Tests vector-backed layer composition.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {

  use tracing_core::subscriber::SubscriberError;
  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultSubscriberError(#[from] strict_test_support::ResultFailure<SubscriberError>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Subscriber as _;
  use tracing::level_filters::LevelFilter;
  use tracing::subscriber::with_default;
  use tracing_mock::layer::named;
  use tracing_subscriber::prelude::*;

  #[test]
  fn just_empty_vec() -> Result<(), TestError> {
    // Just a None means everything is off
    let subscriber = tracing_subscriber::registry().with(Vec::<LevelFilter>::new());
    ensure(
      subscriber.max_level_hint() == Some(LevelFilter::OFF),
      "empty vector disables all levels",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn layer_and_empty_vec() -> Result<(), TestError> {
    let subscriber = tracing_subscriber::registry()
      .with(LevelFilter::INFO)
      .with(Vec::<LevelFilter>::new());
    ensure(
      subscriber.max_level_hint() == Some(LevelFilter::INFO),
      "empty vector preserves previous layer max level",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn on_register_dispatch_is_called() -> Result<(), TestError> {
    let (inner_layer_0, inner_handle_0) = named("inner0").on_register_dispatch().run_with_handle();
    let (inner_layer_1, inner_handle_1) = named("inner0").on_register_dispatch().run_with_handle();

    let subscriber = tracing_subscriber::registry().with(vec![inner_layer_0, inner_layer_1]);
    with_default(subscriber, || {});

    ensure_ok(inner_handle_0.finished(), "mock expectations should finish")?;
    ensure_ok(inner_handle_1.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
