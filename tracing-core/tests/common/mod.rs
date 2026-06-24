// Shared subscriber fixtures for dispatcher integration tests.

use core::num::NonZeroU64;

use tracing_core::{Event, metadata::Metadata, span, subscriber::Subscriber};

/// Subscriber fixture used as the outer/global test subscriber.
pub(super) struct TestSubscriberA;
impl Subscriber for TestSubscriberA {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_non_zero_u64(NonZeroU64::MIN)
    }
    fn record(&self, _: span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: span::Id, _: span::Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: span::Id) {}
    fn exit(&self, _: span::Id) {}
}
/// Subscriber fixture used as the inner/scoped test subscriber.
#[cfg(feature = "std")]
pub(super) struct TestSubscriberB;
#[cfg(feature = "std")]
impl Subscriber for TestSubscriberB {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_non_zero_u64(NonZeroU64::MIN)
    }
    fn record(&self, _: span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: span::Id, _: span::Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: span::Id) {}
    fn exit(&self, _: span::Id) {}
}
