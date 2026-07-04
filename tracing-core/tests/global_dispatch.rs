//! Global dispatcher integration coverage.

#[cfg(test)]
mod tests {
  mod common {
    include!("common/mod.rs");
  }

  use common::*;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing_core::dispatcher::*;

  #[test]
  fn global_dispatch() -> Result<(), TestFailure> {
    ensure_ok(set_global_default(Dispatch::new(TestSubscriberA)), "global dispatch set failed")?;
    get_default(|current| ensure(current.is::<TestSubscriberA>(), "global dispatch get failed"))?;

    #[cfg(feature = "std")]
    with_default(&Dispatch::new(TestSubscriberB), || {
      get_default(|current| ensure(current.is::<TestSubscriberB>(), "thread-local override of global dispatch failed"))
    })?;

    get_default(|current| ensure(current.is::<TestSubscriberA>(), "reset to global override failed"))?;

    ensure(
      set_global_default(Dispatch::new(TestSubscriberA)).is_err(),
      "double global dispatch set must fail",
    )
  }
}
