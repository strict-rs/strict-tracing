//! Feature-gated subscriber fixtures for exercising `tracing-core` in tests.
//!
//! This module is compiled only when the off-by-default `test-util` feature is
//! enabled, and it exists purely to support the crate's own test suite and the
//! callsite-registration regression tests in the wider workspace. It is **not**
//! part of the product API; enable `test-util` from `dev-dependencies` only.
//!
//! Two fixtures live here:
//!
//! * [`CallsiteTrackingSubscriber`] — a lightweight [`Subscriber`] that counts every
//!   `register_callsite` notification it receives, remembers the callsite it registered most
//!   recently, and flags events that arrive on a callsite other than that one. Every other
//!   notification is accepted as a no-op, so it also stands in wherever an inert subscriber fixture
//!   is needed.
//! * [`NoOpSubscriber`] — an inert [`Subscriber`] whose type identity is fixed by a marker
//!   (`Primary` or `Secondary`), so dispatch-identity tests can install one and assert *which*
//!   subscriber a dispatcher currently holds.
//!
//! Construct a [`CallsiteTrackingSubscriber`] with
//! [`CallsiteTrackingSubscriber::new`], take a [`CallsiteTrackingHandle`] with
//! [`CallsiteTrackingSubscriber::handle`] before installing it, then query the
//! handle after driving the code under test:
//!
//! ```rust
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_core::Dispatch;
//! use tracing_core::Event;
//! use tracing_core::Kind;
//! use tracing_core::Level;
//! use tracing_core::Metadata;
//! use tracing_core::callsite::Callsite as _;
//! use tracing_core::callsite::DefaultCallsite;
//! use tracing_core::callsite::Identifier;
//! use tracing_core::dispatcher;
//! use tracing_core::field::FieldSet;
//! use tracing_core::field::Value;
//! use tracing_core::metadata::SourceLocation;
//! use tracing_core::test_util::CallsiteTrackingSubscriber;
//!
//! static CALLSITE: DefaultCallsite = {
//!   static META: Metadata<'static> = Metadata::new(
//!     "example event",
//!     "tracing_core::test_util::example",
//!     Level::INFO,
//!     &SourceLocation::empty(),
//!     &FieldSet::new(&["message"], Identifier(&CALLSITE)),
//!     Kind::EVENT,
//!   );
//!   DefaultCallsite::new(&META)
//! };
//!
//! let subscriber = CallsiteTrackingSubscriber::new();
//! let handle = subscriber.handle();
//!
//! dispatcher::with_default(
//!   &Dispatch::new(subscriber),
//!   || -> Result<(), strict_test_support::TestFailure> {
//!     let _interest = CALLSITE.interest();
//!     let meta = CALLSITE.metadata();
//!     let field = strict_test_support::ensure_some(
//!       meta.fields().field("message"),
//!       "message field is registered",
//!     )?;
//!     let message = "drives callsite registration";
//!     let message_value: &dyn Value = &message;
//!     let values = [(&field, Some(message_value))];
//!     let value_set = meta.fields().value_set(&values);
//!     Event::dispatch(meta, &value_set);
//!     Ok(())
//!   },
//! )?;
//!
//! strict_test_support::ensure(handle.was_registered(), "the event callsite was registered")?;
//! strict_test_support::ensure(
//!   !handle.saw_callsite_mismatch(),
//!   "the event arrived after its callsite registration",
//! )?;
//! # Ok(())
//! # }
//! ```
//!
//! Constructor options unify the variations the workspace's callsite-registry
//! regression tests rely on: [`with_register_delay`] stalls inside
//! `register_callsite` to provoke registration races, and
//! [`with_event_on_register`] re-enters the active dispatcher by emitting an
//! event while a callsite is being registered.
//!
//! [`register_callsite`]: Subscriber::register_callsite
//! [`Subscriber`]: crate::subscriber::Subscriber
//! [`with_register_delay`]: CallsiteTrackingSubscriber::with_register_delay
//! [`with_event_on_register`]: CallsiteTrackingSubscriber::with_event_on_register

use core::marker::PhantomData;
use core::num::NonZeroU64;
use core::ptr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crate::Event;
use crate::Interest;
use crate::Kind;
use crate::Level;
use crate::Metadata;
use crate::Subscriber;
use crate::SubscriberResult;
use crate::callsite::Callsite as _;
use crate::callsite::DefaultCallsite;
use crate::callsite::Identifier;
use crate::field::FieldSet;
use crate::metadata::SourceLocation;
use crate::span;

