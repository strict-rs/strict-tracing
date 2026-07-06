//! Regression coverage for concurrent callsite registration.

#[cfg(test)]
mod tests {
  use std::thread;
  use std::thread::JoinHandle;
  use std::time::Duration;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::subscriber::set_default;
  use tracing_core::test_util::CallsiteTrackingSubscriber;

  fn subscriber_thread(idx: usize, register_sleep_micros: u64) -> Result<JoinHandle<bool>, TestFailure> {
    ensure_ok(
      thread::Builder::new().name(format!("subscriber-{idx}")).spawn(move || {
        // We use a sleep to ensure the starting order of the 2 threads.
        let subscriber = CallsiteTrackingSubscriber::new().with_register_delay(Duration::from_micros(register_sleep_micros));
        let handle = subscriber.handle();
        let _subscriber_guard = set_default(subscriber);

        tracing::info!("event-from-{idx}", idx = idx);

        // Wait a bit for everything to end (we don't want to remove the subscriber
        // immediately because that will mix up the test).
        thread::sleep(Duration::from_millis(100));
        handle.saw_callsite_mismatch()
      }),
      "subscriber thread should spawn",
    )
  }

  #[test]
  fn event_before_register() -> Result<(), TestFailure> {
    let subscriber_1_register_sleep_micros = 100;
    let subscriber_2_register_sleep_micros = 0;

    let jh1 = subscriber_thread(1, subscriber_1_register_sleep_micros)?;

    // This delay ensures that the event!() in the first thread is executed first.
    thread::sleep(Duration::from_micros(50));
    let jh2 = subscriber_thread(2, subscriber_2_register_sleep_micros)?;

    let subscriber1_mismatched = match jh1.join() {
      Ok(mismatched) => mismatched,
      Err(_panic) => return ensure(false, "first subscriber thread should join"),
    };
    let subscriber2_mismatched = match jh2.join() {
      Ok(mismatched) => mismatched,
      Err(_panic) => return ensure(false, "second subscriber thread should join"),
    };

    ensure(
      !subscriber1_mismatched,
      "first subscriber event callsite should match registered callsite",
    )?;
    ensure(
      !subscriber2_mismatched,
      "second subscriber event callsite should match registered callsite",
    )
  }
}
