//! Dispatches trace events to [`Subscriber`]s.
//!
//! This module re-exports the dispatcher layer defined by [`tracing_core::dispatcher`]:
//! [`Dispatch`], the cloneable, type-erased handle that forwards trace data from
//! instrumentation points to a [`Subscriber`], together with the functions that install one as
//! the default — [`set_global_default`] for the whole process and, when the `std` feature is
//! enabled, the scoped, thread-local [`with_default`] and [`set_default`] (which returns a
//! [`DefaultGuard`]) — plus [`get_default`], through which instrumentation accesses the
//! currently active `Dispatch`, and the non-owning [`WeakDispatch`] handle.
//!
//! See the [`tracing_core::dispatcher`] documentation for the full discussion of how default
//! subscribers are installed, scoped, and accessed.
//!
//! The [`crate::subscriber`] module additionally provides `set_default` and `with_default`
//! conveniences that accept an owned [`Subscriber`] and wrap it in a `Dispatch` internally.
//!
//! [`Subscriber`]: crate::Subscriber
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub use tracing_core::dispatcher::DefaultGuard;
pub use tracing_core::dispatcher::Dispatch;
pub use tracing_core::dispatcher::SetGlobalDefaultError;
pub use tracing_core::dispatcher::WeakDispatch;
pub use tracing_core::dispatcher::get_default;
/// Private API for internal use by tracing's macros.
///
/// This function is *not* considered part of `tracing`'s public API, and has no
/// stability guarantees. If you use it, and it breaks or disappears entirely,
/// don't say we didn't warn you.
#[doc(hidden)]
pub use tracing_core::dispatcher::has_been_set;
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub use tracing_core::dispatcher::set_default;
pub use tracing_core::dispatcher::set_global_default;
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub use tracing_core::dispatcher::with_default;
