//! Regression coverage for callsites registered during concurrent dispatch setup.
#![cfg(feature = "std")]

use std::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
    thread::{self, JoinHandle},
    time::Duration,
};

use tracing_core::{
    Dispatch, Event, Kind, Level, Metadata, Subscriber,
    callsite::{Callsite as _, DefaultCallsite},
    dispatcher::set_default,
    field::{FieldSet, Value},
    span,
};

struct TestSubscriber {
    sleep: Duration,
    callsite: AtomicPtr<()>,
}

impl TestSubscriber {
    fn new(sleep_micros: u64) -> Self {
        Self {
            sleep: Duration::from_micros(sleep_micros),
            callsite: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

impl Subscriber for TestSubscriber {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> tracing_core::Interest {
        if !self.sleep.is_zero() {
            thread::sleep(self.sleep);
        }

        let metadata_ptr = ptr::from_ref(metadata).cast::<()>().cast_mut();
        self.callsite.store(metadata_ptr, Ordering::SeqCst);

        tracing_core::Interest::always()
    }

    fn event(&self, event: &Event<'_>) {
        let stored_callsite = self.callsite.load(Ordering::SeqCst);
        let event_callsite = ptr::from_ref(event.metadata()).cast::<()>().cast_mut();

        // This assert is the actual test.
        assert_eq!(
            stored_callsite, event_callsite,
            "stored callsite: {stored_callsite:#?} does not match event \
            callsite: {event_callsite:#?}. Was `event` called before \
            `register_callsite`?"
        );
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(0)
    }
    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
    fn enter(&self, _span: &span::Id) {}
    fn exit(&self, _span: &span::Id) {}
}

fn subscriber_thread(idx: usize, register_sleep_micros: u64) -> JoinHandle<()> {
    thread::Builder::new()
        .name(format!("subscriber-{idx}"))
        .spawn(move || {
            // We use a sleep to ensure the starting order of the 2 threads.
            let subscriber = TestSubscriber::new(register_sleep_micros);
            let _dispatch_guard = set_default(&Dispatch::new(subscriber));

            static CALLSITE: DefaultCallsite = {
                // The values of the metadata are unimportant
                static META: Metadata<'static> = Metadata::new(
                    "event ",
                    "module::path",
                    Level::INFO,
                    None,
                    None,
                    None,
                    FieldSet::new(&["message"], tracing_core::callsite::Identifier(&CALLSITE)),
                    Kind::EVENT,
                );
                DefaultCallsite::new(&META)
            };
            let _interest = CALLSITE.interest();

            let meta = CALLSITE.metadata();
            let field = meta.fields().field("message").unwrap();
            let message = format!("event-from-{idx}", idx = idx);
            let message_value: &dyn Value = &message;
            let values = [(&field, Some(message_value))];
            let value_set = CALLSITE.metadata().fields().value_set(&values);

            Event::dispatch(meta, &value_set);

            // Wait a bit for everything to end (we don't want to remove the subscriber
            // immediately because that will influence the test).
            thread::sleep(Duration::from_millis(10));
        })
        .expect("failed to spawn thread")
}

/// Regression test for missing register_callsite call (#2743)
///
/// This test provokes the race condition which causes the second subscriber to not receive a
/// call to `register_callsite` before it receives a call to `event`.
///
/// Because the test depends on the interaction of multiple dispatchers in different threads,
/// it needs to be in a test file by itself.
#[test]
fn event_before_register() {
    let subscriber_1_register_sleep_micros = 100;
    let subscriber_2_register_sleep_micros = 0;

    let jh1 = subscriber_thread(1, subscriber_1_register_sleep_micros);

    // This delay ensures that the event callsite has interest() called first.
    thread::sleep(Duration::from_micros(50));
    let jh2 = subscriber_thread(2, subscriber_2_register_sleep_micros);

    jh1.join().expect("failed to join thread");
    jh2.join().expect("failed to join thread");
}
