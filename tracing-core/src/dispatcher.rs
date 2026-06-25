//! Dispatches trace events to [`Subscriber`]s.
//!
//! The _dispatcher_ is the component of the tracing system which is responsible
//! for forwarding trace data from the instrumentation points that generate it
//! to the subscriber that collects it.
//!
//! # Using the Trace Dispatcher
//!
//! Every thread in a program using `tracing` has a _default subscriber_. When
//! events occur, or spans are created, they are dispatched to the thread's
//! current subscriber.
//!
//! ## Setting the Default Subscriber
//!
//! By default, the current subscriber is an empty implementation that does
//! nothing. To use a subscriber implementation, it must be set as the default.
//! There are two methods for doing so: [`with_default`] and
//! [`set_global_default`]. `with_default` sets the default subscriber for the
//! duration of a scope, while `set_global_default` sets a default subscriber
//! for the entire process.
//!
//! To use either of these functions, we must first wrap our subscriber in a
//! [`Dispatch`], a cloneable, type-erased reference to a subscriber. For
//! example:
//! ```rust
//! # pub struct FooSubscriber;
//! # use tracing_core::{
//! #   dispatcher, Event, Metadata,
//! #   span::{Attributes, Id, Record},
//! #   subscriber::SubscriberResult,
//! # };
//! # impl tracing_core::Subscriber for FooSubscriber {
//! #   fn new_span(&self, _: &Attributes) -> SubscriberResult<Id> { Ok(Id::from_non_zero_u64(core::num::NonZeroU64::MIN)) }
//! #   fn record(&self, _: Id, _: &Record) -> SubscriberResult { Ok(()) }
//! #   fn event(&self, _: &Event) -> SubscriberResult { Ok(()) }
//! #   fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult { Ok(()) }
//! #   fn enabled(&self, _: &Metadata) -> SubscriberResult<bool> { Ok(false) }
//! #   fn enter(&self, _: Id) -> SubscriberResult { Ok(()) }
//! #   fn exit(&self, _: Id) -> SubscriberResult { Ok(()) }
//! # }
//! # impl FooSubscriber { fn new() -> Self { FooSubscriber } }
//! use dispatcher::Dispatch;
//!
//! let my_subscriber = FooSubscriber::new();
//! let my_dispatch = Dispatch::new(my_subscriber);
//! ```
//! Then, we can use [`with_default`] to set our `Dispatch` as the default for
//! the duration of a block:
//! ```rust
//! # pub struct FooSubscriber;
//! # use tracing_core::{
//! #   dispatcher, Event, Metadata,
//! #   span::{Attributes, Id, Record},
//! #   subscriber::SubscriberResult,
//! # };
//! # impl tracing_core::Subscriber for FooSubscriber {
//! #   fn new_span(&self, _: &Attributes) -> SubscriberResult<Id> { Ok(Id::from_non_zero_u64(core::num::NonZeroU64::MIN)) }
//! #   fn record(&self, _: Id, _: &Record) -> SubscriberResult { Ok(()) }
//! #   fn event(&self, _: &Event) -> SubscriberResult { Ok(()) }
//! #   fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult { Ok(()) }
//! #   fn enabled(&self, _: &Metadata) -> SubscriberResult<bool> { Ok(false) }
//! #   fn enter(&self, _: Id) -> SubscriberResult { Ok(()) }
//! #   fn exit(&self, _: Id) -> SubscriberResult { Ok(()) }
//! # }
//! # impl FooSubscriber { fn new() -> Self { FooSubscriber } }
//! # let my_subscriber = FooSubscriber::new();
//! # let my_dispatch = dispatcher::Dispatch::new(my_subscriber);
//! // no default subscriber
//!
//! # #[cfg(feature = "std")]
//! dispatcher::with_default(&my_dispatch, || {
//!     // my_subscriber is the default
//! });
//!
//! // no default subscriber again
//! ```
//! It's important to note that `with_default` will not propagate the current
//! thread's default subscriber to any threads spawned within the `with_default`
//! block. To propagate the default subscriber to new threads, either use
//! `with_default` from the new thread, or use `set_global_default`.
//!
//! As an alternative to `with_default`, we can use [`set_global_default`] to
//! set a `Dispatch` as the default for all threads, for the lifetime of the
//! program. For example:
//! ```rust
//! # pub struct FooSubscriber;
//! # use tracing_core::{
//! #   dispatcher, Event, Metadata,
//! #   span::{Attributes, Id, Record},
//! #   subscriber::SubscriberResult,
//! # };
//! # impl tracing_core::Subscriber for FooSubscriber {
//! #   fn new_span(&self, _: &Attributes) -> SubscriberResult<Id> { Ok(Id::from_non_zero_u64(core::num::NonZeroU64::MIN)) }
//! #   fn record(&self, _: Id, _: &Record) -> SubscriberResult { Ok(()) }
//! #   fn event(&self, _: &Event) -> SubscriberResult { Ok(()) }
//! #   fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult { Ok(()) }
//! #   fn enabled(&self, _: &Metadata) -> SubscriberResult<bool> { Ok(false) }
//! #   fn enter(&self, _: Id) -> SubscriberResult { Ok(()) }
//! #   fn exit(&self, _: Id) -> SubscriberResult { Ok(()) }
//! # }
//! # impl FooSubscriber { fn new() -> Self { FooSubscriber } }
//! # let my_subscriber = FooSubscriber::new();
//! # let my_dispatch = dispatcher::Dispatch::new(my_subscriber);
//! // no default subscriber
//!
//! let _result = dispatcher::set_global_default(my_dispatch);
//!
//! // `my_subscriber` is now the default
//! ```
//!
//! <pre class="ignore" style="white-space:normal;font:inherit;">
//!     <strong>Note</strong>:the thread-local scoped dispatcher
//!     (<a href="#fn.with_default"><code>with_default</code></a>) requires the
//!     Rust standard library. <code>no_std</code> users should use
//!     <a href="#fn.set_global_default"><code>set_global_default</code></a>
//!     instead.
//! </pre>
//!
//! ## Accessing the Default Subscriber
//!
//! A thread's current default subscriber can be accessed using the
//! [`get_default`] function, which executes a closure with a reference to the
//! currently default `Dispatch`. This is used primarily by `tracing`
//! instrumentation.