/// Registration facts shared between a running tracker and its handles.
#[derive(Debug, Default)]
struct TrackerState {
  /// Number of `register_callsite` notifications observed.
  register_count:    AtomicUsize,
  /// Pointer identity of the most recently registered callsite metadata.
  last_callsite:     AtomicPtr<()>,
  /// Whether an event arrived on a callsite other than the last registered one.
  callsite_mismatch: AtomicBool,
}

/// A [`Subscriber`] that records callsite registrations for later inspection.
///
/// Beyond the registration tracking described in the [module
/// documentation](self), the subscriber behaves as an inert fixture: it
/// enables every callsite, reports [`Interest::always`] from
/// `register_callsite`, assigns every new span the ID `1`, and accepts all
/// other notifications without effect.
///
/// [`Subscriber`]: crate::subscriber::Subscriber
#[derive(Debug)]
pub struct CallsiteTrackingSubscriber {
  /// Registration facts shared with every [`CallsiteTrackingHandle`].
  state:             Arc<TrackerState>,
  /// Pause applied inside `register_callsite` before the registration is recorded.
  register_delay:    Duration,
  /// Whether `register_callsite` re-enters the active dispatcher with an event.
  event_on_register: bool,
}

/// A handle used to inspect the registrations a [`CallsiteTrackingSubscriber`] observed.
///
/// Handles share state with the subscriber they were taken from, so they stay
/// valid after the subscriber has been installed into a dispatcher.
#[derive(Clone, Debug)]
pub struct CallsiteTrackingHandle {
  /// Registration facts shared with the owning subscriber.
  state: Arc<TrackerState>,
}

impl CallsiteTrackingSubscriber {
  /// Creates a tracker with no registration delay and no re-entrant emission.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new();
  /// let handle = subscriber.handle();
  ///
  /// strict_test_support::ensure(!handle.was_registered(), "nothing has registered yet")?;
  /// # Ok(())
  /// # }
  /// ```
  #[allow(
    clippy::single_call_fn,
    reason = "public test-util constructor is consumed by downstream callsite registration regression tests"
  )]
  #[must_use]
  pub fn new() -> Self {
    Self {
      state:             Arc::new(TrackerState::default()),
      register_delay:    Duration::ZERO,
      event_on_register: false,
    }
  }

  /// Stalls `register_callsite` for `delay` before the registration is recorded.
  ///
  /// Registration-race regression tests use this to control the interleaving
  /// of concurrent `register_callsite` and `event` notifications across
  /// threads.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use std::time::Duration;
  ///
  /// use tracing_core::Kind;
  /// use tracing_core::Level;
  /// use tracing_core::Metadata;
  /// use tracing_core::Subscriber as _;
  /// use tracing_core::callsite::Callsite as _;
  /// use tracing_core::callsite::DefaultCallsite;
  /// use tracing_core::callsite::Identifier;
  /// use tracing_core::field::FieldSet;
  /// use tracing_core::metadata::SourceLocation;
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// static CALLSITE: DefaultCallsite = {
  ///   static META: Metadata<'static> = Metadata::new(
  ///     "delayed event",
  ///     "tracing_core::test_util::example",
  ///     Level::INFO,
  ///     &SourceLocation::empty(),
  ///     &FieldSet::new(&["message"], Identifier(&CALLSITE)),
  ///     Kind::EVENT,
  ///   );
  ///   DefaultCallsite::new(&META)
  /// };
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new().with_register_delay(Duration::from_micros(50));
  /// let handle = subscriber.handle();
  ///
  /// let _interest = subscriber.register_callsite(CALLSITE.metadata());
  ///
  /// strict_test_support::ensure(
  ///   handle.was_registered(),
  ///   "the delayed registration is recorded",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub const fn with_register_delay(mut self, delay: Duration) -> Self {
    self.register_delay = delay;
    self
  }

  /// Emits an event through the active dispatcher while `register_callsite` runs.
  ///
  /// This mirrors the re-entrant subscriber used by the callsite deadlock
  /// regression test: registering a callsite triggers instrumentation of its
  /// own, which the callsite registry must service without deadlocking.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new().with_event_on_register();
  /// let handle = subscriber.handle();
  ///
  /// strict_test_support::ensure(!handle.was_registered(), "no callsite has registered yet")?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub const fn with_event_on_register(mut self) -> Self {
    self.event_on_register = true;
    self
  }

  /// Returns a [`CallsiteTrackingHandle`] sharing this subscriber's records.
  ///
  /// Take the handle before installing the subscriber, because installation
  /// transfers ownership of the subscriber to the dispatcher.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new();
  /// let handle = subscriber.handle();
  ///
  /// strict_test_support::ensure(!handle.was_registered(), "nothing registered yet")?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn handle(&self) -> CallsiteTrackingHandle {
    CallsiteTrackingHandle {
      state: Arc::clone(&self.state),
    }
  }
}

