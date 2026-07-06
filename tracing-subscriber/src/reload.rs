//! Wrapper for a `Layer` to allow it to be dynamically reloaded.
//!
//! This module provides a [`Layer` type] implementing the [`Layer` trait] or [`Filter` trait]
//! which wraps another type implementing the corresponding trait. This
//! allows the wrapped type to be replaced with another
//! instance of that type at runtime.
//!
//! This can be used in cases where a subset of `Layer` or `Filter` functionality
//! should be dynamically reconfigured, such as when filtering directives may
//! change at runtime. Note that this layer introduces a (relatively small)
//! amount of overhead, and should thus only be used as needed.
//!
//! # Examples
//!
//! Reloading a [global filtering](crate::layer#global-filtering) layer:
//!
//! ```rust
//! # use tracing::info;
//! use tracing_subscriber::{filter, fmt, reload, prelude::*};
//! # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let filter = filter::LevelFilter::WARN;
//! let (filter, reload_handle) = reload::Layer::new(filter);
//! tracing_subscriber::registry()
//!   .with(filter)
//!   .with(fmt::Layer::default())
//!   .try_init()?;
//! #
//! # // specifying the Registry type is required
//! # let _: &reload::Handle<filter::LevelFilter, tracing_subscriber::Registry> = &reload_handle;
//! #
//! info!("This will be ignored");
//! reload_handle.modify(|filter| *filter = filter::LevelFilter::INFO)?;
//! info!("This will be logged");
//! # Ok(()) }
//! ```
//!
//! Reloading a [`Filtered`](crate::filter::Filtered) layer:
//!
//! ```rust
//! # use tracing::info;
//! use tracing_subscriber::{filter, fmt, reload, prelude::*};
//! # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let filtered_layer = fmt::Layer::default().with_filter(filter::LevelFilter::WARN);
//! let (filtered_layer, reload_handle) = reload::Layer::new(filtered_layer);
//! #
//! # // specifying the Registry type is required
//! # let _: &reload::Handle<filter::Filtered<fmt::Layer<tracing_subscriber::Registry>,
//! # filter::LevelFilter, tracing_subscriber::Registry>,tracing_subscriber::Registry>
//! # = &reload_handle;
//! #
//! tracing_subscriber::registry()
//!   .with(filtered_layer)
//!   .try_init()?;
//! info!("This will be ignored");
//! reload_handle.modify(|layer| *layer.filter_mut() = filter::LevelFilter::INFO)?;
//! info!("This will be logged");
//! # Ok(()) }
//! ```
//!
//! ## Note
//!
//! The [`Layer`] implementation is unable to implement downcasting functionality,
//! so certain [`Layer`] will fail to downcast if wrapped in a `reload::Layer`.
//!
//! If you only want to be able to dynamically change the
//! `Filter` on a layer, prefer wrapping that `Filter` in the `reload::Layer`.
//!
//! [`Filter` trait]: crate::layer::Filter
//! [`Layer` type]: Layer
//! [`Layer` trait]: super::layer::Layer
use crate::layer;
use crate::RwLock;

use core::any::{Any, TypeId};
use std::{
    error, fmt,
    marker::PhantomData,
    sync::{Arc, Weak},
};
use tracing_core::{
    callsite, span,
    subscriber::{Interest, Subscriber, SubscriberResult},
    Dispatch, Event, LevelFilter, Metadata,
};
#[cfg(feature = "tracing-log")]
use tracing_log::{log, AsLog};

/// Wraps a `Layer` or `Filter`, allowing it to be reloaded dynamically at runtime.
#[derive(Debug)]
pub struct Layer<L, S> {
    // TODO(eliza): this once used a `crossbeam_util::ShardedRwLock`. We may
    // eventually wish to replace it with a sharded lock implementation on top
    // of our internal `RwLock` wrapper type. If possible, we should profile
    // this first to determine if it's necessary.
    /// The reloadable layer or filter state.
    inner: Arc<RwLock<L>>,
    /// Connects this reload layer to the wrapped subscriber type.
    _s: PhantomData<fn(S)>,
}

/// Allows reloading the state of an associated [`Layer`](crate::layer::Layer).
#[derive(Debug)]
pub struct Handle<L, S> {
    /// Weak reference to the reloadable layer or filter state.
    inner: Weak<RwLock<L>>,
    /// Connects this handle to the wrapped subscriber type.
    _s: PhantomData<fn(S)>,
}