use crate::{
    Event, LevelFilter, Metadata, callsite, span,
    subscriber::{self, NoSubscriber, Subscriber, SubscriberResult},
};

use alloc::sync::{Arc, Weak};
use core::{
    any::Any,
    error, fmt,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[cfg(feature = "std")]
use std::{
    cell::{Cell, Ref, RefCell},
    sync::OnceLock,
};

/// `Dispatch` trace data to a [`Subscriber`].
#[derive(Clone)]
pub struct Dispatch {
    /// Subscriber target receiving forwarded trace data.
    subscriber: Kind<Arc<dyn Subscriber + Send + Sync>>,
}

/// `WeakDispatch` is a version of [`Dispatch`] that holds a non-owning reference
/// to a [`Subscriber`].
///
/// The `Subscriber` may be accessed by calling [`WeakDispatch::upgrade`],
/// which returns an `Option<Dispatch>`. If all [`Dispatch`] clones that point
/// at the `Subscriber` have been dropped, [`WeakDispatch::upgrade`] will return
/// `None`. Otherwise, it will return `Some(Dispatch)`.
///
/// A `WeakDispatch` may be created from a [`Dispatch`] by calling the
/// [`Dispatch::downgrade`] method. The primary use for creating a
/// [`WeakDispatch`] is to allow a `Subscriber` to hold a cyclical reference to
/// itself without creating a memory leak. See [here] for details.
///
/// This type is analogous to the [`std::sync::Weak`] type, but for a
/// [`Dispatch`] rather than an [`Arc`].
///
/// [`Arc`]: std::sync::Arc
/// [here]: Subscriber#avoiding-memory-leaks
#[derive(Clone)]
pub struct WeakDispatch {
    /// Weak subscriber target upgraded before dispatching.
    subscriber: Kind<Weak<dyn Subscriber + Send + Sync>>,
}

/// Subscriber storage for global and scoped dispatchers.
#[derive(Clone)]
enum Kind<T> {
    /// Process-wide subscriber that is never reference-counted.
    Global(&'static (dyn Subscriber + Send + Sync)),
    /// Reference-counted subscriber installed by a scoped dispatcher.
    Scoped(T),
}

#[cfg(feature = "std")]
std::thread_local! {
    static CURRENT_STATE: State = const {
        State {
            default: RefCell::new(None),
            can_enter: Cell::new(true),
        }
    };
}

/// Records whether any dispatcher has ever been installed.
static EXISTS: AtomicBool = AtomicBool::new(false);
/// Initialization state for the global dispatcher.
static GLOBAL_INIT: AtomicUsize = AtomicUsize::new(UNINITIALIZED);

#[cfg(feature = "std")]
/// Count of scoped dispatchers currently installed on this process.
static SCOPED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Global dispatcher has not been initialized.
const UNINITIALIZED: usize = 0;
/// A thread is initializing the global dispatcher.
const INITIALIZING: usize = 1;
/// Global dispatcher initialization completed.
const INITIALIZED: usize = 2;

#[cfg(feature = "std")]
/// Lazily initialized global dispatcher for `std` builds.
static GLOBAL_DISPATCH: OnceLock<Dispatch> = OnceLock::new();
#[cfg(not(feature = "std"))]
/// Lazily initialized global dispatcher for `no_std` builds.
static GLOBAL_DISPATCH: spin::Once<Dispatch> = spin::Once::new();
/// Fallback dispatcher used when no subscriber has been installed.
static NONE: Dispatch = Dispatch {
    subscriber: Kind::Global(&NO_SUBSCRIBER),
};
/// Shared subscriber that drops every span and event.
static NO_SUBSCRIBER: NoSubscriber = NoSubscriber::new();

/// The dispatch state of a thread.
#[cfg(feature = "std")]
struct State {
    /// This thread's current default dispatcher.
    default: RefCell<Option<Dispatch>>,
    /// Whether or not we can currently begin dispatching a trace event.
    ///
    /// This is set to `false` when functions such as `enter`, `exit`, `event`,
    /// and `new_span` are called on this thread's default dispatcher, to
    /// prevent further trace events triggered inside those functions from
    /// creating an infinite recursion. When we finish handling a dispatch, this
    /// is set back to `true`.
    can_enter: Cell<bool>,
}

/// While this guard is active, additional calls to subscriber functions on
/// the default dispatcher will not be able to access the dispatch context.
/// Dropping the guard will allow the dispatch context to be re-entered.
#[cfg(feature = "std")]
struct Entered<'a>(&'a State);

/// A guard that resets the current default dispatcher to the prior
/// default dispatcher when dropped.
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
#[derive(Debug)]
pub struct DefaultGuard(Option<Dispatch>);

/// Sets this dispatch as the default for the duration of a closure.
///
/// The default dispatcher is used when creating a new [span] or
/// [`Event`].
///
/// <pre class="ignore" style="white-space:normal;font:inherit;">
///     <strong>Note</strong>: This function required the Rust standard library.
///     <code>no_std</code> users should use <a href="fn.set_global_default.html">
///     <code>set_global_default</code></a> instead.
/// </pre>
///
/// [span]: super::span
/// [`Subscriber`]: super::subscriber::Subscriber
/// [`Event`]: super::event::Event
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub fn with_default<T>(dispatcher: &Dispatch, f: impl FnOnce() -> T) -> T {
    // When this guard is dropped, the default dispatcher will be reset to the
    // prior default. Using this (rather than simply resetting after calling
    // `f`) ensures that we always reset to the prior dispatcher even if `f`
    // panics.
    let _guard = set_default(dispatcher);
    f()
}

/// Sets the dispatch as the default dispatch for the duration of the lifetime
/// of the returned `DefaultGuard`.
///
/// <pre class="ignore" style="white-space:normal;font:inherit;">
///     <strong>Note</strong>: This function required the Rust standard library.
///     <code>no_std</code> users should use <a href="fn.set_global_default.html">
///     <code>set_global_default</code></a> instead.
/// </pre>
///
/// [`set_global_default`]: set_global_default
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
#[allow(
    clippy::single_call_fn,
    reason = "public scoped-default guard API is intentionally callable outside with_default"
)]
#[must_use = "Dropping the guard unregisters the dispatcher."]
pub fn set_default(dispatcher: &Dispatch) -> DefaultGuard {
    // When this guard is dropped, the default dispatcher will be reset to the
    // prior default. Using this ensures that we always reset to the prior
    // dispatcher even if the thread calling this function panics.
    let prior = CURRENT_STATE
        .try_with(|state| {
            state.can_enter.set(true);
            state.default.replace(Some(dispatcher.clone()))
        })
        .ok()
        .flatten();
    EXISTS.store(true, Ordering::Release);
    let _previous_scoped_count = SCOPED_COUNT.fetch_add(1, Ordering::Release);
    DefaultGuard(prior)
}

