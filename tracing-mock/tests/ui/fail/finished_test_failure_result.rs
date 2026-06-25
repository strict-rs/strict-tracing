use strict_test_support::{TestFailure, ensure_ok};
use tracing_mock::{expect, subscriber};

fn main() -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
        .event(expect::event())
        .run_with_handle();

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("event");
    });

    let finished: Result<(), TestFailure> = handle.finished();
    ensure_ok(finished, "mock expectations should finish")
}
