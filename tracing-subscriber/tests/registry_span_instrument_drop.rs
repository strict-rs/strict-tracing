//! Tests span drop instrumentation through the registry.
#![cfg(feature = "registry")]

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex};

use tracing::{
    Dispatch, Event, Metadata, Subscriber,
    span::{self, Id},
};
use tracing_core::{Interest, LevelFilter};
use tracing_subscriber::{
    Layer, Registry,
    layer::{Context, SubscriberExt},
};

#[test]
fn span_entered_on_different_thread_from_subscriber() {
    /// Counters for various lifecycle events we want to track.
    #[derive(Default)]
    struct LifecycleCounts {
        layer_new_count: usize,
        layer_enter_count: usize,
        layer_exit_count: usize,
        layer_close_count: usize,

        sub_new_count: usize,
        sub_clone_count: usize,
        sub_enter_count: usize,
        sub_exit_count: usize,
        sub_close_count: usize,
    }

    /// Wraps `tracing_subscriber::Registry` and adds some accounting
    /// to verify that the subscriber is receiving the expected number of calls.
    struct CountingSubscriber {
        inner: Registry,
        counts: Arc<Mutex<LifecycleCounts>>,
    }

    // Forward all subscriber methods to the inner registry, adding counts where appropriate.
    impl Subscriber for CountingSubscriber {
        fn on_register_dispatch(&self, subscriber: &Dispatch) {
            self.inner.on_register_dispatch(subscriber);
        }

        fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
            self.inner.register_callsite(metadata)
        }

        fn max_level_hint(&self) -> Option<LevelFilter> {
            self.inner.max_level_hint()
        }

        fn event_enabled(&self, event: &Event<'_>) -> bool {
            self.inner.event_enabled(event)
        }

        fn clone_span(&self, id: &Id) -> Id {
            self.counts.lock().unwrap().sub_clone_count += 1;
            self.inner.clone_span(id)
        }

        fn drop_span(&self, id: Id) {
            self.counts.lock().unwrap().sub_close_count += 1;
            self.inner.drop_span(id);
        }

        fn try_close(&self, id: Id) -> bool {
            self.counts.lock().unwrap().sub_close_count += 1;
            self.inner.try_close(id)
        }

        fn current_span(&self) -> tracing_core::span::Current {
            self.inner.current_span()
        }

        fn downcast_ref_by_id(&self, id: TypeId) -> Option<&dyn Any> {
            if id == TypeId::of::<Self>() {
                return Some(self);
            }

            self.inner.downcast_ref_by_id(id)
        }

        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            self.inner.enabled(metadata)
        }

        fn new_span(&self, span: &span::Attributes<'_>) -> Id {
            self.counts.lock().unwrap().sub_new_count += 1;
            self.inner.new_span(span)
        }

        fn record(&self, span: &Id, values: &span::Record<'_>) {
            self.inner.record(span, values);
        }

        fn record_follows_from(&self, span: &Id, follows: &Id) {
            self.inner.record_follows_from(span, follows);
        }

        fn event(&self, event: &Event<'_>) {
            self.inner.event(event);
        }

        fn enter(&self, span: &Id) {
            self.inner.enter(span);
            self.counts.lock().unwrap().sub_enter_count += 1;
        }

        fn exit(&self, span: &Id) {
            self.inner.exit(span);
            self.counts.lock().unwrap().sub_exit_count += 1;
        }
    }

    /// Similar to the above, but for a `Layer` which sits atop the subscriber.
    struct CountingLayer {
        counts: Arc<Mutex<LifecycleCounts>>,
    }

    // Just does bookkeeping where relevant.
    impl Layer<CountingSubscriber> for CountingLayer {
        fn on_new_span(
            &self,
            _attrs: &span::Attributes<'_>,
            _id: &Id,
            _ctx: Context<'_, CountingSubscriber>,
        ) {
            self.counts.lock().unwrap().layer_new_count += 1;
        }

        fn on_enter(&self, _id: &Id, _ctx: Context<'_, CountingSubscriber>) {
            self.counts.lock().unwrap().layer_enter_count += 1;
        }

        fn on_exit(&self, _id: &Id, _ctx: Context<'_, CountingSubscriber>) {
            self.counts.lock().unwrap().layer_exit_count += 1;
        }

        fn on_close(&self, _id: Id, _ctx: Context<'_, CountingSubscriber>) {
            self.counts.lock().unwrap().layer_close_count += 1;
        }
    }

    // Setup subscriber and layer.

    let counts = Arc::new(Mutex::new(LifecycleCounts::default()));

    let layer = CountingLayer {
        counts: counts.clone(),
    };

    let subscriber = CountingSubscriber {
        inner: tracing_subscriber::registry(),
        counts: counts.clone(),
    };
    let subscriber = Arc::new(subscriber.with(layer));

    // Create a span using the subscriber
    let span = tracing::subscriber::with_default(subscriber.clone(), move || {
        tracing::span!(tracing::Level::INFO, "span")
    });

    // Enter the span in a thread which doesn't have a direct relationship to the subscriber.
    std::thread::spawn(move || {
        let _entered = span.entered();
    })
    .join()
    .unwrap();

    // layer should have seen exactly one new span & close
    // should be one enter / exit cycle

    let counts = counts.lock().unwrap();

    assert_eq!(counts.layer_new_count, 1);
    assert_eq!(counts.layer_enter_count, 1);
    assert_eq!(counts.layer_exit_count, 1);
    assert_eq!(counts.layer_close_count, 1);

    // subscriber should have seen one new span
    // new + any clones should equal number of closes
    // enter and exit should match layer counts

    assert_eq!(counts.sub_new_count, 1);
    assert_eq!(
        counts.sub_new_count + counts.sub_clone_count,
        counts.sub_close_count
    );

    assert_eq!(counts.sub_enter_count, counts.layer_enter_count);
    assert_eq!(counts.sub_exit_count, counts.layer_exit_count);
}