/// Sets this dispatch as the global default for the duration of the entire program.
/// Will be used as a fallback if no thread-local dispatch has been set in a thread
/// (using `with_default`.)
///
/// Can only be set once; subsequent attempts to set the global default will fail.
/// Returns `Err` if the global default has already been set.
///
/// # Errors
///
/// Returns [`SetGlobalDefaultError`] if a global default dispatcher was already
/// installed by an earlier call.
///
/// <div class="example-wrap" style="display:inline-block"><pre class="compile_fail" style="white-space:normal;font:inherit;">
///     <strong>Warning</strong>: In general, libraries should <em>not</em> call
///     <code>set_global_default()</code>! Doing so will cause conflicts when
///     executables that depend on the library try to set the default later.
/// </pre></div>
///
/// [span]: super::span
/// [`Subscriber`]: super::subscriber::Subscriber
/// [`Event`]: super::event::Event
pub fn set_global_default(dispatcher: Dispatch) -> Result<(), SetGlobalDefaultError> {
    // if `compare_exchange` returns Result::Ok(_), then `new` has been set and
    // `current`—now the prior value—has been returned in the `Ok()` branch.
    if GLOBAL_INIT
        .compare_exchange(
            UNINITIALIZED,
            INITIALIZING,
            Ordering::SeqCst,
            Ordering::SeqCst,
        )
        .is_ok()
    {
        #[cfg(feature = "std")]
        if GLOBAL_DISPATCH.set(dispatcher).is_err() {
            GLOBAL_INIT.store(INITIALIZED, Ordering::SeqCst);
            return Err(SetGlobalDefaultError { _no_construct: () });
        }
        #[cfg(not(feature = "std"))]
        let _dispatch = GLOBAL_DISPATCH.call_once(|| dispatcher);
        GLOBAL_INIT.store(INITIALIZED, Ordering::SeqCst);
        EXISTS.store(true, Ordering::Release);
        Ok(())
    } else {
        Err(SetGlobalDefaultError { _no_construct: () })
    }
}