/// Indicates that an error occurred when reloading a layer.
#[derive(Debug)]
pub struct ReloadError {
    /// The specific reload failure.
    kind: ErrorKind,
}

/// Failure modes for reload operations.
#[derive(Debug)]
enum ErrorKind {
    /// The subscriber containing the reload layer has been dropped.
    SubscriberGone,
    /// The standard-library lock was poisoned by a panic while locked.
    #[cfg(not(feature = "parking_lot"))]
    Poisoned,
}

/// Error returned by reload operations.
pub use ReloadError as Error;

// ===== impl Layer =====

impl<L, S> crate::Layer<S> for Layer<L, S>
where
    L: crate::Layer<S> + 'static,
    S: Subscriber,
{
    fn on_register_dispatch(&self, subscriber: &Dispatch) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_register_dispatch(subscriber)
    }

    fn on_layer(&mut self, subscriber: &mut S) {
        try_lock!(self.inner.write(), else return).on_layer(subscriber);
    }

    #[inline]
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        try_lock_subscriber!(self.inner.read()).register_callsite(metadata)
    }

    #[inline]
    fn enabled(&self, metadata: &Metadata<'_>, ctx: layer::Context<'_, S>) -> SubscriberResult<bool> {
        try_lock_subscriber!(self.inner.read()).enabled(metadata, ctx)
    }

    #[inline]
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, span: span::Id, values: &span::Record<'_>, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_record(span, values, ctx)
    }

    #[inline]
    fn on_follows_from(&self, span: span::Id, follows: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_follows_from(span, follows, ctx)
    }

    #[inline]
    fn event_enabled(&self, event: &Event<'_>, ctx: layer::Context<'_, S>) -> SubscriberResult<bool> {
        try_lock_subscriber!(self.inner.read()).event_enabled(event, ctx)
    }

    #[inline]
    fn on_event(&self, event: &Event<'_>, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_event(event, ctx)
    }

    #[inline]
    fn on_enter(&self, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_enter(id, ctx)
    }

    #[inline]
    fn on_exit(&self, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_exit(id, ctx)
    }

    #[inline]
    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_close(id, ctx)
    }

    #[inline]
    fn on_id_change(&self, old: span::Id, new: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_id_change(old, new, ctx)
    }

    #[inline]
    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        try_lock_subscriber!(self.inner.read()).max_level_hint()
    }

    #[doc(hidden)]
    fn downcast_ref_by_id(&self, id: TypeId) -> Option<&dyn Any> {
        // It is not sound to return arbitrary references through a reloadable
        // layer, because the reference could point into the locked layer and be
        // invalidated after the guard is dropped. `NoneLayerMarker` is special:
        // it is used only as a boolean marker and returned as a static value.
        if id == TypeId::of::<layer::NoneLayerMarker>() {
            let marker_present = try_lock!(self.inner.read(), else return None)
                .downcast_ref_by_id(id)
                .is_some();
            if marker_present {
                return Some(&layer::NONE_LAYER_MARKER);
            }
        }

        None
    }
}

// ===== impl Filter =====

#[cfg(all(feature = "registry", feature = "std"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "registry", feature = "std"))))]
impl<S, L> layer::Filter<S> for Layer<L, S>
where
    L: layer::Filter<S> + 'static,
    S: Subscriber,
{
    #[inline]
    fn callsite_enabled(&self, metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        try_lock_subscriber!(self.inner.read()).callsite_enabled(metadata)
    }

    #[inline]
    fn enabled(&self, metadata: &Metadata<'_>, ctx: &layer::Context<'_, S>) -> SubscriberResult<bool> {
        try_lock_subscriber!(self.inner.read()).enabled(metadata, ctx)
    }

    #[inline]
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_new_span(attrs, id, ctx)
    }

    #[inline]
    fn on_record(&self, span: span::Id, values: &span::Record<'_>, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_record(span, values, ctx)
    }

    #[inline]
    fn on_enter(&self, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_enter(id, ctx)
    }

    #[inline]
    fn on_exit(&self, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_exit(id, ctx)
    }

    #[inline]
    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
        try_lock_subscriber!(self.inner.read()).on_close(id, ctx)
    }

    #[inline]
    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        try_lock_subscriber!(self.inner.read()).max_level_hint()
    }
}