impl Default for CallsiteTrackingSubscriber {
  fn default() -> Self {
    Self::new()
  }
}

impl CallsiteTrackingHandle {
  /// Returns the number of `register_callsite` notifications observed so far.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new();
  /// let handle = subscriber.handle();
  ///
  /// strict_test_support::ensure_eq(&0, &handle.register_count(), "no registrations yet")?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn register_count(&self) -> usize {
    self.state.register_count.load(Ordering::SeqCst)
  }

  /// Returns whether at least one callsite registration was observed.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new();
  /// let handle = subscriber.handle();
  ///
  /// strict_test_support::ensure(!handle.was_registered(), "no registrations yet")?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn was_registered(&self) -> bool {
    self.register_count() > 0
  }

  /// Returns whether an event arrived on a callsite other than the last registered one.
  ///
  /// The flag is sticky: once a mismatching event has been observed it stays
  /// set, even if later events match their registrations again. An event
  /// received before any registration always counts as a mismatch.
  ///
  /// # Examples
  ///
  /// ```rust
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_core::test_util::CallsiteTrackingSubscriber;
  ///
  /// let subscriber = CallsiteTrackingSubscriber::new();
  /// let handle = subscriber.handle();
  ///
  /// strict_test_support::ensure(!handle.saw_callsite_mismatch(), "no events observed yet")?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn saw_callsite_mismatch(&self) -> bool {
    self.state.callsite_mismatch.load(Ordering::SeqCst)
  }
}

impl Subscriber for CallsiteTrackingSubscriber {
  fn register_callsite(&self, metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
    if !self.register_delay.is_zero() {
      thread::sleep(self.register_delay);
    }

    let metadata_ptr = ptr::from_ref(metadata).cast::<()>().cast_mut();
    self.state.last_callsite.store(metadata_ptr, Ordering::SeqCst);
    let _previous_count = self
      .state
      .register_count
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| Some(count.saturating_add(1)));

    if self.event_on_register {
      emit_reentrant_event();
    }

    Ok(Interest::always())
  }

  fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
    Ok(true)
  }

  fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
    Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
  }

  fn record(&self, _span: span::Id, _values: &span::Record<'_>) -> SubscriberResult {
    Ok(())
  }

  fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
    Ok(())
  }

  fn event(&self, event: &Event<'_>) -> SubscriberResult {
    let stored_callsite = self.state.last_callsite.load(Ordering::SeqCst);
    let event_callsite = ptr::from_ref(event.metadata()).cast::<()>().cast_mut();
    if stored_callsite != event_callsite {
      self.state.callsite_mismatch.store(true, Ordering::SeqCst);
    }
    Ok(())
  }

  fn enter(&self, _span: span::Id) -> SubscriberResult {
    Ok(())
  }

  fn exit(&self, _span: span::Id) -> SubscriberResult {
    Ok(())
  }
}

/// Emits an event through the active dispatcher to exercise re-entrant registration.
///
/// The event is dispatched from a single static callsite. Its first dispatch
/// registers that callsite, which re-enters the active subscriber's
/// `register_callsite`; the callsite registry's `REGISTERING` guard stops the
/// recursion after one level. This reproduces the re-entrancy the callsite
/// deadlock regression test guards against.
#[allow(
  clippy::single_call_fn,
  reason = "isolates the re-entrant callsite-registration machinery from register_callsite"
)]
fn emit_reentrant_event() {
  static REENTRANT_CALLSITE: DefaultCallsite = {
    /// Metadata for the re-entrant event emitted during registration.
    static META: Metadata<'static> = Metadata::new(
      "callsite registered",
      "tracing_core::test_util",
      Level::INFO,
      &SourceLocation::empty(),
      &FieldSet::new(&[], Identifier(&REENTRANT_CALLSITE)),
      Kind::EVENT,
    );
    DefaultCallsite::new(&META)
  };

  // Calling `interest` registers the callsite, re-entering the active
  // subscriber's `register_callsite`; `Event::dispatch` then delivers the event.
  let _interest = REENTRANT_CALLSITE.interest();
  let meta = REENTRANT_CALLSITE.metadata();
  let value_set = meta.fields().value_set(&[]);
  Event::dispatch(meta, &value_set);
}

