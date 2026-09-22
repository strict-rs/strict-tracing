//! Tests log compatibility with subscriber filters.
#![cfg(all(feature = "env-filter", feature = "tracing-log"))]

#[cfg(test)]
mod tests {

  use tracing_core::subscriber::SubscriberError;
  use tracing_subscriber::filter;
  use tracing_subscriber::util;
  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultSubscriberError(#[from] strict_test_support::ResultFailure<SubscriberError>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTracingSubscriberUtilTryInitError(#[from] strict_test_support::ResultFailure<util::TryInitError>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTracingSubscriberParseError(#[from] strict_test_support::ResultFailure<filter::ParseError>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing_mock::*;
  use tracing_subscriber::filter::EnvFilter;
  use tracing_subscriber::prelude::*;

  mod my_module {
    use super::TestError;
    use super::ensure;

    #[allow(
      clippy::single_call_fn,
      reason = "log filter tests keep records in a nested module to exercise module-target filtering"
    )]
    pub(super) fn test_records() {
      log::trace!("this should be disabled");
      log::info!("this shouldn't be");
      log::debug!("this should be disabled");
      log::warn!("this should be enabled");
      log::warn!(target: "something else", "this shouldn't be enabled");
      log::error!("this should be enabled too");
    }

    #[allow(
      clippy::single_call_fn,
      reason = "log filter tests keep enabled checks in a nested module to exercise module-target filtering"
    )]
    pub(super) fn test_log_enabled() -> Result<(), TestError> {
      ensure(log::log_enabled!(log::Level::Info), "info is enabled inside `my_module`").map(drop)?;
      ensure(!log::log_enabled!(log::Level::Debug), "debug is disabled inside `my_module`").map(drop)?;
      ensure(log::log_enabled!(log::Level::Warn), "warn is enabled inside `my_module`")
        .map(drop)
        .map_err(TestError::from)
    }
  }

  #[test]
  fn log_is_enabled() -> Result<(), TestError> {
    let filter: EnvFilter = ensure_ok("filter_log::tests::my_module=info".parse(), "log compatibility filter parses")?;
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::WARN))
      .event(expect::event().at_level(Level::ERROR))
      .only()
      .run_with_handle();

    // Note: we have to set the global default in order to set the `log` max
    // level, which can only be set once.
    ensure_ok(subscriber.with(filter).try_init(), "subscriber installs")?;

    my_module::test_records();
    log::info!("this is disabled");

    my_module::test_log_enabled()?;
    ensure(!log::log_enabled!(log::Level::Info), "info is disabled outside `my_module`").map(drop)?;
    ensure(!log::log_enabled!(log::Level::Warn), "warn is disabled outside `my_module`").map(drop)?;

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
