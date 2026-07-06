//! Regression coverage for callsites registered during concurrent dispatch setup.
#![cfg(feature = "std")]

#[cfg(test)]
mod tests {
  use std::thread;
  use std::thread::JoinHandle;
  use std::time::Duration;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_some;
  use tracing_core::Dispatch;
  use tracing_core::Event;
  use tracing_core::Kind;
  use tracing_core::Level;
  use tracing_core::Metadata;
  use tracing_core::callsite::Callsite as _;
  use tracing_core::callsite::DefaultCallsite;
  use tracing_core::callsite::Identifier;
  use tracing_core::dispatcher::set_default;
  use tracing_core::field::FieldSet;
  use tracing_core::field::Value;
  use tracing_core::metadata::SourceLocation;
  use tracing_core::test_util::CallsiteTrackingSubscriber;

  fn subscriber_thread(index: usize, register_sleep_micros: u64) -> Result<JoinHandle<Result<(), TestFailure>>, TestFailure> {
    thread::Builder::new()
      .name(format!("subscriber-{index}"))
      .spawn(move || {
        static CALLSITE: DefaultCallsite = {
          // The values of the metadata are unimportant
          static META: Metadata<'static> = Metadata::new(
            "event ",
            "module::path",
            Level::INFO,
            &SourceLocation::empty(),
            &FieldSet::new(&["message"], Identifier(&CALLSITE)),
            Kind::EVENT,
          );
          DefaultCallsite::new(&META)
        };

        // We use a sleep to ensure the starting order of the 2 threads.
        let subscriber = CallsiteTrackingSubscriber::new().with_register_delay(Duration::from_micros(register_sleep_micros));
        let handle = subscriber.handle();
        let _dispatch_guard = set_default(&Dispatch::new(subscriber));
        let _interest = CALLSITE.interest();

        let meta = CALLSITE.metadata();
        let field = ensure_some(meta.fields().field("message"), "message field is registered")?;
        let message = format!("event-from-{index}");
        let message_value: &dyn Value = &message;
        let values = [(&field, Some(message_value))];
        let value_set = CALLSITE.metadata().fields().value_set(&values);

        Event::dispatch(meta, &value_set);

        // Wait a bit for everything to end (we don't want to remove the subscriber
        // immediately because that will influence the test).
        thread::sleep(Duration::from_millis(10));
        ensure(
          !handle.saw_callsite_mismatch(),
          "event must be called after register_callsite records the callsite",
        )
      })
      .map_err(Into::into)
  }

  /// Regression test for missing `register_callsite` call (#2743).
  ///
  /// This test provokes the race condition which causes the second subscriber to not receive a
  /// call to `register_callsite` before it receives a call to `event`.
  ///
  /// Because the test depends on the interaction of multiple dispatchers in different threads,
  /// it needs to be in a test file by itself.
  #[test]
  fn event_before_register() -> Result<(), TestFailure> {
    let subscriber_1_register_sleep_micros = 100;
    let subscriber_2_register_sleep_micros = 0;

    let jh1 = subscriber_thread(1, subscriber_1_register_sleep_micros)?;

    // This delay ensures that the event callsite has interest() called first.
    thread::sleep(Duration::from_micros(50));
    let jh2 = subscriber_thread(2, subscriber_2_register_sleep_micros)?;

    jh1.join().map_err(|_panic| TestFailure::Condition {
      context: "subscriber 1 thread must not panic",
    })??;
    jh2.join().map_err(|_panic| TestFailure::Condition {
      context: "subscriber 2 thread must not panic",
    })?
  }
}
