use strict_test_support::ResultFailure;
use strict_test_support::ensure_ok;
use tracing_mock::expect;
use tracing_mock::subscriber;

fn main() -> Result<(), ResultFailure<tracing_core::subscriber::SubscriberError>> {
  let (subscriber, handle) = subscriber::mock().event(expect::event()).run_with_handle();

  tracing::subscriber::with_default(subscriber, || {
    tracing::info!("event");
  });

  ensure_ok(handle.finished(), "mock expectations should finish")?;
  Ok(())
}