/// Returns true if a `tracing` dispatcher has ever been set.
///
/// This may be used to completely elide trace points if tracing is not in use
/// at all or has yet to be initialized.
#[doc(hidden)]
#[inline]
pub fn has_been_set() -> bool {
    EXISTS.load(Ordering::Relaxed)
}

/// Returned if setting the global dispatcher fails.
#[derive(Copy, Clone)]
pub struct SetGlobalDefaultError {
    /// Prevents constructing this error outside the crate.
    _no_construct: (),
}

impl fmt::Debug for SetGlobalDefaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SetGlobalDefaultError")
            .field(&Self::MESSAGE)
            .finish()
    }
}

impl fmt::Display for SetGlobalDefaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(Self::MESSAGE)
    }
}

impl error::Error for SetGlobalDefaultError {}

impl SetGlobalDefaultError {
    /// Display text shared by every formatter implementation.
    const MESSAGE: &'static str = "a global default trace dispatcher has already been set";
}

/// Executes a closure with a reference to this thread's current [dispatcher].
///
/// Note that calls to `get_default` should not be nested; if this function is
/// called while inside of another `get_default`, that closure will be provided
/// with `Dispatch::none` rather than the previously set dispatcher.
///
/// [dispatcher]: super::dispatcher::Dispatch
#[cfg(feature = "std")]
pub fn get_default<T, F>(mut f: F) -> T
where
    F: FnMut(&Dispatch) -> T,
{
    if SCOPED_COUNT.load(Ordering::Acquire) == 0 {
        // fast path if no scoped dispatcher has been set; just use the global
        // default.
        return f(get_global());
    }

    CURRENT_STATE
        .try_with(|state| {
            if let Some(entered) = state.enter() {
                if let Some(current) = entered.current() {
                    return f(&current);
                }
                return f(get_global());
            }

            f(&NONE)
        })
        .unwrap_or_else(|_| f(&NONE))
}

/// Executes a closure with a reference to this thread's current [dispatcher].
///
/// Note that calls to `get_default` should not be nested; if this function is
/// called while inside of another `get_default`, that closure will be provided
/// with `Dispatch::none` rather than the previously set dispatcher.
///
/// [dispatcher]: super::dispatcher::Dispatch
#[cfg(feature = "std")]
#[doc(hidden)]
#[inline(never)]
pub fn get_current<T>(f: impl FnOnce(&Dispatch) -> T) -> Option<T> {
    if SCOPED_COUNT.load(Ordering::Acquire) == 0 {
        // fast path if no scoped dispatcher has been set; just use the global
        // default.
        return Some(f(get_global()));
    }

    CURRENT_STATE
        .try_with(|state| {
            let entered = state.enter()?;
            if let Some(current) = entered.current() {
                Some(f(&current))
            } else {
                Some(f(get_global()))
            }
        })
        .ok()?
}

/// Executes a closure with a reference to the current [dispatcher].
///
/// [dispatcher]: super::dispatcher::Dispatch
#[cfg(not(feature = "std"))]
#[doc(hidden)]
pub fn get_current<T>(f: impl FnOnce(&Dispatch) -> T) -> Option<T> {
    Some(f(get_global()))
}

/// Executes a closure with a reference to the current [dispatcher].
///
/// [dispatcher]: super::dispatcher::Dispatch
#[cfg(not(feature = "std"))]
pub fn get_default<T, F>(mut f: F) -> T
where
    F: FnMut(&Dispatch) -> T,
{
    f(&get_global())
}

#[inline]
/// Returns the installed global dispatcher or the no-subscriber fallback.
fn get_global() -> &'static Dispatch {
    if GLOBAL_INIT.load(Ordering::SeqCst) != INITIALIZED {
        return &NONE;
    }

    GLOBAL_DISPATCH.get().unwrap_or(&NONE)
}

