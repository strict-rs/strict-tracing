//! Regression coverage for callsite registration re-entry.

#[cfg(test)]
mod tests {
  use core::num::NonZeroU64;
  use std::sync::mpsc;
  use std::thread;
  use std::time::Duration;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::Event;
  use tracing::metadata::Metadata;
  use tracing::span;
  use tracing::subscriber::Interest;
  use tracing::subscriber::Subscriber;
  use tracing::subscriber::SubscriberResult;
  use tracing::subscriber::{
    self,
  };

  #[test]
  fn register_callsite_doesnt_deadlock() -> Result<(), TestFailure> {
    struct EvilSubscriber;

    impl Subscriber for EvilSubscriber {
      fn register_callsite(&self, meta: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        tracing::info!(?meta, "registered a callsite");
        Ok(Interest::always())
      }

      fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
        Ok(true)
      }
      fn new_span(&self, _: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
        Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
      }
      fn record(&self, _: span::Id, _: &span::Record<'_>) -> SubscriberResult {
        Ok(())
      }
      fn record_follows_from(&self, _: span::Id, _: span::Id) -> SubscriberResult {
        Ok(())
      }
      fn event(&self, _: &Event<'_>) -> SubscriberResult {
        Ok(())
      }
      fn enter(&self, _: span::Id) -> SubscriberResult {
        Ok(())
      }
      fn exit(&self, _: span::Id) -> SubscriberResult {
        Ok(())
      }
    }

    ensure_ok(subscriber::set_global_default(EvilSubscriber), "global subscriber should install")?;

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
