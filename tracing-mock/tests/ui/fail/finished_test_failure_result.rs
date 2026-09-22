use strict_test_support::TestFailure;
use tracing_mock::expect;
use tracing_mock::subscriber;

fn main() -> Result<(), TestFailure> {
  let (subscriber, handle) = subscriber::mock().event(expect::event()).run_with_handle();

  tracing::subscriber::with_default(subscriber, || {
    tracing::info!("event");
  });

  let finished: Result<(), TestFailure> = handle.finished();
  finished
}
