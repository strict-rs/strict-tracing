//! Tests registry max level hints.
#![cfg(all(feature = "registry", feature = "fmt"))]

#[cfg(test)]
mod tests {

  use tracing_subscriber::util;
  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonLevelFilter(#[from] strict_test_support::ComparisonFailure<LevelFilter, LevelFilter>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTracingSubscriberUtilTryInitError(#[from] strict_test_support::ResultFailure<util::TryInitError>),
  }

  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use tracing_subscriber::filter::LevelFilter;
  use tracing_subscriber::fmt;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::registry;

  #[test]
  fn registry_sets_max_level_hint() -> Result<(), TestError> {
    ensure_ok(
      registry().with(fmt::layer()).with(LevelFilter::DEBUG).try_init(),
      "registry installs",
    )?;
    ensure_eq(
      LevelFilter::current(),
      LevelFilter::DEBUG,
      "registry init updates the current max level hint",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