/// Marker selecting the primary [`NoOpSubscriber`] identity.
///
/// The marker is a zero-sized type used only as the `M` parameter of
/// [`NoOpSubscriber`]; it never appears at runtime.
#[derive(Debug, Clone, Copy)]
pub struct Primary;

/// Marker selecting the secondary [`NoOpSubscriber`] identity.
///
/// The marker is a zero-sized type used only as the `M` parameter of
/// [`NoOpSubscriber`]; it never appears at runtime.
#[derive(Debug, Clone, Copy)]
pub struct Secondary;

/// An inert [`Subscriber`] whose type identity is fixed by the marker `M`.
///
/// Every notification is accepted without effect: it enables every callsite,
/// assigns every new span the ID `1`, and does nothing else. The `M` marker
/// exists purely to give otherwise-identical no-op subscribers distinct
/// `'static` types, so a dispatch-identity test can install
/// `NoOpSubscriber::<Primary>::new()` and assert which subscriber is active with
/// [`Dispatch::is`](crate::Dispatch::is).
///
/// # Examples
///
/// ```rust
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_core::Dispatch;
/// use tracing_core::dispatcher;
/// use tracing_core::test_util::NoOpSubscriber;
/// use tracing_core::test_util::Primary;
/// use tracing_core::test_util::Secondary;
///
/// dispatcher::with_default(&Dispatch::new(NoOpSubscriber::<Primary>::new()), || {
///   dispatcher::get_default(|current| {
///     strict_test_support::ensure(
///       current.is::<NoOpSubscriber<Primary>>(),
///       "the primary subscriber is active",
///     )?;
///     strict_test_support::ensure(
///       !current.is::<NoOpSubscriber<Secondary>>(),
///       "the secondary subscriber is not active",
///     )
///   })
/// })?;
/// # Ok(())
/// # }
/// ```
///
/// [`Subscriber`]: crate::subscriber::Subscriber
#[derive(Debug)]
pub struct NoOpSubscriber<M> {
  /// Ties this subscriber to its identity marker without storing a value.
  marker: PhantomData<fn() -> M>,
}

impl<M> NoOpSubscriber<M> {
  /// Creates a new inert subscriber with the `M` identity.
  #[allow(
    clippy::single_call_fn,
    reason = "public test-util constructor keeps marker-specific no-op subscribers reusable across downstream tests"
  )]
  #[must_use]
  pub const fn new() -> Self {
    Self {
      marker: PhantomData
    }
  }
}

impl<M> Default for NoOpSubscriber<M> {
  fn default() -> Self {
    Self::new()
  }
}