#[cfg(feature = "std")]
/// Weak registrar stored in the callsite dispatch registry.
pub(crate) struct Registrar(Kind<Weak<dyn Subscriber + Send + Sync>>);

impl Dispatch {
    /// Returns a new `Dispatch` that discards events and spans.
    #[inline]
    #[must_use]
    pub fn none() -> Self {
        Self {
            subscriber: Kind::Global(&NO_SUBSCRIBER),
        }
    }

    /// Returns a `Dispatch` that forwards to the given [`Subscriber`].
    ///
    /// [`Subscriber`]: super::subscriber::Subscriber
    #[allow(
        clippy::single_call_fn,
        reason = "public Dispatch constructor is the documented subscriber erasure entrypoint"
    )]
    pub fn new<S>(subscriber: S) -> Self
    where
        S: Subscriber + Send + Sync + 'static,
    {
        let me = Self {
            subscriber: Kind::Scoped(Arc::new(subscriber)),
        };
        callsite::register_dispatch(&me);
        me
    }

    #[cfg(feature = "std")]
    /// Returns a weak registrar used for future callsite interest rebuilds.
    pub(crate) fn registrar(&self) -> Registrar {
        Registrar(self.subscriber.downgrade())
    }

    /// Creates a [`WeakDispatch`] from this `Dispatch`.
    ///
    /// A [`WeakDispatch`] is similar to a [`Dispatch`], but it does not prevent
    /// the underlying [`Subscriber`] from being dropped. Instead, it only permits
    /// access while other references to the `Subscriber` exist. This is equivalent
    /// to the standard library's [`Arc::downgrade`] method, but for `Dispatch`
    /// rather than `Arc`.
    ///
    /// The primary use for creating a [`WeakDispatch`] is to allow a `Subscriber`
    /// to hold a cyclical reference to itself without creating a memory leak.
    /// See [here] for details.
    ///
    /// [`Arc::downgrade`]: std::sync::Arc::downgrade
    /// [here]: Subscriber#avoiding-memory-leaks
    #[must_use]
    pub fn downgrade(&self) -> WeakDispatch {
        WeakDispatch {
            subscriber: self.subscriber.downgrade(),
        }
    }

    #[inline]
    /// Returns the subscriber backing this dispatch.
    pub(crate) fn subscriber(&self) -> &(dyn Subscriber + Send + Sync) {
        match self.subscriber {
            Kind::Global(subscriber) => subscriber,
            Kind::Scoped(ref subscriber) => subscriber.as_ref(),
        }
    }

    /// Registers a new callsite with this subscriber, returning whether or not
    /// the subscriber is interested in being notified about the callsite.
    ///
    /// This calls the [`register_callsite`] function on the [`Subscriber`]
    /// that this `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot evaluate or record its
    /// interest in the callsite.
    ///
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`register_callsite`]: super::subscriber::Subscriber::register_callsite
    #[inline]
    pub fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<subscriber::Interest> {
        self.subscriber().register_callsite(metadata)
    }

    /// Returns the highest [verbosity level][level] that this [`Subscriber`] will
    /// enable, or `None`, if the subscriber does not implement level-based
    /// filtering or chooses not to implement this method.
    ///
    /// This calls the [`max_level_hint`] function on the [`Subscriber`]
    /// that this `Dispatch` forwards to.
    ///
    /// [level]: super::Level
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`register_callsite`]: super::subscriber::Subscriber::max_level_hint
    // TODO(eliza): consider making this a public API?
    #[inline]
    pub(crate) fn max_level_hint(&self) -> Option<LevelFilter> {
        self.subscriber().max_level_hint()
    }

    /// Record the construction of a new span, returning a new [ID] for the
    /// span being constructed.
    ///
    /// This calls the [`new_span`] function on the [`Subscriber`] that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot process the new span.
    ///
    /// [ID]: super::span::Id
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`new_span`]: super::subscriber::Subscriber::new_span
    #[inline]
    pub fn new_span(&self, span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
        self.subscriber().new_span(span)
    }

    /// Record a set of values on a span.
    ///
    /// This calls the [`record`] function on the [`Subscriber`] that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot record the provided
    /// field values.
    ///
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`record`]: super::subscriber::Subscriber::record
    #[inline]
    pub fn record(&self, span: span::Id, values: &span::Record<'_>) -> SubscriberResult {
        self.subscriber().record(span, values)
    }

    /// Adds an indication that `span` follows from the span with the id
    /// `follows`.
    ///
    /// This calls the [`record_follows_from`] function on the [`Subscriber`]
    /// that this `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot record the causal
    /// relationship between the spans.
    ///
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`record_follows_from`]: super::subscriber::Subscriber::record_follows_from
    #[inline]
    pub fn record_follows_from(&self, span: span::Id, follows: span::Id) -> SubscriberResult {
        self.subscriber().record_follows_from(span, follows)
    }

    /// Returns true if a span with the specified [metadata] would be
    /// recorded.
    ///
    /// This calls the [`enabled`] function on the [`Subscriber`] that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot evaluate whether the
    /// metadata should be enabled.
    ///
    /// [metadata]: super::metadata::Metadata
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`enabled`]: super::subscriber::Subscriber::enabled
    #[inline]
    pub fn enabled(&self, metadata: &Metadata<'_>) -> SubscriberResult<bool> {
        self.subscriber().enabled(metadata)
    }

    /// Records that an [`Event`] has occurred.
    ///
    /// This calls the [`event`] function on the [`Subscriber`] that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot evaluate or record the
    /// event.
    ///
    /// [`Event`]: super::event::Event
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`event`]: super::subscriber::Subscriber::event
    #[inline]
    pub fn event(&self, event: &Event<'_>) -> SubscriberResult {
        let subscriber = self.subscriber();
        if subscriber.event_enabled(event)? {
            subscriber.event(event)?;
        }
        Ok(())
    }

    /// Records that a span has been `can_enter`.
    ///
    /// This calls the [`enter`] function on the [`Subscriber`] that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot process the span-enter
    /// notification.
    ///
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`enter`]: super::subscriber::Subscriber::enter
    pub fn enter(&self, span: span::Id) -> SubscriberResult {
        self.subscriber().enter(span)
    }

    /// Records that a span has been exited.
    ///
    /// This calls the [`exit`] function on the [`Subscriber`] that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot process the span-exit
    /// notification.
    ///
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`exit`]: super::subscriber::Subscriber::exit
    pub fn exit(&self, span: span::Id) -> SubscriberResult {
        self.subscriber().exit(span)
    }

    /// Notifies the subscriber that a [span ID] has been cloned.
    ///
    /// This function must only be called with span IDs that were returned by
    /// this `Dispatch`'s [`new_span`] function. The `tracing` crate upholds
    /// this guarantee and any other libraries implementing instrumentation APIs
    /// must as well.
    ///
    /// This calls the [`clone_span`] function on the `Subscriber` that this
    /// `Dispatch` forwards to.
    ///
    /// [span ID]: super::span::Id
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`clone_span`]: super::subscriber::Subscriber::clone_span
    /// [`new_span`]: super::subscriber::Subscriber::new_span
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot clone its span handle
    /// state.
    #[inline]
    pub fn clone_span(&self, id: span::Id) -> SubscriberResult<span::Id> {
        self.subscriber().clone_span(id)
    }

    /// Notifies the subscriber that a [span ID] has been dropped, and returns
    /// `true` if there are now 0 IDs referring to that span.
    ///
    /// This function must only be called with span IDs that were returned by
    /// this `Dispatch`'s [`new_span`] function. The `tracing` crate upholds
    /// this guarantee and any other libraries implementing instrumentation APIs
    /// must as well.
    ///
    /// This calls the [`try_close`] function on the [`Subscriber`] that this
    ///  `Dispatch` forwards to.
    ///
    /// [span ID]: super::span::Id
    /// [`Subscriber`]: super::subscriber::Subscriber
    /// [`try_close`]: super::subscriber::Subscriber::try_close
    /// [`new_span`]: super::subscriber::Subscriber::new_span
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber cannot update its span-close
    /// state.
    pub fn try_close(&self, id: span::Id) -> SubscriberResult<bool> {
        self.subscriber().try_close(id)
    }

    /// Returns a type representing this subscriber's view of the current span.
    ///
    /// This calls the [`current`] function on the `Subscriber` that this
    /// `Dispatch` forwards to.
    ///
    /// # Errors
    ///
    /// Returns an error if the wrapped subscriber tracks current-span state but
    /// cannot query it.
    ///
    /// [`current`]: super::subscriber::Subscriber::current_span
    #[inline]
    pub fn current_span(&self) -> SubscriberResult<span::Current> {
        self.subscriber().current_span()
    }

    /// Returns `true` if this `Dispatch` forwards to a `Subscriber` of type
    /// `T`.
    #[inline]
    #[must_use]
    pub fn is<T: Any>(&self) -> bool {
        <dyn Subscriber + Send + Sync>::is::<T>(self.subscriber())
    }

    /// Returns some reference to the `Subscriber` this `Dispatch` forwards to
    /// if it is of type `T`, or `None` if it isn't.
    #[inline]
    #[must_use]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        <dyn Subscriber + Send + Sync>::downcast_ref(self.subscriber())
    }
}

