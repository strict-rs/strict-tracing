//! Regression coverage for concurrent callsite registration.

#[cfg(test)]
mod tests {
  use core::num::NonZeroU64;
  use std::ptr;
  use std::sync::Arc;
  use std::sync::atomic::AtomicBool;
  use std::sync::atomic::AtomicPtr;
  use std::sync::atomic::Ordering;
  use std::thread::JoinHandle;
  use std::thread::{
    self,
  };
  use std::time::Duration;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Subscriber;
  use tracing::subscriber::SubscriberResult;
  use tracing::subscriber::set_default;
  use tracing_core::Metadata;
  use tracing_core::span;

  struct TestSubscriber {
    sleep:               Duration,
    callsite:            AtomicPtr<()>,
    mismatched_callsite: Arc<AtomicBool>,
  }

  impl Subscriber for TestSubscriber {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> SubscriberResult<tracing_core::Interest> {
      if !self.sleep.is_zero() {
        thread::sleep(self.sleep);
      }

      let metadata_ptr = ptr::from_ref(metadata).cast::<()>().cast_mut();
      self.callsite.store(metadata_ptr, Ordering::SeqCst);
      Ok(tracing_core::Interest::always())
    }

    fn event(&self, event: &tracing_core::Event<'_>) -> SubscriberResult {
      let stored_callsite = self.callsite.load(Ordering::SeqCst);
      let event_callsite = ptr::from_ref(event.metadata()).cast::<()>().cast_mut();

      self
        .mismatched_callsite
        .store(stored_callsite != event_callsite, Ordering::SeqCst);
      Ok(())
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
      Ok(true)
    }
    fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
      Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
    }
    fn record(&self, _span: span::Id, _values: &span::Record<'_>) -> SubscriberResult {
      Ok(())
    }
    fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
      Ok(())
    }
    fn enter(&self, _span: span::Id) -> SubscriberResult {
      Ok(())
    }
    fn exit(&self, _span: span::Id) -> SubscriberResult {
      Ok(())
    }
  }

  fn subscriber_thread(idx: usize, register_sleep_micros: u64) -> Result<JoinHandle<bool>, TestFailure> {
    ensure_ok(
      thread::Builder::new().name(format!("subscriber-{idx}")).spawn(move || {
        let mismatched_callsite = Arc::new(AtomicBool::new(false));
        let subscriber_mismatch = Arc::clone(&mismatched_callsite);
        // We use a sleep to ensure the starting order of the 2 threads.
        let subscriber = TestSubscriber {
          sleep:               Duration::from_micros(register_sleep_micros),
          callsite:            AtomicPtr::new(ptr::null_mut()),
          mismatched_callsite: subscriber_mismatch,
        };
        let _subscriber_guard = set_default(subscriber);

        tracing::info!("event-from-{idx}", idx = idx);

        // Wait a bit for everything to end (we don't want to remove the subscriber
        // immediately because that will mix up the test).
        thread::sleep(Duration::from_millis(100));
        mismatched_callsite.load(Ordering::SeqCst)
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
