//! Tests formatter max level hints.
#![cfg(feature = "fmt")]

#[cfg(test)]
mod tests {

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonLevelFilter(#[from] strict_test_support::ComparisonFailure<LevelFilter, LevelFilter>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use tracing_subscriber::filter::LevelFilter;

  #[test]
  fn fmt_sets_max_level_hint() -> Result<(), TestError> {
    let init_result = tracing_subscriber::fmt().with_max_level(LevelFilter::DEBUG).try_init();
    ensure(init_result.is_ok(), "fmt try_init succeeds").map(drop)?;
    ensure_eq(
      LevelFilter::current(),
      LevelFilter::DEBUG,
      "fmt init updates the current max level hint",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
