//! Thread-local dispatcher integration coverage.
#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing_core::dispatcher::*;
  use tracing_core::test_util::NoOpSubscriber;
  use tracing_core::test_util::Primary;
  use tracing_core::test_util::Secondary;

  #[test]
  fn set_default_dispatch() -> Result<(), TestFailure> {
    ensure_ok(
      set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())),
      "global dispatch set failed",
    )?;
    get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "global dispatch get failed"))?;

    let guard = set_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()));
    get_default(|current| ensure(current.is::<NoOpSubscriber<Secondary>>(), "set_default get failed"))?;

    // Drop the guard, setting the dispatch back to the global dispatch
    drop(guard);

    get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "global dispatch get failed"))
  }

  #[test]
  fn nested_set_default() -> Result<(), TestFailure> {
    let _guard = set_default(&Dispatch::new(NoOpSubscriber::<Primary>::new()));
    get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "set_default for outer subscriber failed"))?;

    let inner_guard = set_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()));
    get_default(|current| ensure(current.is::<NoOpSubscriber<Secondary>>(), "set_default inner subscriber failed"))?;

    drop(inner_guard);
    get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "set_default outer subscriber failed"))
  }
}
