//! Regression coverage for callsite registration re-entry.

#[cfg(test)]
mod tests {
  use std::sync::mpsc;
  use std::thread;
  use std::time::Duration;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::subscriber;
  use tracing_core::test_util::CallsiteTrackingSubscriber;

  #[test]
  fn register_callsite_doesnt_deadlock() -> Result<(), TestFailure> {
    // Installing this subscriber re-enters the active dispatcher with an event
    // during `register_callsite`; the callsite registry must service that
    // re-entrant registration without deadlocking.
    ensure_ok(
      subscriber::set_global_default(CallsiteTrackingSubscriber::new().with_event_on_register()),
      "global subscriber should install",
    )?;

    // spawn a thread, and assert it doesn't hang...
    let (tx, didnt_hang) = mpsc::channel();
    let th = thread::spawn(move || {
      tracing::info!("hello world!");
      tx.send(())
    });

    ensure_ok(
      didnt_hang
                // Note: 60 seconds is *way* more than enough, but let's be generous in
                // case of e.g. slow CI machines.
                .recv_timeout(Duration::from_mins(1)),
      "the thread must not hang",
    )?;
    let send_result = match th.join() {
      Ok(send_result) => send_result,
      Err(_panic) => return ensure(false, "thread should join successfully"),
    };
    ensure_ok(send_result, "thread should send completion signal")
  }
}
