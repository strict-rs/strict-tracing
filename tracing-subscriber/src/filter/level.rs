use crate::layer::Context;
use tracing_core::{
    Metadata,
    subscriber::{Interest, Subscriber, SubscriberResult},
};

pub use tracing_core::metadata::{LevelFilter, ParseLevelFilterError as ParseError};

// === impl LevelFilter ===

impl<S: Subscriber> crate::Layer<S> for LevelFilter {
    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<Interest> {
        Ok(if self >= metadata.level() {
            Interest::always()
        } else {
            Interest::never()
        })
    }

    fn enabled(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> SubscriberResult<bool> {
        Ok(self >= metadata.level())
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(Some(*self))
    }
}