impl Default for Dispatch {
    /// Returns the current default dispatcher
    fn default() -> Self {
        get_default(Clone::clone)
    }
}

impl fmt::Debug for Dispatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.subscriber {
            Kind::Scoped(ref subscriber) => f
                .debug_tuple("Dispatch::Scoped")
                .field(&format_args!("{subscriber:p}"))
                .finish(),
            Kind::Global(subscriber) => f
                .debug_tuple("Dispatch::Global")
                .field(&format_args!("{subscriber:p}"))
                .finish(),
        }
    }
}

impl<S> From<S> for Dispatch
where
    S: Subscriber + Send + Sync + 'static,
{
    #[inline]
    fn from(subscriber: S) -> Self {
        Self::new(subscriber)
    }
}

// === impl WeakDispatch ===

impl WeakDispatch {
    /// Attempts to upgrade this `WeakDispatch` to a [`Dispatch`].
    ///
    /// Returns `None` if the referenced `Dispatch` has already been dropped.
    ///
    /// ## Examples
    ///
    /// ```
    /// # use tracing_core::subscriber::NoSubscriber;
    /// # use tracing_core::dispatcher::Dispatch;
    /// let strong = Dispatch::new(NoSubscriber::default());
    /// let weak = strong.downgrade();
    ///
    /// // The strong here keeps it alive, so we can still access the object.
    /// if weak.upgrade().is_none() {
    ///     return Err("weak dispatch should upgrade while the strong dispatch is alive".into());
    /// }
    ///
    /// drop(strong); // But not any more.
    /// if weak.upgrade().is_some() {
    ///     return Err("weak dispatch should not upgrade after the strong dispatch is dropped".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn upgrade(&self) -> Option<Dispatch> {
        self.subscriber
            .upgrade()
            .map(|subscriber| Dispatch { subscriber })
    }
}

