//! Tests span drop instrumentation through the registry.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
    use std::any::{Any, TypeId};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::thread::spawn;

    use strict_test_support::{TestFailure, ensure, ensure_eq};
    use tracing::{
        Dispatch, Event, Level, Metadata, Subscriber,
        span::{self, Id},
        subscriber::with_default,
    };
    use tracing_core::{Interest, LevelFilter, span::Current, subscriber::SubscriberResult};
    use tracing_subscriber::{
        Layer, Registry,
        layer::{Context, SubscriberExt as _},
        registry,
    };

    /// Counters for the lifecycle events tracked by this test.
    #[derive(Default)]
    struct LifecycleCounts {
        layer_new: AtomicUsize,
        layer_enter: AtomicUsize,
        layer_exit: AtomicUsize,
        layer_close: AtomicUsize,

        sub_new: AtomicUsize,
        sub_clone: AtomicUsize,
        sub_enter: AtomicUsize,
        sub_exit: AtomicUsize,
        sub_close: AtomicUsize,
    }

    impl LifecycleCounts {
        fn increment_layer_new(&self) {
            let _previous_count = self.layer_new.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_layer_enter(&self) {
            let _previous_count = self.layer_enter.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_layer_exit(&self) {
            let _previous_count = self.layer_exit.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_layer_close(&self) {
            let _previous_count = self.layer_close.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_sub_new(&self) {
            let _previous_count = self.sub_new.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_sub_clone(&self) {
            let _previous_count = self.sub_clone.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_sub_enter(&self) {
            let _previous_count = self.sub_enter.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_sub_exit(&self) {
            let _previous_count = self.sub_exit.fetch_add(1, Ordering::SeqCst);
        }

        fn increment_sub_close(&self) {
            let _previous_count = self.sub_close.fetch_add(1, Ordering::SeqCst);
        }

        fn layer_new(&self) -> usize {
            self.layer_new.load(Ordering::SeqCst)
        }

        fn layer_enter(&self) -> usize {
            self.layer_enter.load(Ordering::SeqCst)
        }

        fn layer_exit(&self) -> usize {
            self.layer_exit.load(Ordering::SeqCst)
        }

        fn layer_close(&self) -> usize {
            self.layer_close.load(Ordering::SeqCst)
        }

        fn sub_new(&self) -> usize {
            self.sub_new.load(Ordering::SeqCst)
        }

        fn sub_clone(&self) -> usize {
            self.sub_clone.load(Ordering::SeqCst)
        }

        fn sub_enter(&self) -> usize {
            self.sub_enter.load(Ordering::SeqCst)
        }

        fn sub_exit(&self) -> usize {
            self.sub_exit.load(Ordering::SeqCst)
        }

        fn sub_close(&self) -> usize {
            self.sub_close.load(Ordering::SeqCst)
        }
    }

    /// Wraps `tracing_subscriber::Registry` and counts subscriber lifecycle calls.
    struct CountingSubscriber {
        inner: Registry,
        counts: Arc<LifecycleCounts>,
    }

    impl Subscriber for CountingSubscriber {
        fn on_register_dispatch(&self, subscriber: &Dispatch) -> SubscriberResult {
            self.inner.on_register_dispatch(subscriber)
        }

        fn register_callsite(
            &self,
            metadata: &'static Metadata<'static>,
        ) -> SubscriberResult<Interest> {
            self.inner.register_callsite(metadata)
        }

        fn max_level_hint(&self) -> Option<LevelFilter> {
            self.inner.max_level_hint()
        }

        fn event_enabled(&self, event: &Event<'_>) -> SubscriberResult<bool> {
            self.inner.event_enabled(event)
        }

        fn clone_span(&self, id: Id) -> SubscriberResult<Id> {
            self.counts.increment_sub_clone();
            self.inner.clone_span(id)
        }

        fn try_close(&self, id: Id) -> SubscriberResult<bool> {
            self.counts.increment_sub_close();
            self.inner.try_close(id)
        }

        fn current_span(&self) -> SubscriberResult<Current> {
            self.inner.current_span()
        }

        fn downcast_ref_by_id(&self, id: TypeId) -> Option<&dyn Any> {
            if id == TypeId::of::<Self>() {
                return Some(self);
            }

            self.inner.downcast_ref_by_id(id)
        }

        fn enabled(&self, metadata: &Metadata<'_>) -> SubscriberResult<bool> {
            self.inner.enabled(metadata)
        }

        fn new_span(&self, span: &span::Attributes<'_>) -> SubscriberResult<Id> {
            self.counts.increment_sub_new();
            self.inner.new_span(span)
        }

        fn record(&self, span: Id, values: &span::Record<'_>) -> SubscriberResult {
            self.inner.record(span, values)
        }

        fn record_follows_from(&self, span: Id, follows: Id) -> SubscriberResult {
            self.inner.record_follows_from(span, follows)
        }

        fn event(&self, event: &Event<'_>) -> SubscriberResult {
            self.inner.event(event)
        }

        fn enter(&self, span: Id) -> SubscriberResult {
            self.inner.enter(span)?;
            self.counts.increment_sub_enter();
            Ok(())
        }

        fn exit(&self, span: Id) -> SubscriberResult {
            self.inner.exit(span)?;
            self.counts.increment_sub_exit();
            Ok(())
        }
    }

    /// Counts lifecycle callbacks observed by the layer atop the subscriber.
    struct CountingLayer {
        counts: Arc<LifecycleCounts>,
    }

    impl Layer<CountingSubscriber> for CountingLayer {
        fn on_new_span(
            &self,
            _attrs: &span::Attributes<'_>,
            _id: Id,
            _ctx: Context<'_, CountingSubscriber>,
        ) -> SubscriberResult {
            self.counts.increment_layer_new();
            Ok(())
        }

        fn on_enter(&self, _id: Id, _ctx: Context<'_, CountingSubscriber>) -> SubscriberResult {
            self.counts.increment_layer_enter();
            Ok(())
        }

        fn on_exit(&self, _id: Id, _ctx: Context<'_, CountingSubscriber>) -> SubscriberResult {
            self.counts.increment_layer_exit();
            Ok(())
        }

        fn on_close(&self, _id: Id, _ctx: Context<'_, CountingSubscriber>) -> SubscriberResult {
            self.counts.increment_layer_close();
            Ok(())
        }
    }

    #[test]
    fn span_entered_on_different_thread_from_subscriber() -> Result<(), TestFailure> {
        let counts = Arc::new(LifecycleCounts::default());

        let layer = CountingLayer {
            counts: Arc::clone(&counts),
        };

        let counting_subscriber = CountingSubscriber {
            inner: registry(),
            counts: Arc::clone(&counts),
        };
        let subscriber = Arc::new(counting_subscriber.with(layer));

        let span = with_default(Arc::clone(&subscriber), move || {
            tracing::span!(Level::INFO, "span")
        });

        let thread_result = spawn(move || {
            let _entered = span.entered();
        })
        .join();
        ensure(
            thread_result.is_ok(),
            "span can be entered on a thread without a direct subscriber relationship",
        )?;

        ensure_eq(&counts.layer_new(), &1, "layer observes one new span")?;
        ensure_eq(&counts.layer_enter(), &1, "layer observes one enter")?;
        ensure_eq(&counts.layer_exit(), &1, "layer observes one exit")?;
        ensure_eq(&counts.layer_close(), &1, "layer observes one close")?;

        let sub_new_and_clone = counts.sub_new().saturating_add(counts.sub_clone());
        ensure_eq(&counts.sub_new(), &1, "subscriber observes one new span")?;
        ensure_eq(
            &sub_new_and_clone,
            &counts.sub_close(),
            "subscriber closes each new or cloned span",
        )?;
        ensure_eq(
            &counts.sub_enter(),
            &counts.layer_enter(),
            "subscriber and layer enter counts match",
        )?;
        ensure_eq(
            &counts.sub_exit(),
            &counts.layer_exit(),
            "subscriber and layer exit counts match",
        )
    }
}
