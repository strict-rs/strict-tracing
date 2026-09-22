//! Thread-local dispatcher integration coverage.
#![cfg(feature = "std")]

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
  use tracing_core::test_util::Secondary;

  #[test]
  fn set_default_dispatch() -> Result<(), TestError> {
    ensure_ok(
      set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())),
      "global dispatch set failed",
    )?;
    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Primary>>(), "global dispatch get failed")
        .map(drop)
        .map_err(TestError::from)
    })?;

    let guard = set_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()));
    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Secondary>>(), "set_default get failed")
        .map(drop)
        .map_err(TestError::from)
    })?;

    // Drop the guard, setting the dispatch back to the global dispatch
    drop(guard);

    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Primary>>(), "global dispatch get failed")
        .map(drop)
        .map_err(TestError::from)
    })
  }

  #[test]
  fn nested_set_default() -> Result<(), TestError> {
    let _guard = set_default(&Dispatch::new(NoOpSubscriber::<Primary>::new()));
    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Primary>>(), "set_default for outer subscriber failed")
        .map(drop)
        .map_err(TestError::from)
    })?;

    let inner_guard = set_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()));
    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Secondary>>(), "set_default inner subscriber failed")
        .map(drop)
        .map_err(TestError::from)
    })?;

    drop(inner_guard);
    get_default(|current| {
      ensure(current.is::<NoOpSubscriber<Primary>>(), "set_default outer subscriber failed")
        .map(drop)
        .map_err(TestError::from)
    })
  }
}