impl<L, S> Layer<L, S> {
    /// Wraps the given [`Layer`] or [`Filter`], returning a `reload::Layer`
    /// and a `Handle` that allows the inner value to be modified at runtime.
    ///
    /// [`Layer`]: crate::layer::Layer
    /// [`Filter`]: crate::layer::Filter
    #[allow(
        clippy::single_call_fn,
        reason = "public reload constructor is called by downstream layer and filter users"
    )]
    pub fn new(inner: L) -> (Self, Handle<L, S>) {
        let this = Self {
            inner: Arc::new(RwLock::new(inner)),
            _s: PhantomData,
        };
        let handle = this.handle();
        (this, handle)
    }

    /// Returns a `Handle` that can be used to reload the wrapped [`Layer`] or [`Filter`].
    ///
    /// [`Layer`]: crate::layer::Layer
    /// [`Filter`]: crate::layer::Filter
    #[must_use]
    pub fn handle(&self) -> Handle<L, S> {
        Handle {
            inner: Arc::downgrade(&self.inner),
            _s: PhantomData,
        }
    }
}

// ===== impl Handle =====

impl<L, S> Handle<L, S> {
    /// Replace the current [`Layer`] or [`Filter`] with the provided `new_value`.
    ///
    /// [`Handle::reload`] cannot be used with the [`Filtered`] layer; use
    /// [`Handle::modify`] instead (see [this issue] for additional details).
    ///
    /// However, if the _only_ the [`Filter`]  needs to be modified, use
    /// `reload::Layer` to wrap the `Filter` directly.
    ///
    /// [`Layer`]: crate::layer::Layer
    /// [`Filter`]: crate::layer::Filter
    /// [`Filtered`]: crate::filter::Filtered
    ///
    /// [this issue]: https://github.com/tokio-rs/tracing/issues/1629
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the subscriber containing this reloadable value has
    /// been dropped, or if the backing lock is poisoned when using the standard
    /// library lock backend.
    pub fn reload(&self, new_value: impl Into<L>) -> Result<(), Error> {
        self.modify(|layer| {
            *layer = new_value.into();
        })
    }

    /// Invokes a closure with a mutable reference to the current layer or filter,
    /// allowing it to be modified in place.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the subscriber containing this reloadable value has
    /// been dropped, or if the backing lock is poisoned when using the standard
    /// library lock backend.
    pub fn modify(&self, f: impl FnOnce(&mut L)) -> Result<(), Error> {
        let inner = self.inner.upgrade().ok_or(Error {
            kind: ErrorKind::SubscriberGone,
        })?;

        let mut lock = try_lock!(inner.write(), else return Err(Error::poisoned()));
        f(&mut *lock);
        // Release the lock before rebuilding the interest cache, as that
        // function will lock the new layer.
        drop(lock);

        callsite::rebuild_interest_cache();

        // If the `log` crate compatibility feature is in use, set `log`'s max
        // level as well, in case the max `tracing` level changed. We do this
        // *after* rebuilding the interest cache, as that's when the `tracing`
        // max level filter is re-computed.
        #[cfg(feature = "tracing-log")]
        log::set_max_level(AsLog::as_log(&LevelFilter::current()));

        Ok(())
    }

    /// Returns a clone of the layer or filter's current value if it still exists.
    /// Otherwise, if the subscriber has been dropped, returns `None`.
    #[must_use]
    pub fn clone_current(&self) -> Option<L>
    where
        L: Clone,
    {
        self.with_current(L::clone).ok()
    }

    /// Invokes a closure with a borrowed reference to the current layer or filter,
    /// returning the result (or an error if the subscriber no longer exists).
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if the subscriber containing this reloadable value has
    /// been dropped, or if the backing lock is poisoned when using the standard
    /// library lock backend.
    pub fn with_current<T>(&self, f: impl FnOnce(&L) -> T) -> Result<T, Error> {
        let inner = self.inner.upgrade().ok_or(Error {
            kind: ErrorKind::SubscriberGone,
        })?;
        let lock = try_lock!(inner.read(), else return Err(Error::poisoned()));
        Ok(f(&*lock))
    }
}

