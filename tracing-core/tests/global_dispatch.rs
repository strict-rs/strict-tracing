//! Global dispatcher integration coverage.

#[cfg(test)]
mod tests {

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Retains the native globaldefault failure.
    #[error(transparent)]
    GlobalDefault(#[from] strict_test_support::ResultFailure<SetGlobalDefaultError>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing_core::dispatcher::*;
  use tracing_core::test_util::NoOpSubscriber;
  use tracing_core::test_util::Primary;
  #[cfg(feature = "std")]
  use tracing_core::test_util::Secondary;

  #[test]
  fn global_dispatch() -> Result<(), TestError> {
    ensure_ok(
      set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())),
      "global dispatch set failed",
    )?;
    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Primary>>(), "global dispatch get failed")
        .map(drop)
        .map_err(TestError::from)
    })?;

    #[cfg(feature = "std")]
    with_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()), || {
      get_default(|current| {
        ensure(
          current.is::<NoOpSubscriber<Secondary>>(),
          "thread-local override of global dispatch failed",
        )
        .map(drop)
        .map_err(TestError::from)
      })
    })?;

    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Primary>>(), "reset to global override failed")
        .map(drop)
        .map_err(TestError::from)
    })?;

    ensure(
      set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())).is_err(),
      "double global dispatch set must fail",
    )
    .map(drop)
    .map_err(TestError::from)
  }
}