impl<M: 'static> Subscriber for NoOpSubscriber<M> {
  fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
    Ok(true)
  }

  fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
    Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
  }

  fn record(&self, _span: span::Id, _values: &span::Record<'_>) -> SubscriberResult {
    Ok(())
  }

  fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
    Ok(())
  }

  fn event(&self, _event: &Event<'_>) -> SubscriberResult {
    Ok(())
  }

  fn enter(&self, _span: span::Id) -> SubscriberResult {
    Ok(())
  }

  fn exit(&self, _span: span::Id) -> SubscriberResult {
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use core::num::NonZeroU64;
  use std::sync::Arc;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering;
  use std::time::Duration;
  use std::time::Instant;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;

  use super::CallsiteTrackingSubscriber;
  use super::NoOpSubscriber;
  use super::Primary;
  use super::Secondary;
  use crate::Dispatch;
  use crate::Event;
  use crate::Interest;
  use crate::Kind;
  use crate::Level;
  use crate::Metadata;
  use crate::Subscriber;
  use crate::SubscriberResult;
  use crate::callsite::Callsite as _;
  use crate::callsite::DefaultCallsite;
  use crate::callsite::Identifier;
  use crate::dispatcher::get_default;
  use crate::dispatcher::with_default;
  use crate::field::FieldSet;
  use crate::field::Value;
  use crate::metadata::SourceLocation;
  use crate::span;

  /// Static callsite fixture driven manually by the tests below.
  static MANUAL_CALLSITE: DefaultCallsite = {
    /// Metadata for the manual callsite fixture.
    static META: Metadata<'static> = Metadata::new(
      "manual event",
      "tracing_core::test_util::tests",
      Level::INFO,
      &SourceLocation::empty(),
      &FieldSet::new(&["message"], Identifier(&MANUAL_CALLSITE)),
      Kind::EVENT,
    );
    DefaultCallsite::new(&META)
  };

  /// An inert [`Subscriber`] that counts the events dispatched to it.
  #[derive(Debug)]
  struct EventCounter {
    /// Shared count of `event` notifications observed.
    events: Arc<AtomicUsize>,
  }

  impl EventCounter {
    /// Creates a counter alongside a shared handle to its event count.
    fn new() -> (Self, Arc<AtomicUsize>) {
      let events = Arc::new(AtomicUsize::new(0));
      (
        Self {
          events: Arc::clone(&events),
        },
        events,
      )
    }
  }

  impl Subscriber for EventCounter {
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> SubscriberResult<Interest> {
      Ok(Interest::always())
    }

    fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
      Ok(true)
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
      Ok(span::Id::from_non_zero_u64(NonZeroU64::MIN))
    }

    fn record(&self, _span: span::Id, _values: &span::Record<'_>) -> SubscriberResult {
      Ok(())
    }

    fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
      Ok(())
    }

    fn event(&self, _event: &Event<'_>) -> SubscriberResult {
      let _previous = self.events.fetch_add(1, Ordering::SeqCst);
      Ok(())
    }

    fn enter(&self, _span: span::Id) -> SubscriberResult {
      Ok(())
    }

    fn exit(&self, _span: span::Id) -> SubscriberResult {
      Ok(())
    }
  }

  /// Sends one event for the manual callsite directly to `tracking`.
  fn notify_manual_event(tracking: &CallsiteTrackingSubscriber) -> Result<(), TestFailure> {
    let message_field = ensure_some(
      MANUAL_CALLSITE.metadata().fields().field("message"),
      "the manual callsite declares a message field",
    )?;
    let message = "manual event";
    let message_value: &dyn Value = &message;
    let values = [(&message_field, Some(message_value))];
    let value_set = MANUAL_CALLSITE.metadata().fields().value_set(&values);
    ensure_ok(
      tracking.event(&Event::new(MANUAL_CALLSITE.metadata(), &value_set)),
      "manual event notification succeeds",
    )
  }

  #[test]
  fn observes_registration_when_a_callsite_registers() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::new();
    let handle = tracking.handle();

    with_default(&Dispatch::new(tracking), || {
      // Requesting interest registers the callsite with the installed tracker.
      let _interest = MANUAL_CALLSITE.interest();
    });

    ensure(handle.was_registered(), "installing and registering records the event callsite")?;
    ensure(handle.register_count() >= 1, "at least one registration is counted")
  }

  #[test]
  fn reports_nothing_when_no_callsite_registers() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::new();
    let handle = tracking.handle();
    drop(tracking);

    ensure(!handle.was_registered(), "no registration is reported when none occurred")?;
    ensure_eq(&0, &handle.register_count(), "the registration count stays zero")?;
    ensure(
      !handle.saw_callsite_mismatch(),
      "no mismatch is reported when no event was observed",
    )
  }

  #[test]
  fn event_after_registration_reports_no_mismatch() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::new();
    let handle = tracking.handle();

    let interest = ensure_ok(
      tracking.register_callsite(MANUAL_CALLSITE.metadata()),
      "manual registration succeeds",
    )?;
    ensure(interest.is_always(), "the tracker always expresses interest")?;
    notify_manual_event(&tracking)?;

    ensure_eq(&1, &handle.register_count(), "exactly one registration is counted")?;
    ensure(
      !handle.saw_callsite_mismatch(),
      "an event on the registered callsite is not a mismatch",
    )
  }

  #[test]
  fn event_without_registration_reports_a_mismatch() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::new();
    let handle = tracking.handle();

    notify_manual_event(&tracking)?;

    ensure_eq(&0, &handle.register_count(), "no registration was observed")?;
    ensure(
      handle.saw_callsite_mismatch(),
      "an event without a preceding registration is a mismatch",
    )
  }

  #[test]
  fn register_delay_defers_recording_the_registration() -> Result<(), TestFailure> {
    let delay = Duration::from_millis(25);
    let tracking = CallsiteTrackingSubscriber::new().with_register_delay(delay);
    let handle = tracking.handle();

    let started = Instant::now();
    let interest = ensure_ok(
      tracking.register_callsite(MANUAL_CALLSITE.metadata()),
      "delayed registration succeeds",
    )?;

    ensure(started.elapsed() >= delay, "registration waits for the configured delay")?;
    ensure(interest.is_always(), "the delayed tracker still expresses interest")?;
    ensure_eq(&1, &handle.register_count(), "the delayed registration is recorded")
  }

  #[test]
  fn event_on_register_emits_through_the_active_dispatcher() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::new().with_event_on_register();
    let (observer, events) = EventCounter::new();

    let register_result = with_default(&Dispatch::new(observer), || tracking.register_callsite(MANUAL_CALLSITE.metadata()));
    let interest = ensure_ok(register_result, "re-entrant registration succeeds")?;

    ensure(interest.is_always(), "the re-entrant tracker still expresses interest")?;
    ensure(
      events.load(Ordering::SeqCst) >= 1,
      "registering with event_on_register emits an event to the active dispatcher",
    )
  }

  #[test]
  fn register_without_event_on_register_emits_nothing() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::new();
    let (observer, events) = EventCounter::new();

    let register_result = with_default(&Dispatch::new(observer), || tracking.register_callsite(MANUAL_CALLSITE.metadata()));
    let interest = ensure_ok(register_result, "registration succeeds without emitting")?;

    ensure(interest.is_always(), "the default tracker still expresses interest")?;
    ensure_eq(
      &0,
      &events.load(Ordering::SeqCst),
      "no event is emitted during registration by default",
    )
  }

  #[test]
  fn no_op_markers_produce_distinct_subscriber_types() -> Result<(), TestFailure> {
    with_default(&Dispatch::new(NoOpSubscriber::<Primary>::new()), || {
      get_default(|current| {
        ensure(
          current.is::<NoOpSubscriber<Primary>>(),
          "the primary marker identifies the installed subscriber",
        )?;
        ensure(
          !current.is::<NoOpSubscriber<Secondary>>(),
          "the secondary marker does not match the primary subscriber",
        )
      })
    })
  }

  /// Drives every inert notification method against `subscriber` and checks its responses.
  fn ensure_inert_subscriber<S: Subscriber>(subscriber: &S) -> Result<(), TestFailure> {
    let meta = MANUAL_CALLSITE.metadata();
    let value_set = meta.fields().value_set(&[]);

    let enabled = ensure_ok(subscriber.enabled(meta), "the inert subscriber answers the enabled query")?;
    ensure(enabled, "the inert subscriber enables every callsite")?;

    let attributes = span::Attributes::new(meta, &value_set);
    let span_id = ensure_ok(subscriber.new_span(&attributes), "the inert subscriber assigns a span id")?;
    ensure(
      span_id == span::Id::from_non_zero_u64(NonZeroU64::MIN),
      "the inert subscriber assigns span id 1",
    )?;

    let record = span::Record::new(&value_set);
    ensure_ok(subscriber.record(span_id, &record), "the inert subscriber accepts a record")?;
    ensure_ok(
      subscriber.record_follows_from(span_id, span_id),
      "the inert subscriber accepts a follows-from",
    )?;
    ensure_ok(
      subscriber.event(&Event::new(meta, &value_set)),
      "the inert subscriber accepts an event",
    )?;
    ensure_ok(subscriber.enter(span_id), "the inert subscriber accepts an enter")?;
    ensure_ok(subscriber.exit(span_id), "the inert subscriber accepts an exit")
  }

  #[test]
  fn no_op_subscriber_accepts_every_notification() -> Result<(), TestFailure> {
    ensure_inert_subscriber(&NoOpSubscriber::<Secondary>::new())
  }

  #[test]
  fn tracking_subscriber_accepts_inert_notifications() -> Result<(), TestFailure> {
    ensure_inert_subscriber(&CallsiteTrackingSubscriber::new())
  }

  #[test]
  fn defaults_construct_fresh_fixtures() -> Result<(), TestFailure> {
    let tracking = CallsiteTrackingSubscriber::default();
    let handle = tracking.handle();
    ensure(!handle.was_registered(), "a defaulted tracker has observed no registrations")?;

    let inert = NoOpSubscriber::<Primary>::default();
    let enabled = ensure_ok(
      inert.enabled(MANUAL_CALLSITE.metadata()),
      "the defaulted inert subscriber answers the enabled query",
    )?;
    ensure(enabled, "the defaulted inert subscriber enables every callsite")
  }
}
