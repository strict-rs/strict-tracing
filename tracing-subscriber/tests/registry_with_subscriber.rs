//! Tests registry composition with subscriber extensions.
#![cfg(feature = "registry")]
#[cfg(test)]
mod tests {

  use tokio::task;
  use tracing_subscriber::util;
  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTokioJoinError(#[from] strict_test_support::ResultFailure<task::JoinError>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultTracingSubscriberUtilTryInitError(#[from] strict_test_support::ResultFailure<util::TryInitError>),
  }

  use strict_test_support::ensure_ok;
  use tracing_futures::Instrument as _;
  use tracing_futures::WithSubscriber as _;
  use tracing_subscriber::prelude::*;

  #[tokio::test]
  async fn future_with_subscriber() -> Result<(), TestError> {
    ensure_ok(tracing_subscriber::registry().try_init(), "registry installs")?;
    let foo_span = tracing::info_span!("foo");
    let _foo_enter = foo_span.enter();
    let bar_span = tracing::info_span!("bar");
    let _bar_enter = bar_span.enter();
    let observed_current = async {
      let _current_span = tracing::Span::current();
    }
    .instrument(tracing::info_span!("hi"))
    .with_subscriber(tracing_subscriber::registry());
    ensure_ok(tokio::spawn(observed_current).await, "future with subscriber completes")?;
    Ok(())
  }
}
