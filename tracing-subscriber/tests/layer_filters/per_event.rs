use core::fmt::Debug;
use strict_test_support::{TestFailure, ensure_ok};
use tracing::subscriber::set_default;
use tracing::{Event, Level, Metadata};
use tracing_core::{Field, subscriber::SubscriberResult};
use tracing_mock::{expect, layer};
use tracing_subscriber::{
    field::Visit,
    layer::{Context, Filter},
    prelude::*,
};

struct FilterEvent;

impl<S> Filter<S> for FilterEvent {
    fn enabled(&self, _meta: &Metadata<'_>, _cx: &Context<'_, S>) -> SubscriberResult<bool> {
        Ok(true)
    }

    fn event_enabled(&self, event: &Event<'_>, _cx: &Context<'_, S>) -> SubscriberResult<bool> {
        struct ShouldEnable(bool);
        impl Visit for ShouldEnable {
            fn record_bool(&mut self, field: &Field, value: bool) {
                if field.name() == "enable" {
                    self.0 = value;
                }
            }

            fn record_debug(&mut self, _field: &Field, _value: &dyn Debug) {}
        }
        let mut should_enable = ShouldEnable(false);
        event.record(&mut should_enable);
        Ok(should_enable.0)
    }
}

#[test]
fn per_layer_event_field_filtering() -> Result<(), TestFailure> {
    let (expect, handle) = layer::mock()
        .event(expect::event().at_level(Level::TRACE))
        .event(expect::event().at_level(Level::INFO))
        .only()
        .run_with_handle();

    let subscriber = tracing_subscriber::registry().with(expect.with_filter(FilterEvent));
    let _subscriber = set_default(subscriber);

    tracing::trace!(enable = true, "hello trace");
    tracing::debug!("hello debug");
    tracing::info!(enable = true, "hello info");
    tracing::warn!(enable = false, "hello warn");
    tracing::error!("hello error");

    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}