impl fmt::Debug for WeakDispatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.subscriber {
            Kind::Scoped(ref subscriber) => f
                .debug_tuple("WeakDispatch::Scoped")
                .field(&format_args!("{subscriber:p}"))
                .finish(),
            Kind::Global(subscriber) => f
                .debug_tuple("WeakDispatch::Global")
                .field(&format_args!("{subscriber:p}"))
                .finish(),
        }
    }
}

#[cfg(feature = "std")]
impl Registrar {
    /// Upgrades this registrar into a live dispatcher.
    #[allow(
        clippy::single_call_fn,
        reason = "keep registrar upgrade logic inside dispatcher while callsite filters live dispatchers"
    )]
    pub(crate) fn upgrade(&self) -> Option<Dispatch> {
        self.0.upgrade().map(|subscriber| Dispatch { subscriber })
    }
}

// ===== impl State =====

impl Kind<Arc<dyn Subscriber + Send + Sync>> {
    /// Downgrades the reference-counted subscriber without affecting globals.
    fn downgrade(&self) -> Kind<Weak<dyn Subscriber + Send + Sync>> {
        match *self {
            Kind::Global(subscriber) => Kind::Global(subscriber),
            Kind::Scoped(ref subscriber) => Kind::Scoped(Arc::downgrade(subscriber)),
        }
    }
}

impl Kind<Weak<dyn Subscriber + Send + Sync>> {
    /// Upgrades a weak subscriber handle without affecting globals.
    fn upgrade(&self) -> Option<Kind<Arc<dyn Subscriber + Send + Sync>>> {
        match *self {
            Kind::Global(subscriber) => Some(Kind::Global(subscriber)),
            Kind::Scoped(ref subscriber) => Some(Kind::Scoped(subscriber.upgrade()?)),
        }
    }
}

// ===== impl State =====

#[cfg(feature = "std")]
impl State {
    #[inline]
    /// Enters dispatch if the thread is not already dispatching.
    const fn enter(&self) -> Option<Entered<'_>> {
        if self.can_enter.replace(false) {
            Some(Entered(self))
        } else {
            None
        }
    }
}

// ===== impl Entered =====

#[cfg(feature = "std")]
impl<'a> Entered<'a> {
    #[inline]
    /// Borrows the scoped dispatcher guarded by this entered state.
    fn current(&self) -> Option<Ref<'a, Dispatch>> {
        let default = self.0.default.try_borrow().ok()?;
        Ref::filter_map(default, Option::as_ref).ok()
    }
}

#[cfg(feature = "std")]
impl Drop for Entered<'_> {
    #[inline]
    fn drop(&mut self) {
        self.0.can_enter.set(true);
    }
}

// ===== impl DefaultGuard =====

#[cfg(feature = "std")]
impl Drop for DefaultGuard {
    #[inline]
    fn drop(&mut self) {
        // Replace the dispatcher and then drop the old one outside
        // of the thread-local context. Dropping the dispatch may
        // lead to the drop of a subscriber which, in the process,
        // could then also attempt to access the same thread local
        // state -- causing a clash.
        let prev = CURRENT_STATE.try_with(|state| state.default.replace(self.0.take()));
        let _previous_scoped_count = SCOPED_COUNT.fetch_sub(1, Ordering::Release);
        drop(prev);
    }
}

