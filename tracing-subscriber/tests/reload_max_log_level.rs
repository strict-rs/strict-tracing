//! Tests reloadable filters updating log max levels.
#![cfg(all(feature = "env-filter", feature = "tracing-log"))]

#[cfg(test)]
mod tests {

  use tracing_core::subscriber::SubscriberError;
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
    ResultTracingSubscriberReloadReloadError(#[from] strict_test_support::ResultFailure<reload::ReloadError>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTracingSubscriberUtilTryInitError(#[from] strict_test_support::ResultFailure<util::TryInitError>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Level;
  use tracing_mock::expect;
  use tracing_mock::subscriber;
  use tracing_subscriber::filter::LevelFilter;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::reload;

  #[test]
  fn reload_max_log_level() -> Result<(), TestError> {
    let (subscriber, mock_handle) = subscriber::mock()
      .event(expect::event().at_level(Level::INFO))
      .event(expect::event().at_level(Level::DEBUG))
      .event(expect::event().at_level(Level::INFO))
      .only()
      .run_with_handle();
    let (filter, reload_handle) = reload::Layer::new(LevelFilter::INFO);
    ensure_ok(subscriber.with(filter).try_init(), "subscriber installs")?;

    ensure(log::log_enabled!(log::Level::Info), "info logs start enabled").map(drop)?;
    ensure(!log::log_enabled!(log::Level::Debug), "debug logs start disabled").map(drop)?;
    ensure(!log::log_enabled!(log::Level::Trace), "trace logs start disabled").map(drop)?;

    log::debug!("i'm disabled");
    log::info!("i'm enabled");

    ensure_ok(reload_handle.reload(Level::DEBUG), "reloading succeeds")?;

    ensure(log::log_enabled!(log::Level::Info), "info logs stay enabled").map(drop)?;
    ensure(log::log_enabled!(log::Level::Debug), "debug logs become enabled").map(drop)?;
    ensure(!log::log_enabled!(log::Level::Trace), "trace logs stay disabled").map(drop)?;

    log::debug!("i'm enabled now");
    log::info!("i'm still enabled, too");

    ensure_ok(mock_handle.finished(), "mock expectations should finish")?;
    Ok(())
  }
}
