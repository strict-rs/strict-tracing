//! [`Layer`]s that control which spans and events are enabled by the wrapped
//! subscriber.
//!
//! This module contains a number of types that provide implementations of
//! various strategies for filtering which spans and events are enabled. For
//! details on filtering spans and events using [`Layer`]s, see the
//! [`layer` module's documentation].
//!
//! [`layer` module's documentation]: crate::layer#filtering-with-layers
//! [`Layer`]: crate::layer
#[cfg(not(all(feature = "registry", feature = "std")))]
use core::any::TypeId;

#[cfg(not(all(feature = "registry", feature = "std")))]
use tracing_core::Subscriber;

#[cfg(not(all(feature = "registry", feature = "std")))]
use crate::Layer;

/// Closure-backed filter constructors and implementations.
mod filter_fn;

feature! {
    #![all(feature = "env-filter", feature = "std")]
    /// Environment-variable directive filters.
    mod env;
    pub use self::env::*;
}

feature! {
    #![all(feature = "registry", feature = "std")]
    /// Per-layer filter wrappers and bookkeeping.
    #[doc(hidden)]
    pub mod layer_filters;
    pub use self::layer_filters::*;
}

/// Level-based filter implementation.
mod level;

pub use self::filter_fn::*;
pub use self::level::LevelFilter;
pub use self::level::ParseError as LevelParseError;

feature! {
    #![any(feature = "std", feature = "alloc")]
    pub mod targets;
    pub use self::targets::Targets;

    /// Shared directive parsing and matching support.
    mod directive;
    pub use self::directive::ParseError;
}

/// Returns whether a type ID identifies the per-layer-filter downcast marker.
#[cfg(not(all(feature = "registry", feature = "std")))]
pub(crate) const fn is_plf_downcast_marker(_: TypeId) -> bool {
  false
}

/// Does a type implementing `Subscriber` contain any per-layer filters?
#[cfg(not(all(feature = "registry", feature = "std")))]
pub(crate) const fn subscriber_has_plf<S>(_: &S) -> bool
where
  S: Subscriber,
{
  false
}

/// Does a type implementing `Layer` contain any per-layer filters?
#[cfg(not(all(feature = "registry", feature = "std")))]
pub(crate) const fn layer_has_plf<L, S>(_: &L) -> bool
where
  L: Layer<S>,
  S: Subscriber,
{
  false
}
