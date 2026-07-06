//! Local dispatcher lookup coverage before global initialization.

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing_core::dispatcher;
  use tracing_core::dispatcher::Dispatch;
  use tracing_core::subscriber::NoSubscriber;
  use tracing_core::test_util::NoOpSubscriber;
  use tracing_core::test_util::Primary;
  #[cfg(feature = "std")]
  use tracing_core::test_util::Secondary;

  /// This test reproduces the following issues:
  /// - <https://github.com/tokio-rs/tracing/issues/2587>
  /// - <https://github.com/tokio-rs/tracing/issues/2411>
  /// - <https://github.com/tokio-rs/tracing/issues/2436>
  #[test]
  fn local_dispatch_before_init() -> Result<(), TestFailure> {
    dispatcher::get_default(|current| ensure(current.is::<NoSubscriber>(), "initial default subscriber is NoSubscriber"))?;

    // Temporarily override the default dispatcher with a scoped dispatcher.
    // Using a scoped dispatcher makes the thread local state attempt to cache
    // the scoped default.
    #[cfg(feature = "std")]
    dispatcher::with_default(&Dispatch::new(NoOpSubscriber::<Secondary>::new()), || {
      dispatcher::get_default(|current| ensure(current.is::<NoOpSubscriber<Secondary>>(), "overriden subscriber not set"))
    })?;

    dispatcher::get_default(|current| ensure(current.is::<NoSubscriber>(), "scoped default resets to NoSubscriber"))?;

    ensure_ok(
      dispatcher::set_global_default(Dispatch::new(NoOpSubscriber::<Primary>::new())),
      "set global dispatch failed",
    )?;

    dispatcher::get_default(|current| ensure(current.is::<NoOpSubscriber<Primary>>(), "default subscriber not set"))
  }
}