impl<L, S> Clone for Handle<L, S> {
    fn clone(&self) -> Self {
        Self {
            inner: Weak::clone(&self.inner),
            _s: PhantomData,
        }
    }
}

// ===== impl Error =====

impl ReloadError {
    #[cfg(not(feature = "parking_lot"))]
    fn poisoned() -> Self {
        Self {
            kind: ErrorKind::Poisoned,
        }
    }

    /// Returns `true` if this error occurred because the layer was poisoned by
    /// a panic on another thread.
    #[must_use]
    pub const fn is_poisoned(&self) -> bool {
        #[cfg(not(feature = "parking_lot"))]
        {
            matches!(self.kind, ErrorKind::Poisoned)
        }
        #[cfg(feature = "parking_lot")]
        {
            match self.kind {
                ErrorKind::SubscriberGone => false,
            }
        }
    }

    /// Returns `true` if this error occurred because the `Subscriber`
    /// containing the reloadable layer was dropped.
    #[must_use]
    pub const fn is_dropped(&self) -> bool {
        matches!(self.kind, ErrorKind::SubscriberGone)
    }
}

impl fmt::Display for ReloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self.kind {
            ErrorKind::SubscriberGone => "subscriber no longer exists",
            #[cfg(not(feature = "parking_lot"))]
            ErrorKind::Poisoned => "lock poisoned",
        };
        f.pad(msg)
    }
}