#[cfg(test)]
mod test {
    #[cfg(feature = "std")]
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(feature = "std")]
    use core::num::NonZeroU64;

    use super::*;
    #[cfg(feature = "std")]
    use crate::{
        callsite::Callsite,
        metadata::{Kind, Level, Metadata},
        subscriber::Interest,
    };
    #[cfg(feature = "std")]
    use strict_test_support::ensure_eq;
    use strict_test_support::{TestFailure, ensure};

    #[test]
    fn dispatch_is() -> Result<(), TestFailure> {
        let dispatcher = Dispatch::new(NoSubscriber::default());
        ensure(
            dispatcher.is::<NoSubscriber>(),
            "dispatcher type is NoSubscriber",
        )
    }

    #[test]
    fn dispatch_downcasts() -> Result<(), TestFailure> {
        let dispatcher = Dispatch::new(NoSubscriber::default());
        ensure(
            dispatcher.downcast_ref::<NoSubscriber>().is_some(),
            "dispatcher downcasts to NoSubscriber",
        )
    }

    #[cfg(feature = "std")]
    struct TestCallsite;
    #[cfg(feature = "std")]
    static TEST_CALLSITE: TestCallsite = TestCallsite;
    #[cfg(feature = "std")]
    static TEST_META: Metadata<'static> = metadata! {
        name: "test",
        target: module_path!(),
        level: Level::DEBUG,
        fields: &[],
        callsite: &TEST_CALLSITE,
        kind: Kind::EVENT
    };

    #[cfg(feature = "std")]
    impl Callsite for TestCallsite {
        fn set_interest(&self, _: Interest) {}
        fn metadata(&self) -> &Metadata<'_> {
            &TEST_META
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn events_dont_infinite_loop() -> Result<(), TestFailure> {
        static EVENTS: AtomicUsize = AtomicUsize::new(0);

        // This test ensures that an event triggered within a subscriber
        // won't cause an infinite loop of events.
        struct TestSubscriber;
        impl Subscriber for TestSubscriber {
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
                let _previous_events = EVENTS.fetch_add(1, Ordering::Relaxed);
                Event::dispatch(&TEST_META, &TEST_META.fields().value_set(&[]));
                Ok(())
            }

            fn enter(&self, _: span::Id) -> SubscriberResult {
                Ok(())
            }

            fn exit(&self, _: span::Id) -> SubscriberResult {
                Ok(())
            }
        }

        with_default(&Dispatch::new(TestSubscriber), || {
            Event::dispatch(&TEST_META, &TEST_META.fields().value_set(&[]));
        });
        ensure_eq(
            &EVENTS.load(Ordering::Relaxed),
            &1_usize,
            "event method is called once",
        )
    }

    #[test]
    #[cfg(feature = "std")]
    fn spans_dont_infinite_loop() -> Result<(), TestFailure> {
        static NEW_SPANS: AtomicUsize = AtomicUsize::new(0);

        // This test ensures that a span created within a subscriber
        // won't cause an infinite loop of new spans.

        fn mk_span() {
            let _span = get_default(|current| {
                current.new_span(&span::Attributes::new(
                    &TEST_META,
                    &TEST_META.fields().value_set(&[]),
                ))
            });
        }

        struct TestSubscriber;
        impl Subscriber for TestSubscriber {
            fn enabled(&self, _: &Metadata<'_>) -> SubscriberResult<bool> {
                Ok(true)
            }

            fn new_span(&self, _: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
                let _previous_new_spans = NEW_SPANS.fetch_add(1, Ordering::Relaxed);
                mk_span();
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

        with_default(&Dispatch::new(TestSubscriber), mk_span);
        ensure_eq(
            &NEW_SPANS.load(Ordering::Relaxed),
            &1_usize,
            "new_span method is called once",
        )
    }

    #[test]
    fn default_no_subscriber() -> Result<(), TestFailure> {
        let default_dispatcher = Dispatch::default();
        ensure(
            default_dispatcher.is::<NoSubscriber>(),
            "default dispatcher is NoSubscriber",
        )
    }

    #[cfg(feature = "std")]
    #[test]
    fn default_dispatch() -> Result<(), TestFailure> {
        struct TestSubscriber;
        impl Subscriber for TestSubscriber {
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
        let guard = set_default(&Dispatch::new(TestSubscriber));
        let scoped_dispatcher = Dispatch::default();
        ensure(
            scoped_dispatcher.is::<TestSubscriber>(),
            "default dispatcher follows scoped subscriber",
        )?;

        drop(guard);
        let reset_dispatcher = Dispatch::default();
        ensure(
            reset_dispatcher.is::<NoSubscriber>(),
            "default dispatcher resets to NoSubscriber",
        )
    }
}
