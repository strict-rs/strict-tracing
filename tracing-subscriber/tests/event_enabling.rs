//! Tests event enabling behavior for subscriber layers.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use strict_test_support::{TestFailure, ensure_eq};
    use tracing::{Event, Metadata, Subscriber, subscriber::with_default};
    use tracing_core::subscriber::SubscriberResult;
    use tracing_subscriber::{Layer, layer::Context, prelude::*, registry};

    struct TrackingLayer {
        enabled: bool,
        event_enabled_count: Arc<AtomicUsize>,
        event_enabled: bool,
        on_event_count: Arc<AtomicUsize>,
    }

    impl<C> Layer<C> for TrackingLayer
    where
        C: Subscriber + Send + Sync + 'static,
    {
        fn enabled(
            &self,
            _metadata: &Metadata<'_>,
            _ctx: Context<'_, C>,
        ) -> SubscriberResult<bool> {
            Ok(self.enabled)
        }

        fn event_enabled(
            &self,
            _event: &Event<'_>,
            _ctx: Context<'_, C>,
        ) -> SubscriberResult<bool> {
            let _previous_count = self.event_enabled_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.event_enabled)
        }

        fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, C>) -> SubscriberResult {
            let _previous_count = self.on_event_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn event_enabled_is_only_called_once() -> Result<(), TestFailure> {
        let event_enabled_count = Arc::new(AtomicUsize::default());
        let count = Arc::clone(&event_enabled_count);
        let subscriber = registry().with(TrackingLayer {
            enabled: true,
            event_enabled_count,
            event_enabled: true,
            on_event_count: Arc::new(AtomicUsize::default()),
        });
        with_default(subscriber, || {
            tracing::error!("hiya!");
        });

        ensure_eq(
            &1,
            &count.load(Ordering::SeqCst),
            "event_enabled is called once for an enabled event",
        )
    }

    #[test]
    fn event_enabled_not_called_when_not_enabled() -> Result<(), TestFailure> {
        let event_enabled_count = Arc::new(AtomicUsize::default());
        let count = Arc::clone(&event_enabled_count);
        let subscriber = registry().with(TrackingLayer {
            enabled: false,
            event_enabled_count,
            event_enabled: true,
            on_event_count: Arc::new(AtomicUsize::default()),
        });
        with_default(subscriber, || {
            tracing::error!("hiya!");
        });

        ensure_eq(
            &0,
            &count.load(Ordering::SeqCst),
            "event_enabled is skipped when the layer is disabled",
        )
    }

    #[test]
    fn event_disabled_does_disable_event() -> Result<(), TestFailure> {
        let on_event_count = Arc::new(AtomicUsize::default());
        let count = Arc::clone(&on_event_count);
        let subscriber = registry().with(TrackingLayer {
            enabled: true,
            event_enabled_count: Arc::new(AtomicUsize::default()),
            event_enabled: false,
            on_event_count,
        });
        with_default(subscriber, || {
            tracing::error!("hiya!");
        });

        ensure_eq(
            &0,
            &count.load(Ordering::SeqCst),
            "disabled events are not observed by on_event",
        )
    }
}