impl error::Error for ReloadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{Filter as _, Layer as _};
    use crate::registry;
    use alloc::{format, sync::Arc, vec, vec::Vec};
    use core::num::NonZeroU64;
    use parking_lot::Mutex;
    use strict_test_support::{TestFailure, ensure, ensure_contains, ensure_eq, ensure_ok};
    use tracing_core::callsite::Callsite;
    use tracing_core::metadata::Kind;
    use tracing_core::span::{Attributes, Id, Record};
    use tracing_core::Level;

    /// Callsite used by reload wrapper metadata fixtures.
    struct ReloadCallsite;

    /// Shared callsite used by reload wrapper metadata fixtures.
    static RELOAD_CALLSITE: ReloadCallsite = ReloadCallsite;

    /// Metadata used for direct reload wrapper calls.
    static RELOAD_META: Metadata<'static> = tracing_core::metadata! {
        name: "reload_test",
        target: "reload_target",
        level: Level::INFO,
        fields: &["answer"],
        callsite: &RELOAD_CALLSITE,
        kind: Kind::EVENT,
    };

    impl Callsite for ReloadCallsite {
        fn set_interest(&self, _: Interest) {}

        fn metadata(&self) -> &Metadata<'_> {
            &RELOAD_META
        }
    }

    /// Shared hook call log for reload forwarding tests.
    type CallLog = Arc<Mutex<Vec<&'static str>>>;

    /// Reload layer used by handle lifecycle tests.
    type NumberReloadLayer = Layer<usize, registry::Registry>;

    /// Reload handle used by handle lifecycle tests.
    type NumberReloadHandle = Handle<usize, registry::Registry>;

    /// Reload layer used by layer forwarding tests.
    type RecordingReloadLayer = Layer<RecordingLayer, registry::Registry>;

    /// Reload handle used by layer forwarding tests.
    type RecordingReloadHandle = Handle<RecordingLayer, registry::Registry>;

    /// Reload layer used by optional-layer downcast tests.
    type OptionalIdentityReloadLayer = Layer<Option<layer::Identity>, registry::Registry>;

    /// Reload handle used by optional-layer downcast tests.
    type OptionalIdentityReloadHandle = Handle<Option<layer::Identity>, registry::Registry>;

    /// Reload layer used by filter forwarding tests.
    type FilterReloadLayer = Layer<RecordingFilter, registry::Registry>;

    /// Reload handle used by filter forwarding tests.
    type FilterReloadHandle = Handle<RecordingFilter, registry::Registry>;

    /// Layer whose hooks are recorded by reload wrapper tests.
    #[derive(Debug)]
    struct RecordingLayer {
        /// Hook call log.
        calls: CallLog,
    }

    impl RecordingLayer {
        /// Records a layer hook.
        fn record(&self, hook: &'static str) {
            self.calls.lock().push(hook);
        }
    }

    impl layer::Layer<registry::Registry> for RecordingLayer {
        fn on_register_dispatch(&self, _subscriber: &Dispatch) -> SubscriberResult<()> {
            self.record("on_register_dispatch");
            Ok(())
        }

        fn on_layer(&mut self, _subscriber: &mut registry::Registry) {
            self.record("on_layer");
        }

        fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
            self.record("register_callsite");
            Ok(Interest::always())
        }

        fn enabled(&self, _metadata: &Metadata<'_>, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<bool> {
            self.record("enabled");
            Ok(false)
        }

        fn on_new_span(
            &self,
            _attrs: &Attributes<'_>,
            _id: Id,
            _ctx: layer::Context<'_, registry::Registry>,
        ) -> SubscriberResult<()> {
            self.record("on_new_span");
            Ok(())
        }

        fn on_record(
            &self,
            _span: Id,
            _values: &Record<'_>,
            _ctx: layer::Context<'_, registry::Registry>,
        ) -> SubscriberResult<()> {
            self.record("on_record");
            Ok(())
        }

        fn on_follows_from(
            &self,
            _span: Id,
            _follows: Id,
            _ctx: layer::Context<'_, registry::Registry>,
        ) -> SubscriberResult<()> {
            self.record("on_follows_from");
            Ok(())
        }

        fn event_enabled(&self, _event: &Event<'_>, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<bool> {
            self.record("event_enabled");
            Ok(false)
        }

        fn on_event(&self, _event: &Event<'_>, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_event");
            Ok(())
        }

        fn on_enter(&self, _id: Id, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_enter");
            Ok(())
        }

        fn on_exit(&self, _id: Id, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_exit");
            Ok(())
        }

        fn on_close(&self, _id: Id, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_close");
            Ok(())
        }

        fn on_id_change(
            &self,
            _old: Id,
            _new: Id,
            _ctx: layer::Context<'_, registry::Registry>,
        ) -> SubscriberResult<()> {
            self.record("on_id_change");
            Ok(())
        }

        fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
            self.record("max_level_hint");
            Ok(Some(LevelFilter::INFO))
        }
    }

    /// Filter whose hooks are recorded by reload wrapper tests.
    #[derive(Debug)]
    struct RecordingFilter {
        /// Hook call log.
        calls: CallLog,
    }

    impl RecordingFilter {
        /// Records a filter hook.
        fn record(&self, hook: &'static str) {
            self.calls.lock().push(hook);
        }
    }

    impl layer::Filter<registry::Registry> for RecordingFilter {
        fn callsite_enabled(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
            self.record("callsite_enabled");
            Ok(Interest::never())
        }

        fn enabled(&self, _metadata: &Metadata<'_>, _ctx: &layer::Context<'_, registry::Registry>) -> SubscriberResult<bool> {
            self.record("enabled");
            Ok(false)
        }

        fn on_new_span(
            &self,
            _attrs: &Attributes<'_>,
            _id: Id,
            _ctx: layer::Context<'_, registry::Registry>,
        ) -> SubscriberResult<()> {
            self.record("on_new_span");
            Ok(())
        }

        fn on_record(
            &self,
            _span: Id,
            _values: &Record<'_>,
            _ctx: layer::Context<'_, registry::Registry>,
        ) -> SubscriberResult<()> {
            self.record("on_record");
            Ok(())
        }

        fn on_enter(&self, _id: Id, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_enter");
            Ok(())
        }

        fn on_exit(&self, _id: Id, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_exit");
            Ok(())
        }

        fn on_close(&self, _id: Id, _ctx: layer::Context<'_, registry::Registry>) -> SubscriberResult<()> {
            self.record("on_close");
            Ok(())
        }

        fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
            self.record("max_level_hint");
            Ok(Some(LevelFilter::WARN))
        }
    }

    /// Runs a callback with event/span fixtures for direct hook tests.
    fn with_reload_fixtures<R>(
        f: impl FnOnce(&Event<'_>, &Attributes<'_>, &Record<'_>, Id, Id) -> Result<R, TestFailure>,
    ) -> Result<R, TestFailure> {
        let values = RELOAD_META.fields().value_set_all(&[]);
        let event = Event::new(&RELOAD_META, &values);
        let attrs = Attributes::new(&RELOAD_META, &values);
        let record = Record::new(&values);
        let first_id = Id::from_non_zero_u64(NonZeroU64::MIN);
        let second_id = Id::from_non_zero_u64(NonZeroU64::MAX);
        f(&event, &attrs, &record, first_id, second_id)
    }

    #[test]
    fn handle_reads_modifies_reloads_and_reports_dropped_state() -> Result<(), TestFailure> {
        let (layer, handle): (NumberReloadLayer, NumberReloadHandle) = Layer::new(1);
        ensure(handle.clone_current() == Some(1), "clone_current returns the initial value")?;
        let current = ensure_ok(handle.with_current(|inner| *inner), "with_current reads")?;
        ensure_eq(&current, &1, "with_current borrows the current value")?;
        ensure_ok(
            handle.modify(|inner| {
                *inner = 2;
            }),
            "modify updates the current value",
        )?;
        ensure(handle.clone_current() == Some(2), "modify result is visible to clone_current")?;
        let cloned = handle.clone();
        ensure_ok(cloned.reload(3_usize), "cloned handles can reload")?;
        ensure(handle.clone_current() == Some(3), "reload replaces the current value")?;

        let layer_debug = format!("{layer:?}");
        ensure_contains(&layer_debug, "Layer", "reload layer debug names the wrapper")?;
        let handle_debug = format!("{handle:?}");
        ensure_contains(&handle_debug, "Handle", "reload handle debug names the wrapper")?;

        drop(layer);
        let Some(error) = handle.reload(4_usize).err() else {
            return ensure(false, "dropped reload layer returns an error");
        };
        ensure(error.is_dropped(), "dropped reload errors report dropped state")?;
        ensure(!error.is_poisoned(), "dropped reload errors are not poisoned errors")?;
        let display = format!("{error}");
        ensure_contains(&display, "subscriber no longer exists", "dropped error display is stable")?;
        let debug = format!("{error:?}");
        ensure_contains(&debug, "SubscriberGone", "dropped error debug preserves the error kind")?;
        ensure(handle.clone_current().is_none(), "clone_current returns none after layer drop")
    }

    #[test]
    fn reload_layer_forwards_registration_and_metadata_hooks() -> Result<(), TestFailure> {
        with_reload_fixtures(|event, _attrs, _record, _first_id, _second_id| {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let (mut reload_layer, _handle): (RecordingReloadLayer, RecordingReloadHandle) = Layer::new(RecordingLayer {
                calls: Arc::clone(&calls),
            });
            let dispatch = Dispatch::new(registry::Registry::default());
            let mut subscriber = registry::Registry::default();
            let context = layer::Context::<registry::Registry>::none();

            ensure_ok(reload_layer.on_register_dispatch(&dispatch), "reload layer forwards dispatch registration")?;
            reload_layer.on_layer(&mut subscriber);
            ensure(
                ensure_ok(reload_layer.register_callsite(&RELOAD_META), "reload layer forwards callsite registration")?.is_always(),
                "reload layer forwards callsite interest",
            )?;
            ensure(
                !ensure_ok(reload_layer.enabled(&RELOAD_META, context.clone()), "reload layer forwards enabled")?,
                "reload layer forwards enabled result",
            )?;
            ensure(
                !ensure_ok(reload_layer.event_enabled(event, context), "reload layer forwards event_enabled")?,
                "reload layer forwards event_enabled result",
            )?;
            ensure(
                ensure_ok(reload_layer.max_level_hint(), "reload layer forwards max-level hint")? == Some(LevelFilter::INFO),
                "reload layer forwards max-level hints",
            )?;
            ensure(
                reload_layer.downcast_ref_by_id(TypeId::of::<RecordingLayer>()).is_none(),
                "reload layers reject downcasts into lock-protected inner values",
            )?;

            let expected = vec![
                "on_register_dispatch",
                "on_layer",
                "register_callsite",
                "enabled",
                "event_enabled",
                "max_level_hint",
            ];
            ensure(*calls.lock() == expected, "reload layer forwards registration hooks once")
        })
    }

    #[test]
    fn reload_layer_forwards_span_and_event_lifecycle_hooks() -> Result<(), TestFailure> {
        with_reload_fixtures(|event, attrs, record, first_id, second_id| {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let (reload_layer, _handle): (RecordingReloadLayer, RecordingReloadHandle) = Layer::new(RecordingLayer {
                calls: Arc::clone(&calls),
            });
            let context = layer::Context::<registry::Registry>::none();

            ensure_ok(
                reload_layer.on_new_span(attrs, first_id, context.clone()),
                "reload layer forwards new spans",
            )?;
            ensure_ok(
                reload_layer.on_record(first_id, record, context.clone()),
                "reload layer forwards records",
            )?;
            ensure_ok(
                reload_layer.on_follows_from(first_id, second_id, context.clone()),
                "reload layer forwards follows-from",
            )?;
            ensure_ok(
                reload_layer.on_event(event, context.clone()),
                "reload layer forwards events",
            )?;
            ensure_ok(reload_layer.on_enter(first_id, context.clone()), "reload layer forwards enter")?;
            ensure_ok(reload_layer.on_exit(first_id, context.clone()), "reload layer forwards exit")?;
            ensure_ok(reload_layer.on_close(first_id, context.clone()), "reload layer forwards close")?;
            ensure_ok(
                reload_layer.on_id_change(first_id, second_id, context),
                "reload layer forwards ID changes",
            )?;

            let expected = vec![
                "on_new_span",
                "on_record",
                "on_follows_from",
                "on_event",
                "on_enter",
                "on_exit",
                "on_close",
                "on_id_change",
            ];
            ensure(*calls.lock() == expected, "reload layer forwards lifecycle hooks once")
        })
    }

    #[test]
    fn reload_layer_downcast_exposes_only_disabled_option_layer_marker() -> Result<(), TestFailure> {
        let (none_layer, _none_handle): (OptionalIdentityReloadLayer, OptionalIdentityReloadHandle) = Layer::new(None);
        ensure(
            none_layer
                .downcast_ref_by_id(TypeId::of::<layer::NoneLayerMarker>())
                .is_some(),
            "reload layer exposes the disabled optional layer marker",
        )?;
        ensure(
            none_layer
                .downcast_ref_by_id(TypeId::of::<Option<layer::Identity>>())
                .is_none(),
            "reload layer does not expose the lock-protected optional layer",
        )?;

        let (some_layer, _some_handle): (OptionalIdentityReloadLayer, OptionalIdentityReloadHandle) =
            Layer::new(Some(layer::Identity::default()));
        ensure(
            some_layer
                .downcast_ref_by_id(TypeId::of::<layer::NoneLayerMarker>())
                .is_none(),
            "enabled optional layers do not expose the disabled marker",
        )
    }

    #[test]
    fn reload_layer_forwards_filter_hooks() -> Result<(), TestFailure> {
        with_reload_fixtures(|_event, attrs, record, first_id, _second_id| {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let (filter, _handle): (FilterReloadLayer, FilterReloadHandle) = Layer::new(RecordingFilter {
                calls: Arc::clone(&calls),
            });
            let context = layer::Context::<registry::Registry>::none();

            ensure(
                ensure_ok(filter.callsite_enabled(&RELOAD_META), "reload filter forwards callsite interest")?.is_never(),
                "reload filter forwards callsite interest result",
            )?;
            ensure(
                !ensure_ok(
                    filter.enabled(&RELOAD_META, &context),
                    "reload filter forwards enabled",
                )?,
                "reload filter forwards enabled result",
            )?;
            ensure_ok(
                filter.on_new_span(attrs, first_id, context.clone()),
                "reload filter forwards new spans",
            )?;
            ensure_ok(
                filter.on_record(first_id, record, context.clone()),
                "reload filter forwards records",
            )?;
            ensure_ok(filter.on_enter(first_id, context.clone()), "reload filter forwards enter")?;
            ensure_ok(filter.on_exit(first_id, context.clone()), "reload filter forwards exit")?;
            ensure_ok(filter.on_close(first_id, context), "reload filter forwards close")?;
            ensure(
                ensure_ok(filter.max_level_hint(), "reload filter forwards max-level hint")? == Some(LevelFilter::WARN),
                "reload filter forwards max-level hint result",
            )?;

            let expected = vec![
                "callsite_enabled",
                "enabled",
                "on_new_span",
                "on_record",
                "on_enter",
                "on_exit",
                "on_close",
                "max_level_hint",
            ];
            ensure(*calls.lock() == expected, "reload filter forwards each filter hook once")
        })
    }
}
