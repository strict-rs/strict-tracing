//! Tests vector-backed layer composition.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Subscriber as _;
  use tracing::level_filters::LevelFilter;
  use tracing::subscriber::with_default;
  use tracing_mock::layer::named;
  use tracing_subscriber::prelude::*;

  #[test]
  fn just_empty_vec() -> Result<(), TestFailure> {
    // Just a None means everything is off
    let subscriber = tracing_subscriber::registry().with(Vec::<LevelFilter>::new());
    ensure(
      subscriber.max_level_hint() == Some(LevelFilter::OFF),
      "empty vector disables all levels",
    )
  }

  #[test]
  fn layer_and_empty_vec() -> Result<(), TestFailure> {
    let subscriber = tracing_subscriber::registry()
      .with(LevelFilter::INFO)
      .with(Vec::<LevelFilter>::new());
    ensure(
      subscriber.max_level_hint() == Some(LevelFilter::INFO),
      "empty vector preserves previous layer max level",
    )
  }

  #[test]
  fn on_register_dispatch_is_called() -> Result<(), TestFailure> {
    let (inner_layer_0, inner_handle_0) = named("inner0").on_register_dispatch().run_with_handle();
    let (inner_layer_1, inner_handle_1) = named("inner0").on_register_dispatch().run_with_handle();

    let subscriber = tracing_subscriber::registry().with(vec![inner_layer_0, inner_layer_1]);
    with_default(subscriber, || {});

    ensure_ok(inner_handle_0.finished(), "mock expectations should finish")?;
    ensure_ok(inner_handle_1.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
