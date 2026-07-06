//! Tests registry composition with subscriber extensions.
#![cfg(feature = "registry")]
#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure_ok;
  use tracing_futures::Instrument as _;
  use tracing_futures::WithSubscriber as _;
  use tracing_subscriber::prelude::*;

  #[tokio::test]
  async fn future_with_subscriber() -> Result<(), TestFailure> {
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
