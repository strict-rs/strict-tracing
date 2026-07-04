//! Tests subscriber initialization utilities.
#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing_mock::*;
  use tracing_subscriber::EnvFilter;
  use tracing_subscriber::fmt;
  use tracing_subscriber::fmt::layer as fmt_layer;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::registry;

  // This test target owns `SubscriberInitExt` coverage, including the
  // `tracing-log` side effect. Tests whose subject is filtering or mock ordering
  // use `tracing::subscriber::set_default` so this process-global logger state
  // does not become part of their expected event stream.

  #[test]
  fn init_ext_works() -> Result<(), TestFailure> {
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(tracing::Level::INFO).with_target("init_works"))
      .run_with_handle();

    let _guard = subscriber.set_default();
    tracing::info!(target: "init_works", "it worked!");
    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  #[cfg(feature = "tracing-log")]
  fn set_default_initializes_log_tracer() -> Result<(), TestFailure> {
    let (subscriber, mock_handle) = subscriber::mock()
      .event(
        expect::event()
          .at_level(tracing::Level::INFO)
          .with_target("log")
          .with_fields(expect::msg("it worked through log!").and(expect::field("log.target").with_value(&"init_ext_log_bridge"))),
      )
      .only()
      .run_with_handle();

    let _guard = subscriber.set_default();
    log::info!(target: "init_ext_log_bridge", "it worked through log!");
    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }

  #[test]
  #[cfg(feature = "fmt")]
  fn builders_are_init_ext() {
    let _guard = fmt().set_default();
    let _result = fmt().with_target(false).compact().try_init();
  }

  #[test]
  #[cfg(all(feature = "fmt", feature = "env-filter"))]
  fn layered_is_init_ext() {
    let _guard = registry().with(fmt_layer()).with(EnvFilter::new("foo=info")).set_default();
  }
}
