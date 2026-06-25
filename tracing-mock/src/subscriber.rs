//! An implementation of the [`Subscriber`] trait to receive and validate
//! `tracing` data.
//!
//! The [`MockSubscriber`] is the central component of this crate. The
//! `MockSubscriber` has expectations set on it which are later
//! validated as the code under test is run.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::{expect, subscriber, field};
//!
//! let (subscriber, handle) = subscriber::mock()
//!     // Expect a single event with a specified message
//!     .event(expect::event().with_fields(expect::msg("droids")))
//!     .only()
//!     .run_with_handle();
//!
//! // Use `with_default` to apply the `MockSubscriber` for the duration
//! // of the closure - this is what we are testing.
//! tracing::subscriber::with_default(subscriber, || {
//!     // These *are* the droids we are looking for
//!     tracing::info!("droids");
//! });
//!
//! // Use the handle to check the expectations. This line returns an error if
//! // an expectation is not met.
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! A more complex example may consider multiple spans and events with
//! their respective fields:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::{expect, subscriber, field};
//!
//! let span = expect::span()
//!     .named("my_span");
//! let (subscriber, handle) = subscriber::mock()
//!     // Enter a matching span
//!     .enter(&span)
//!     // Record an event with message "subscriber parting message"
//!     .event(expect::event().with_fields(expect::msg("subscriber parting message")))
//!     // Record a value for the field `parting` on a matching span
//!     .record(&span, expect::field("parting").with_value(&"goodbye world!"))
//!     // Exit a matching span
//!     .exit(span)
//!     // Expect no further messages to be recorded
//!     .only()
//!     // Return the subscriber and handle
//!     .run_with_handle();
//!
//! // Use `with_default` to apply the `MockSubscriber` for the duration
//! // of the closure - this is what we are testing.
//! tracing::subscriber::with_default(subscriber, || {
//!     let span = tracing::trace_span!(
//!         "my_span",
//!         greeting = "hello world",
//!         parting = tracing::field::Empty
//!     );
//!
//!     let _guard = span.enter();
//!     tracing::info!("subscriber parting message");
//!     let parting = "goodbye world!";
//!
//!     span.record("parting", &parting);
//! });
//!
//! // Use the handle to check the expectations. This line returns an error if
//! // an expectation is not met.
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! If we modify the previous example so that we **don't** enter the
//! span before recording an event, the test will fail:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::{expect, subscriber, field};
//!
//! let span = expect::span()
//!     .named("my_span");
//! let (subscriber, handle) = subscriber::mock()
//!     .enter(&span)
//!     .event(expect::event().with_fields(expect::msg("collect parting message")))
//!     .record(&span, expect::field("parting").with_value(&"goodbye world!"))
//!     .exit(span)
//!     .only()
//!     .run_with_handle();
//!
//! // Use `with_default` to apply the `MockSubscriber` for the duration
//! // of the closure - this is what we are testing.
//! tracing::subscriber::with_default(subscriber, || {
//!     let span = tracing::trace_span!(
//!         "my_span",
//!         greeting = "hello world",
//!         parting = tracing::field::Empty
//!     );
//!
//!     // Don't enter the span.
//!     // let _guard = span.enter();
//!     tracing::info!("subscriber parting message");
//!     let parting = "goodbye world!";
//!
//!     span.record("parting", &parting);
//! });
//!
//! // Use the handle to check the expectations. This line returns an error if
//! // an expectation is not met.
//! strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
//! # Ok(())
//! # }
//! ```
//!
//! This will return an error message such as the following:
//!
//! ```text
//! [main] expected to enter a span named `my_span`
//! [main] but instead observed event Event {
//!     fields: ValueSet {
//!         message: subscriber parting message,
//!         callsite: Identifier(0x10eda3278),
//!     },
//!     metadata: Metadata {
//!         name: "event src/subscriber.rs:27",
//!         target: "rust_out",
//!         level: Level(
//!             Info,
//!         ),
//!         module_path: "rust_out",
//!         location: src/subscriber.rs:27,
//!         fields: {message},
//!         callsite: Identifier(0x10eda3278),
//!         kind: Kind(EVENT),
//!     },
//!     parent: Current,
//! }', tracing/tracing-mock/src/expect.rs:59:33
//! ```
//!
//! [`Subscriber`]: trait@tracing::Subscriber
//! [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
use tracing::{
    Event, Metadata, Subscriber,
    level_filters::LevelFilter,
    span::{self, Attributes, Id},
    subscriber::Interest,
};
use tracing_core::{span::Current, subscriber::SubscriberResult};

use crate::{
    ancestry::get_ancestry,
    event::ExpectedEvent,
    expect::Expect,
    failure::{ExpectationError, ExpectationResult, SharedFailures},
    field::ExpectedFields,
    span::{ActualSpan, ExpectedSpan, NewSpan},
};

/// Runtime state tracked for a span observed by the mock subscriber.
pub(crate) struct SpanState {
    /// The span ID assigned by the mock subscriber.
    id: Id,
    /// The span name from static metadata.
    name: &'static str,
    /// Outstanding references to the span.
    refs: usize,
    /// Static span metadata.
    meta: &'static Metadata<'static>,
}

