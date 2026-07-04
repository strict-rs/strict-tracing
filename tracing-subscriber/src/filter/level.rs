use tracing_core::Metadata;
pub use tracing_core::metadata::LevelFilter;
pub use tracing_core::metadata::ParseLevelFilterError as ParseError;
use tracing_core::subscriber::Interest;
use tracing_core::subscriber::Subscriber;
use tracing_core::subscriber::SubscriberResult;

use crate::layer::Context;

// === impl LevelFilter ===

impl<S: Subscriber> crate::Layer<S> for LevelFilter {
  fn register_callsite(&self, metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
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
