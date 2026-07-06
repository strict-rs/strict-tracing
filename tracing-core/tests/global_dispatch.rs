//! Global dispatcher integration coverage.

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing_core::dispatcher::*;
  use tracing_core::test_util::NoOpSubscriber;
  use tracing_core::test_util::Primary;
  #[cfg(feature = "std")]
  use tracing_core::test_util::Secondary;

  #[test]
  fn global_dispatch() -> Result<(), TestFailure> {
    ensure_ok(
      set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())),
      "global dispatch set failed",
    )?;
    get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "global dispatch get failed"))?;

    #[cfg(feature = "std")]
    with_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()), || {
      get_default(|current| {
        ensure(
          current.is::<NoOpSubscriber<Secondary>>(),
          "thread-local override of global dispatch failed",
        )
      })
    })?;

    get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "reset to global override failed"))?;

    ensure(
      set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())).is_err(),
      "double global dispatch set must fail",
    )
  }
}