impl From<&SpanState> for ActualSpan {
    fn from(span_state: &SpanState) -> Self {
        Self::new(span_state.id, Some(span_state.meta))
    }
}

/// Active subscriber implementation returned by
/// [`MockSubscriber::run_with_handle`].
struct Running<F: Fn(&Metadata<'_>) -> bool> {
    /// Known spans by ID.
    spans: Mutex<HashMap<Id, SpanState>>,
    /// Pending expectations.
    expected: Arc<Mutex<VecDeque<Expect>>>,
    /// First expectation failure observed while the subscriber is running.
    failures: SharedFailures,
    /// Current entered span stack.
    current: Mutex<Vec<Id>>,
    /// Next span ID allocator.
    ids: AtomicUsize,
    /// Max level hint reported by this subscriber.
    max_level: Option<LevelFilter>,
    /// Metadata filter for observed traces.
    filter: F,
    /// Name used in failure messages.
    name: String,
}

/// A subscriber which can validate received traces.
///
/// For a detailed description and examples see the documentation
/// for the methods and the [`subscriber`] module.
///
/// [`subscriber`]: mod@crate::subscriber
#[derive(Debug)]
pub struct MockSubscriber<F: Fn(&Metadata<'_>) -> bool> {
    /// Pending expectations to install in the running subscriber.
    expected: VecDeque<Expect>,
    /// Max level hint reported by the running subscriber.
    max_level: Option<LevelFilter>,
    /// Metadata filter applied before expectation matching.
    filter: F,
    /// Name used in failure messages.
    name: String,
}

/// A handle which is used to invoke validation of expectations.
///
/// The handle is currently only used to validate that all the expected
/// events and spans were seen.
///
/// For additional information and examples, see the [`subscriber`]
/// module documentation.
///
/// [`subscriber`]: mod@crate::subscriber
#[derive(Debug)]
pub struct MockHandle {
    /// Pending expectations shared with the running mock.
    expected: Arc<Mutex<VecDeque<Expect>>>,
    /// First expectation failure recorded by the running mock.
    failures: SharedFailures,
    /// Name used in failure messages.
    name: String,
}

/// Create a new [`MockSubscriber`].
///
/// For additional information and examples, see the [`subscriber`]
/// module and [`MockSubscriber`] documentation.
///
/// # Examples
///
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::{expect, subscriber, field};
///
/// let span = expect::span()
///     .named("my_span");
/// let (subscriber, handle) = subscriber::mock()
///     // Enter a matching span
///     .enter(&span)
///     // Record an event with message "subscriber parting message"
///     .event(expect::event().with_fields(expect::msg("subscriber parting message")))
///     // Record a value for the field `parting` on a matching span
///     .record(&span, expect::field("parting").with_value(&"goodbye world!"))
///     // Exit a matching span
///     .exit(span)
///     // Expect no further messages to be recorded
///     .only()
///     // Return the subscriber and handle
///     .run_with_handle();
///
/// // Use `with_default` to apply the `MockSubscriber` for the duration
/// // of the closure - this is what we are testing.
/// tracing::subscriber::with_default(subscriber, || {
///     let span = tracing::trace_span!(
///         "my_span",
///         greeting = "hello world",
///         parting = tracing::field::Empty
///     );
///
///     let _guard = span.enter();
///     tracing::info!("subscriber parting message");
///     let parting = "goodbye world!";
///
///     span.record("parting", &parting);
/// });
///
/// // Use the handle to check the expectations. This line returns an error if
/// // an expectation is not met.
/// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
/// # Ok(())
/// # }
/// ```
///
/// [`subscriber`]: mod@crate::subscriber
#[must_use]
#[allow(
    clippy::single_call_fn,
    reason = "public DSL constructor is the documented entry point for mock subscribers"
)]
pub fn mock() -> MockSubscriber<fn(&Metadata<'_>) -> bool> {
    const fn allow_all(_: &Metadata<'_>) -> bool {
        true
    }

    MockSubscriber {
        expected: VecDeque::new(),
        filter: allow_all,
        max_level: None,
        name: thread::current()
            .name()
            .unwrap_or("mock_subscriber")
            .to_owned(),
    }
}

