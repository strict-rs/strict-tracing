// Shared subscriber fixtures for dispatcher integration tests.

use core::num::NonZeroU64;

use tracing_core::{
    Event, SubscriberResult,
    metadata::Metadata,
    span,
    subscriber::Subscriber,
};

/// Subscriber fixture used as the outer/global test subscriber.
pub(super) struct TestSubscriberA;
impl Subscriber for TestSubscriberA {
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
/// Subscriber fixture used as the inner/scoped test subscriber.
#[cfg(feature = "std")]
pub(super) struct TestSubscriberB;
#[cfg(feature = "std")]
impl Subscriber for TestSubscriberB {
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