impl<F> MockSubscriber<F>
where
    F: Fn(&Metadata<'_>) -> bool + 'static,
{
    /// Overrides the name printed by the mock subscriber's debugging output.
    ///
    /// The debugging output is displayed if the test fails, or if the test is
    /// run with `--nocapture`.
    ///
    /// By default, the mock subscriber's name is the  name of the test
    /// (*technically*, the name of the thread where it was created, which is
    /// the name of the test unless tests are run with `--test-threads=1`).
    /// When a test has only one mock subscriber, this is sufficient. However,
    /// some tests may include multiple subscribers, in order to test
    /// interactions between multiple subscribers. In that case, it can be
    /// helpful to give each subscriber a separate name to distinguish where the
    /// debugging output comes from.
    ///
    /// # Examples
    ///
    /// In the following example, we create 2 subscribers, both
    /// expecting to receive an event. As we only record a single
    /// event, the test will fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber_1, handle_1) = subscriber::mock()
    ///     .named("subscriber-1")
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// let (subscriber_2, handle_2) = subscriber::mock()
    ///     .named("subscriber-2")
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// let _guard = tracing::subscriber::set_default(subscriber_2);
    ///
    /// tracing::subscriber::with_default(subscriber_1, || {
    ///     tracing::info!("a");
    /// });
    ///
    /// let handle_1_result = handle_1.finished();
    /// let handle_2_result = handle_2.finished();
    /// strict_test_support::ensure(handle_1_result.is_err() || handle_2_result.is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// In the test output, we see that the subscriber which didn't
    /// received the event was the one named `subscriber-2`, which is
    /// correct as the subscriber named `subscriber-1` was the default
    /// when the event was recorded:
    ///
    /// ```text
    /// [subscriber-2] more notifications expected: [
    ///     Event(
    ///         MockEvent,
    ///     ),
    /// ]', tracing-mock/src/subscriber.rs:1276:13
    /// ```
    #[must_use]
    pub fn named(self, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..self
        }
    }

    /// Adds an expectation that an event matching the [`ExpectedEvent`]
    /// will be recorded next.
    ///
    /// The `event` can be a default mock which will match any event
    /// (`expect::event()`) or can include additional expectations.
    /// See the [`ExpectedEvent`] documentation for more details.
    ///
    /// If an event is recorded that doesn't match the `ExpectedEvent`,
    /// or if something else (such as entering a span) is recorded
    /// first, then the expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("a");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// A span is entered before the event, causing the test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("span");
    ///     let _guard = span.enter();
    ///     tracing::info!("a");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn event(mut self, event: ExpectedEvent) -> Self {
        self.expected.push_back(Expect::Event(event));
        self
    }

    /// Adds an expectation that the creation of a span will be
    /// recorded next.
    ///
    /// This function accepts `Into<NewSpan>` instead of
    /// [`ExpectedSpan`] directly, so it can be used to test
    /// span fields and the span parent. This is because a
    /// subscriber only receives the span fields and parent when
    /// a span is created, not when it is entered.
    ///
    /// The new span doesn't need to be entered for this expectation
    /// to succeed.
    ///
    /// If a span is recorded that doesn't match the `ExpectedSpan`,
    /// or if something else (such as an event) is recorded first,
    /// then the expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing")
    ///     .with_fields(expect::field("testing").with_value(&"yes"));
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     _ = tracing::info_span!("the span we're testing", testing = "yes");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// An event is recorded before the span is created, causing the
    /// test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing")
    ///     .with_fields(expect::field("testing").with_value(&"yes"));
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("an event");
    ///     _ = tracing::info_span!("the span we're testing", testing = "yes");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new_span<I>(mut self, new_span: I) -> Self
    where
        I: Into<NewSpan>,
    {
        self.expected.push_back(Expect::NewSpan(new_span.into()));
        self
    }

    /// Adds an expectation that entering a span matching the
    /// [`ExpectedSpan`] will be recorded next.
    ///
    /// This expectation is generally accompanied by a call to
    /// [`exit`] as well. If used together with [`only`], this
    /// is necessary.
    ///
    /// If the span that is entered doesn't match the [`ExpectedSpan`],
    /// or if something else (such as an event) is recorded first,
    /// then the expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(&span)
    ///     .exit(&span)
    ///     .only()
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("the span we're testing");
    ///     let _entered = span.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// An event is recorded before the span is entered, causing the
    /// test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(&span)
    ///     .exit(&span)
    ///     .only()
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("an event");
    ///     let span = tracing::info_span!("the span we're testing");
    ///     let _entered = span.enter();
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`exit`]: fn@Self::exit
    /// [`only`]: fn@Self::only
    #[must_use]
    pub fn enter<S>(mut self, span: S) -> Self
    where
        S: Into<ExpectedSpan>,
    {
        self.expected.push_back(Expect::Enter(span.into()));
        self
    }

    /// Adds ab expectation that exiting a span matching the
    /// [`ExpectedSpan`] will be recorded next.
    ///
    /// As a span may be entered and exited multiple times,
    /// this is different from the span being closed. In
    /// general [`enter`] and `exit` should be paired.
    ///
    /// If the span that is exited doesn't match the [`ExpectedSpan`],
    /// or if something else (such as an event) is recorded first,
    /// then the expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(&span)
    ///     .exit(&span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("the span we're testing");
    ///     let _entered = span.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// An event is recorded before the span is exited, causing the
    /// test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(&span)
    ///     .exit(&span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("the span we're testing");
    ///     let _entered = span.enter();
    ///     tracing::info!("an event");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`enter`]: fn@Self::enter
    #[must_use]
    pub fn exit<S>(mut self, span: S) -> Self
    where
        S: Into<ExpectedSpan>,
    {
        self.expected.push_back(Expect::Exit(span.into()));
        self
    }

    /// Adds an expectation that cloning a span matching the
    /// [`ExpectedSpan`] will be recorded next.
    ///
    /// The cloned span does need to be entered.
    ///
    /// If the span that is cloned doesn't match the [`ExpectedSpan`],
    /// or if something else (such as an event) is recorded first,
    /// then the expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .clone_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("the span we're testing");
    ///     _ = span.clone();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// An event is recorded before the span is cloned, causing the
    /// test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO)
    ///     .named("the span we're testing");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .clone_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("the span we're testing");
    ///     tracing::info!("an event");
    ///     _ = span.clone();
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn clone_span<S>(mut self, span: S) -> Self
    where
        S: Into<ExpectedSpan>,
    {
        self.expected.push_back(Expect::CloneSpan(span.into()));
        self
    }

    /// Adds an expectation that a span matching the [`ExpectedSpan`]
    /// being finally closed by [`Subscriber::try_close`] will be recorded next.
    ///
    /// `try_close` is called every time a span handle is dropped. The mock
    /// subscriber tracks span reference counts and only matches this
    /// expectation when the final handle has closed the span.
    ///
    /// [`Subscriber::try_close`]: fn@tracing::Subscriber::try_close
    #[must_use]
    pub fn close_span<S>(mut self, span: S) -> Self
    where
        S: Into<ExpectedSpan>,
    {
        self.expected.push_back(Expect::CloseSpan(span.into()));
        self
    }

    /// Adds an expectation that a `follows_from` relationship will be
    /// recorded next. Specifically that a span matching `consequence`
    /// follows from a span matching `cause`.
    ///
    /// For further details on what this causal relationship means, see
    /// [`Span::follows_from`].
    ///
    /// If either of the 2 spans don't match their respective
    /// [`ExpectedSpan`] or if something else (such as an event) is
    /// recorded first, then the expectation will fail.
    ///
    /// **Note**: The 2 spans, `consequence` and `cause` are matched
    /// by `name` only.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let cause = expect::span().named("cause");
    /// let consequence = expect::span().named("consequence");
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .follows_from(consequence, cause)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let cause = tracing::info_span!("cause");
    ///     let consequence = tracing::info_span!("consequence");
    ///
    ///     consequence.follows_from(&cause);
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The `cause` span doesn't match, it is actually recorded at
    /// `Level::WARN` instead of the expected `Level::INFO`, causing
    /// this test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let cause = expect::span().named("cause");
    /// let consequence = expect::span().named("consequence");
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .follows_from(consequence, cause)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let cause = tracing::info_span!("another cause");
    ///     let consequence = tracing::info_span!("consequence");
    ///
    ///     consequence.follows_from(&cause);
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Span::follows_from`]: fn@tracing::Span::follows_from
    #[must_use]
    pub fn follows_from<S1, S2>(mut self, consequence: S1, cause: S2) -> Self
    where
        S1: Into<ExpectedSpan>,
        S2: Into<ExpectedSpan>,
    {
        self.expected.push_back(Expect::FollowsFrom {
            consequence: consequence.into(),
            cause: cause.into(),
        });
        self
    }

    /// Adds an expectation that `fields` are recorded on a span
    /// matching the [`ExpectedSpan`] will be recorded next.
    ///
    /// For further information on how to specify the expected
    /// fields, see the documentation on the [`field`] module.
    ///
    /// If either the span doesn't match the [`ExpectedSpan`], the
    /// fields don't match the expected fields, or if something else
    /// (such as an event) is recorded first, then the expectation
    /// will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .named("my_span");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .record(span, expect::field("parting").with_value(&"goodbye world!"))
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::trace_span!(
    ///         "my_span",
    ///         greeting = "hello world",
    ///         parting = tracing::field::Empty
    ///     );
    ///     span.record("parting", "goodbye world!");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The value of the recorded field doesn't match the expectation,
    /// causing the test to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .named("my_span");
    /// let (subscriber, handle) = subscriber::mock()
    ///     .record(span, expect::field("parting").with_value(&"goodbye world!"))
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::trace_span!(
    ///         "my_span",
    ///         greeting = "hello world",
    ///         parting = tracing::field::Empty
    ///     );
    ///     span.record("parting", "goodbye universe!");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`field`]: mod@crate::field
    #[must_use]
    pub fn record<S, I>(mut self, span: S, fields: I) -> Self
    where
        S: Into<ExpectedSpan>,
        I: Into<ExpectedFields>,
    {
        self.expected
            .push_back(Expect::Visit(span.into(), fields.into()));
        self
    }

    /// Adds an expectation that [`Subscriber::on_register_dispatch`] will
    /// be called next.
    ///
    /// **Note**: This expectation is usually fulfilled automatically when
    /// a subscriber is set as the default via [`tracing::subscriber::with_default`]
    /// or [`tracing::subscriber::set_global_default`], so explicitly expecting
    /// this is not usually necessary. However, it may be useful when testing
    /// custom subscriber implementations that manually call `on_register_dispatch`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .on_register_dispatch()
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     // The subscriber's on_register_dispatch was called when it was set as default
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{subscriber};
    ///
    /// struct WrapSubscriber<S: tracing::Subscriber> {
    ///     inner: S,
    /// }
    ///
    /// impl<S: tracing::Subscriber> tracing::Subscriber for WrapSubscriber<S> {
    /// #     fn enabled(
    /// #         &self,
    /// #         metadata: &tracing::Metadata<'_>,
    /// #     ) -> tracing::subscriber::SubscriberResult<bool> {
    /// #         self.inner.enabled(metadata)
    /// #     }
    /// #     fn new_span(
    /// #         &self,
    /// #         span: &tracing_core::span::Attributes<'_>,
    /// #     ) -> tracing::subscriber::SubscriberResult<tracing_core::span::Id> {
    /// #         self.inner.new_span(span)
    /// #     }
    /// #     fn record(
    /// #         &self,
    /// #         span: tracing_core::span::Id,
    /// #         values: &tracing_core::span::Record<'_>,
    /// #     ) -> tracing::subscriber::SubscriberResult {
    /// #         self.inner.record(span, values)
    /// #     }
    /// #     fn record_follows_from(
    /// #         &self,
    /// #         span: tracing_core::span::Id,
    /// #         follows: tracing_core::span::Id,
    /// #     ) -> tracing::subscriber::SubscriberResult {
    /// #         self.inner.record_follows_from(span, follows)
    /// #     }
    /// #     fn event(&self, event: &tracing::Event<'_>) -> tracing::subscriber::SubscriberResult {
    /// #         self.inner.event(event)
    /// #     }
    /// #     fn enter(&self, span: tracing_core::span::Id) -> tracing::subscriber::SubscriberResult {
    /// #         self.inner.enter(span)
    /// #     }
    /// #     fn exit(&self, span: tracing_core::span::Id) -> tracing::subscriber::SubscriberResult {
    /// #         self.inner.exit(span)
    /// #     }
    ///     // All other Subscriber methods implemented to forward correctly.
    ///
    ///     fn on_register_dispatch(
    ///         &self,
    ///         subscriber: &tracing::Dispatch,
    ///     ) -> tracing::subscriber::SubscriberResult {
    ///         // Doesn't forward to `self.inner`
    ///         let _ = subscriber;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let (subscriber, handle) = subscriber::mock().on_register_dispatch().run_with_handle();
    /// let wrap_subscriber = WrapSubscriber { inner: subscriber };
    ///
    /// tracing::subscriber::with_default(wrap_subscriber, || {
    ///     // The subscriber's on_register_dispatch is called when set as default
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Subscriber::on_register_dispatch`]: tracing::Subscriber::on_register_dispatch
    #[must_use]
    pub fn on_register_dispatch(mut self) -> Self {
        self.expected.push_back(Expect::OnRegisterDispatch);
        self
    }

    /// Filter the traces evaluated by the `MockSubscriber`.
    ///
    /// The filter will be applied to all traces received before
    /// any validation occurs - so its position in the call chain
    /// is not important. The filter does not perform any validation
    /// itself.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .with_filter(|meta| meta.level() <= &tracing::Level::WARN)
    ///     .event(expect::event())
    ///     .only()
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("a");
    ///     tracing::warn!("b");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_filter<G>(self, filter: G) -> MockSubscriber<G>
    where
        G: Fn(&Metadata<'_>) -> bool + 'static,
    {
        MockSubscriber {
            expected: self.expected,
            filter,
            max_level: self.max_level,
            name: self.name,
        }
    }

    /// Sets the max level that will be provided to the `tracing`
    /// system.
    ///
    /// This method can be used to test the internals of `tracing`,
    /// but it is also useful to filter out traces on more verbose
    /// levels if you only want to verify above a certain level.
    ///
    /// **Note**: this value determines a global filter, if
    /// `with_max_level_hint` is called on multiple subscribers, the
    /// global filter will be the least restrictive of all subscribers.
    /// To filter the events evaluated by a specific `MockSubscriber`,
    /// use [`with_filter`] instead.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .with_max_level_hint(tracing::Level::INFO)
    ///     .event(expect::event().at_level(tracing::Level::INFO))
    ///     .only()
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::debug!("a message we don't care about");
    ///     tracing::info!("a message we want to validate");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`with_filter`]: fn@Self::with_filter
    #[must_use]
    pub fn with_max_level_hint(self, hint: impl Into<LevelFilter>) -> Self {
        Self {
            max_level: Some(hint.into()),
            ..self
        }
    }

    /// Expects that no further traces are received.
    ///
    /// The call to `only` should appear immediately before the final
    /// call to `run` or `run_with_handle`, as any expectations which
    /// are added after `only` will not be considered.
    ///
    /// # Examples
    ///
    /// Consider this simple test. It passes even though we only
    /// expect a single event, but receive three:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("a");
    ///     tracing::info!("b");
    ///     tracing::info!("c");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// After including `only`, the test will fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .event(expect::event())
    ///     .only()
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("a");
    ///     tracing::info!("b");
    ///     tracing::info!("c");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn only(mut self) -> Self {
        self.expected.push_back(Expect::Nothing);
        self
    }

    /// Consume the receiver and return an `impl` [`Subscriber`] which can
    /// be set as the default subscriber.
    ///
    /// This function is similar to [`run_with_handle`], but it doesn't
    /// return a [`MockHandle`]. This is useful if the desired
    /// expectations can be checked externally to the subscriber.
    ///
    /// # Examples
    ///
    /// The following test is used within the `tracing`
    /// codebase:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::subscriber;
    ///
    /// let spans_are_equal = tracing::subscriber::with_default(subscriber::mock().run(), || {
    ///     let foo1 = tracing::span!(tracing::Level::TRACE, "foo");
    ///     let foo2 = foo1.clone();
    ///     foo1 == foo2
    /// });
    /// strict_test_support::ensure(spans_are_equal, "cloned spans compare equal")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Subscriber`]: tracing::Subscriber
    /// [`run_with_handle`]: fn@Self::run_with_handle
    #[must_use]
    pub fn run(self) -> impl Subscriber {
        let (subscriber, _) = self.run_with_handle();
        subscriber
    }

    /// Consume the receiver and return an `impl` [`Subscriber`] which can
    /// be set as the default subscriber and a [`MockHandle`] which can
    /// be used to validate the provided expectations.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// // subscriber and handle are returned from `run_with_handle()`
    /// let (subscriber, handle) = subscriber::mock()
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("a");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Subscriber`]: tracing::Subscriber
    #[must_use]
    pub fn run_with_handle(self) -> (impl Subscriber, MockHandle) {
        let expected = Arc::new(Mutex::new(self.expected));
        let failures = SharedFailures::default();
        let handle = MockHandle::new(Arc::clone(&expected), failures.clone(), self.name.clone());
        let subscriber = Running {
            spans: Mutex::new(HashMap::new()),
            expected,
            failures,
            current: Mutex::new(Vec::new()),
            ids: AtomicUsize::new(1),
            filter: self.filter,
            max_level: self.max_level,
            name: self.name,
        };
        (subscriber, handle)
    }
}

impl<F> Subscriber for Running<F>
where
    F: Fn(&Metadata<'_>) -> bool + 'static,
{
    fn on_register_dispatch(&self, _subscriber: &tracing::Dispatch) -> SubscriberResult {
        {
            let mut expected = self.expected.lock();
            if matches!(expected.front(), Some(Expect::OnRegisterDispatch)) {
                let _matched = expected.pop_front();
            }
        }
        Ok(())
    }

    fn enabled(&self, meta: &Metadata<'_>) -> SubscriberResult<bool> {
        Ok((self.filter)(meta))
    }

    fn register_callsite(&self, meta: &'static Metadata<'static>) -> SubscriberResult<Interest> {
        Ok(if (self.filter)(meta) {
            Interest::always()
        } else {
            Interest::never()
        })
    }
    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.max_level
    }

    fn record(&self, id: Id, values: &span::Record<'_>) -> SubscriberResult {
        let span_name = {
            let spans = self.spans.lock();
            let Some(span) = spans.get(&id) else {
                self.record_failure(format_args!(
                    "[{}] no span for ID `{}`",
                    self.name,
                    id.into_u64()
                ));
                return Ok(());
            };
            let span_name = span.name;
            drop(spans);
            span_name
        };
        let next_visit = {
            let mut expected = self.expected.lock();
            if matches!(expected.front(), Some(Expect::Visit(_, _))) {
                expected.pop_front()
            } else {
                None
            }
        };
        let Some(Expect::Visit(expected_span, mut expected_values)) = next_visit else {
            return Ok(());
        };
        if let Some(name) = expected_span.name()
            && name != span_name
        {
            self.record_failure(format_args!(
                "[{}] expected to record span named `{}`, but got `{}`",
                self.name, name, span_name
            ));
            return Ok(());
        }
        let context = format!("span {span_name}: ");
        let mut checker = expected_values.checker(&context, &self.name);
        values.record(&mut checker);
        let _result = self.record_result(checker.finish());
        Ok(())
    }

    fn event(&self, event: &Event<'_>) -> SubscriberResult {
        let name = event.metadata().name();
        let next = {
            let mut expected = self.expected.lock();
            expected.pop_front()
        };
        match next {
            None => {}
            Some(Expect::Event(mut expected)) => {
                #[cfg(feature = "tracing-subscriber")]
                {
                    if expected.scope_mut().is_some() {
                        self.record_failure(format_args!(
                            "Expected scope for events is not supported with `MockSubscriber`."
                        ));
                        return Ok(());
                    }
                }
                let event_get_ancestry = || {
                    get_ancestry(
                        &event,
                        || self.lookup_current(),
                        |span_id| self.spans.lock().get(span_id).map(Into::into),
                    )
                };
                let _result =
                    self.record_result(expected.check(event, event_get_ancestry, &self.name));
            }
            Some(ex) => {
                let _result =
                    self.record_result(ex.bad(&self.name, format_args!("observed event `{name}`")));
            }
        }
        Ok(())
    }

    fn record_follows_from(&self, consequence_id: Id, cause_id: Id) -> SubscriberResult {
        let span_names = {
            let spans = self.spans.lock();
            spans.get(&consequence_id).and_then(|consequence_span| {
                spans
                    .get(&cause_id)
                    .map(|cause_span| (consequence_span.name, cause_span.name))
            })
        };
        if let Some((consequence_name, cause_name)) = span_names {
            let next = {
                let mut expected = self.expected.lock();
                expected.pop_front()
            };
            match next {
                None => {}
                Some(Expect::FollowsFrom {
                    consequence: ref expected_consequence,
                    cause: ref expected_cause,
                }) => {
                    if let Some(name) = expected_consequence.name()
                        && name != consequence_name
                    {
                        self.record_failure(format_args!(
                            "[{}] expected consequence span named `{}`, but got `{}`",
                            self.name, name, consequence_name
                        ));
                        return Ok(());
                    }
                    if let Some(name) = expected_cause.name()
                        && name != cause_name
                    {
                        self.record_failure(format_args!(
                            "[{}] expected cause span named `{}`, but got `{}`",
                            self.name, name, cause_name
                        ));
                    }
                }
                Some(ex) => {
                    let _result = self.record_result(ex.bad(
                        &self.name,
                        format_args!(
                            "consequence `{consequence_name}` followed cause `{cause_name}`"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn new_span(&self, span: &Attributes<'_>) -> SubscriberResult<Id> {
        let meta = span.metadata();
        let allocated_slot = match self
            .ids
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |id| id.checked_add(1))
        {
            Ok(id) => id,
            Err(id) => {
                self.record_failure(format_args!(
                    "[{}] span ID allocator exhausted at {}",
                    self.name, id
                ));
                return Ok(Self::fallback_span_id());
            }
        };
        let span_id_u64 = match u64::try_from(allocated_slot) {
            Ok(id) => id,
            Err(error) => {
                self.record_failure(format_args!(
                    "[{}] span ID allocator could not convert {} to u64: {}",
                    self.name, allocated_slot, error
                ));
                return Ok(Self::fallback_span_id());
            }
        };
        let Some(id) = Id::try_from_u64(span_id_u64) else {
            self.record_failure(format_args!(
                "[{}] span ID allocator produced zero",
                self.name
            ));
            return Ok(Self::fallback_span_id());
        };
        let next_new_span = {
            let mut expected = self.expected.lock();
            if matches!(expected.front(), Some(Expect::NewSpan(_))) {
                expected.pop_front()
            } else {
                None
            }
        };
        if let Some(Expect::NewSpan(mut expected_span)) = next_new_span {
            if let Some(ref expected_id) = expected_span.span.id
                && let Err(error) = expected_id.set(id.into_u64())
            {
                self.record_failure(format_args!(
                    "[{}] could not set expected span ID: {}",
                    self.name, error
                ));
            }

            let _result = self.record_result(expected_span.check(
                span,
                || {
                    let spans = self.spans.lock();
                    get_ancestry(
                        &span,
                        || self.lookup_current(),
                        |span_id| spans.get(span_id).map(Into::into),
                    )
                },
                &self.name,
            ));
        }
        {
            let mut spans = self.spans.lock();
            let _previous = spans.insert(
                id,
                SpanState {
                    id,
                    name: meta.name(),
                    refs: 1,
                    meta,
                },
            );
        }
        Ok(id)
    }

    fn enter(&self, id: Id) -> SubscriberResult {
        let span_state = {
            let spans = self.spans.lock();
            spans
                .get(&id)
                .map(|span| (span.name, ActualSpan::from(span)))
        };
        let Some((span_name, actual_span)) = span_state else {
            self.current.lock().push(id);
            return Ok(());
        };
        let next = {
            let mut expected = self.expected.lock();
            expected.pop_front()
        };
        match next {
            None => {}
            Some(Expect::Enter(ref expected_span)) => {
                let _result = self.record_result(expected_span.check(
                    &actual_span,
                    "to enter a span",
                    &self.name,
                ));
            }
            Some(ex) => {
                let _result = self
                    .record_result(ex.bad(&self.name, format_args!("entered span `{span_name}`")));
            }
        }
        self.current.lock().push(id);
        Ok(())
    }

    fn exit(&self, id: Id) -> SubscriberResult {
        let (span_name, actual_span) = {
            let spans = self.spans.lock();
            let Some(span) = spans.get(&id) else {
                self.record_failure(format_args!(
                    "[{}] no span for ID `{}`",
                    self.name,
                    id.into_u64()
                ));
                return Ok(());
            };
            let span_state = (span.name, ActualSpan::from(span));
            drop(spans);
            span_state
        };
        let next = {
            let mut expected = self.expected.lock();
            expected.pop_front()
        };
        match next {
            None => {}
            Some(Expect::Exit(ref expected_span)) => {
                let _result = self.record_result(expected_span.check(
                    &actual_span,
                    "to exit a span",
                    &self.name,
                ));
                let curr = self.current.lock().pop();
                if curr.as_ref() != Some(&id) {
                    let current_name = curr.map_or("<unknown>", |current_id| {
                        let spans = self.spans.lock();
                        spans
                            .get(&current_id)
                            .map_or("<unknown>", |state| state.name)
                    });
                    self.record_failure(format_args!(
                        "[{}] exited span `{}`, but the current span was `{}`",
                        self.name, span_name, current_name
                    ));
                }
            }
            Some(ex) => {
                let _result = self
                    .record_result(ex.bad(&self.name, format_args!("exited span `{span_name}`")));
            }
        }
        Ok(())
    }

    fn clone_span(&self, id: Id) -> SubscriberResult<Id> {
        let mut spans = self.spans.lock();
        let actual_span = match spans.get_mut(&id) {
            Some(span) => {
                if let Some(refs) = span.refs.checked_add(1) {
                    span.refs = refs;
                } else {
                    self.record_failure(format_args!(
                        "[{}] clone_span reference count overflowed for `{}`",
                        self.name, span.name
                    ));
                }
                Some(ActualSpan::from(&*span))
            }
            None => None,
        };
        drop(spans);

        {
            let mut expected = self.expected.lock();
            let was_expected =
                expected
                    .front()
                    .and_then(Expect::clone_span)
                    .is_some_and(|expected_span| {
                        let result = actual_span.as_ref().map_or_else(
                            || {
                                let observed_span = (&id).into();
                                expected_span.check(&observed_span, "to clone a span", &self.name)
                            },
                            |observed_span| {
                                expected_span.check(observed_span, "to clone a span", &self.name)
                            },
                        );
                        let _result = self.record_result(result);
                        true
                    });
            if was_expected {
                let _matched = expected.pop_front();
            }
        }
        Ok(id)
    }

    fn try_close(&self, id: Id) -> SubscriberResult<bool> {
        let closed_span: Option<ActualSpan> = self.spans.try_lock().and_then(|mut spans| {
            let span = spans.get_mut(&id)?;
            if span.refs == 0 {
                self.record_failure(format_args!(
                    "[{}] close_span called for {} with no remaining refs; id={}",
                    self.name,
                    span.name,
                    id.into_u64()
                ));
                return None;
            }
            span.refs = span.refs.saturating_sub(1);
            if span.refs != 0 {
                return None;
            }

            let actual_span = (&*span).into();
            let _closed = spans.remove(&id);
            Some(actual_span)
        });
        let Some(actual_span) = closed_span else {
            return Ok(false);
        };

        if let Some(mut expected) = self.expected.try_lock() {
            let was_expected = match expected.front() {
                Some(expectation) if expectation.close_span().is_some() => {
                    let Some(expected_span) = expectation.close_span() else {
                        return Ok(true);
                    };
                    let _result = self.record_result(expected_span.check(
                        &actual_span,
                        "to close a span",
                        &self.name,
                    ));
                    true
                }
                Some(_) | None => false,
            };
            if was_expected {
                let _matched = expected.pop_front();
            }
        }

        Ok(true)
    }

    fn current_span(&self) -> SubscriberResult<Current> {
        let current = {
            let stack = self.current.lock();
            stack.last().copied()
        };
        Ok(current.map_or_else(Current::none, |id| {
            let spans = self.spans.lock();
            let Some(state) = spans.get(&id) else {
                self.record_failure(format_args!(
                    "[{}] no state for current span ID `{}`",
                    self.name,
                    id.into_u64()
                ));
                return Current::none();
            };
            let current_span = Current::new(id, state.meta);
            drop(spans);
            current_span
        }))
    }
}

impl<F> Running<F>
where
    F: Fn(&Metadata<'_>) -> bool,
{
    /// Returns a fallback ID for impossible allocation failures.
    const fn fallback_span_id() -> Id {
        Id::from_non_zero_u64(NonZeroU64::MIN)
    }

    /// Records a formatted expectation failure.
    fn record_failure(&self, args: fmt::Arguments<'_>) {
        self.failures.record(ExpectationError::from_args(args));
    }

    /// Records a fallible expectation result.
    fn record_result<T>(&self, result: ExpectationResult<T>) -> Option<T> {
        self.failures.record_result(result)
    }

    /// Returns the currently entered span ID, if any.
    #[allow(
        clippy::single_call_fn,
        reason = "helper centralizes current-span stack lookup for ancestry closures"
    )]
    fn lookup_current(&self) -> Option<Id> {
        let stack = self.current.lock();
        stack.last().copied()
    }
}

impl MockHandle {
    /// Creates a mock handle over shared expectations.
    pub(crate) const fn new(
        expected: Arc<Mutex<VecDeque<Expect>>>,
        failures: SharedFailures,
        name: String,
    ) -> Self {
        Self {
            expected,
            failures,
            name,
        }
    }

    /// Checks the expectations which were set on the [`MockSubscriber`].
    ///
    /// This returns an error when any expected notifications were not recorded.
    ///
    /// # Errors
    ///
    /// Returns a [`tracing_core::SubscriberError`] when a recorded notification mismatched
    /// an expectation, or when expected notifications remain unobserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .event(expect::event())
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info!("a");
    /// });
    ///
    /// handle.finished()?;
    /// # Ok::<_, tracing_core::SubscriberError>(())
    /// ```
    pub fn finished(&self) -> SubscriberResult {
        if let Some(error) = self.failures.first() {
            return Err(error);
        }

        let mut remaining = String::new();
        {
            let expected = self.expected.lock();
            for expectation in &*expected {
                if expectation != &Expect::Nothing {
                    if !remaining.is_empty() {
                        remaining.push_str(", ");
                    }
                    remaining.push_str(&expectation.to_string());
                }
            }
        }

        if remaining.is_empty() {
            Ok(())
        } else {
            Err(ExpectationError::from_args(format_args!(
                "\n[{}] more notifications expected: {}",
                self.name, remaining
            )))
        }
    }
}
