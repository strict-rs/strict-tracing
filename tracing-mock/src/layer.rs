//! Mock [`Layer`] support for validating traces.
//!
//! It validates that the `tracing` data it receives matches the expected
//! output for a test.
//!
//!
//! The [`MockLayer`] is the central component in these tools. The
//! `MockLayer` has expectations set on it which are later
//! validated as the code under test is run.
//!
//! ```no_run
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::expect;
//! use tracing_mock::layer;
//! use tracing_subscriber::Layer;
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_subscriber::util::SubscriberInitExt;
//!
//! let (layer, handle) = layer::mock()
//!     // Expect a single event with a specified message
//!     .event(expect::event().with_fields(expect::msg("droids")))
//!     .run_with_handle();
//!
//! // Use `set_default` to apply the `MockSubscriber` until the end
//! // of the current scope (when the guard `_subscriber` is dropped).
//! let _subscriber = tracing_subscriber::registry()
//!   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
//!   .set_default();
//!
//! // These *are* the droids we are looking for
//! tracing::info!("droids");
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
//! ```no_run
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::expect;
//! use tracing_mock::layer;
//! use tracing_subscriber::Layer;
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_subscriber::util::SubscriberInitExt;
//!
//! let span = expect::span().named("my_span");
//! let (layer, handle) = layer::mock()
//!     // Enter a matching span
//!     .enter(&span)
//!     // Record an event with message "collect parting message"
//!     .event(expect::event().with_fields(expect::msg("say hello")))
//!     // Exit a matching span
//!     .exit(&span)
//!     // Expect no further messages to be recorded
//!     .only()
//!     // Return the layer and handle
//!     .run_with_handle();
//!
//! // Use `set_default` to apply the `MockLayers` until the end
//! // of the current scope (when the guard `_subscriber` is dropped).
//! let _subscriber = tracing_subscriber::registry()
//!   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
//!   .set_default();
//!
//! {
//!   let span = tracing::trace_span!("my_span", greeting = "hello world",);
//!
//!   let _guard = span.enter();
//!   tracing::info!("say hello");
//! }
//!
//! // Use the handle to check the expectations. This line returns an error if
//! // an expectation is not met.
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! Unmet expectations can be inspected through returned errors by using
//! [`MockHandle::finished`]:
//!
//! ```
//! use strict_test_support::TestFailure;
//! use strict_test_support::ensure;
//! use tracing_mock::expect;
//! use tracing_mock::layer;
//!
//! # fn main() -> Result<(), TestFailure> {
//! let span = expect::span().named("my_span");
//! let (layer, handle) = layer::mock()
//!     // Enter a matching span
//!     .enter(&span)
//!     // Record an event with message "collect parting message"
//!     .event(expect::event().with_fields(expect::msg("say hello")))
//!     // Exit a matching span
//!     .exit(&span)
//!     // Expect no further messages to be recorded
//!     .only()
//!     // Return the subscriber and handle
//!     .run_with_handle();
//!
//! drop(layer);
//! let result = handle.finished();
//! ensure(
//!   result.is_err(),
//!   "unmet mock layer expectations return an error",
//! )?;
//! # Ok(())
//! # }
//! ```
//!
//! [`Layer`]: trait@tracing_subscriber::layer::Layer
//! [`MockHandle::finished`]: fn@crate::subscriber::MockHandle::finished
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;
use tracing_core::Event;
use tracing_core::Subscriber;
use tracing_core::span::Attributes;
use tracing_core::span::Id;
use tracing_core::span::Record;
use tracing_core::subscriber::SubscriberResult;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::registry::Scope;
use tracing_subscriber::registry::SpanRef;

use crate::ancestry::ActualAncestry;
use crate::ancestry::HasAncestry;
use crate::ancestry::get_ancestry;
use crate::event::ExpectedEvent;
use crate::expect::Expect;
use crate::failure::ExpectationError;
use crate::failure::ExpectationResult;
use crate::failure::SharedFailures;
use crate::span::ActualSpan;
use crate::span::ExpectedSpan;
use crate::span::NewSpan;
use crate::subscriber::MockHandle;

/// Create a [`MockLayerBuilder`] used to construct a
/// [`MockLayer`].
///
/// For additional information and examples, see the [`layer`]
/// module and [`MockLayerBuilder`] documentation.
///
/// # Examples
///
/// ```no_run
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::layer;
/// use tracing_subscriber::Layer;
/// use tracing_subscriber::layer::SubscriberExt;
/// use tracing_subscriber::util::SubscriberInitExt;
///
/// let span = expect::span().named("my_span");
/// let (layer, handle) = layer::mock()
///     // Enter a matching span
///     .enter(&span)
///     // Record an event with message "collect parting message"
///     .event(expect::event().with_fields(expect::msg("say hello")))
///     // Exit a matching span
///     .exit(&span)
///     // Expect no further messages to be recorded
///     .only()
///     // Return the subscriber and handle
///     .run_with_handle();
///
/// // Use `set_default` to apply the `MockSubscriber` until the end
/// // of the current scope (when the guard `_subscriber` is dropped).
/// let _subscriber = tracing_subscriber::registry()
///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
///   .set_default();
///
/// {
///   let span = tracing::trace_span!("my_span", greeting = "hello world",);
///
///   let _guard = span.enter();
///   tracing::info!("say hello");
/// }
///
/// // Use the handle to check the expectations. This line returns an error if
/// // an expectation is not met.
/// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
/// # Ok(())
/// # }
/// ```
///
/// [`layer`]: mod@crate::layer
#[must_use]
#[allow(
  clippy::single_call_fn,
  reason = "public DSL constructor is the documented entry point for mock layers"
)]
pub fn mock() -> MockLayerBuilder {
  MockLayerBuilder {
    expected: VecDeque::default(),
    name:     thread::current().name().map(String::from).unwrap_or_default(),
  }
}

/// Create a [`MockLayerBuilder`] with a name already set.
///
/// This constructor is equivalent to calling
/// [`MockLayerBuilder::named`] in the following way"
/// `layer::mock().named(name)`.
///
/// For additional information and examples, see the [`layer`]
/// module and [`MockLayerBuilder`] documentation.
///
/// # Examples
///
/// The example from [`MockLayerBuilder::named`] could be rewritten as:
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::layer;
/// use tracing_subscriber::Layer;
/// use tracing_subscriber::layer::SubscriberExt;
/// use tracing_subscriber::util::SubscriberInitExt;
///
/// let (layer_1, handle_1) = layer::named("subscriber-1").event(expect::event()).run_with_handle();
///
/// let (layer_2, handle_2) = layer::named("subscriber-2").event(expect::event()).run_with_handle();
///
/// let _subscriber = tracing_subscriber::registry()
///   .with(layer_2.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
///   .set_default();
/// {
///   let _subscriber = tracing_subscriber::registry()
///     .with(layer_1.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
///     .set_default();
///
///   tracing::info!("a");
/// }
///
/// let handle_1_result = handle_1.finished();
/// let handle_2_result = handle_2.finished();
/// strict_test_support::ensure(
///   handle_1_result.is_err() || handle_2_result.is_err(),
///   "mock expectation mismatch returns an error",
/// )?;
/// # Ok(())
/// # }
/// ```
///
/// [`MockLayerBuilder::named`]: fn@crate::layer::MockLayerBuilder::named
/// [`layer`]: mod@crate::layer
#[must_use]
pub fn named(name: impl fmt::Display) -> MockLayerBuilder {
  mock().named(name)
}

/// A builder for constructing [`MockLayer`]s.
///
/// The methods on this builder set expectations which are then
/// validated by the constructed [`MockLayer`].
///
/// For a detailed description and examples see the documentation
/// for the methods and the [`layer`] module.
///
/// [`layer`]: mod@crate::layer

#[derive(Debug)]
pub struct MockLayerBuilder {
  /// Pending expectations.
  expected: VecDeque<Expect>,
  /// Name used in failure messages.
  name:     String,
}

/// A layer which validates the traces it receives.
///
/// A `MockLayer` is constructed with a
/// [`MockLayerBuilder`]. For a detailed description and examples,
/// see the documentation for that struct and for the [`layer`]
/// module.
///
/// [`layer`]: mod@crate::layer
pub struct MockLayer {
  /// Pending expectations.
  expected: Arc<Mutex<VecDeque<Expect>>>,
  /// First expectation failure observed while the layer is running.
  failures: SharedFailures,
  /// Current entered span stack.
  current:  Mutex<Vec<Id>>,
  /// Name used in failure messages.
  name:     String,
}

impl MockLayerBuilder {
  /// Overrides the name printed by the mock layer's debugging output.
  ///
  /// The debugging output is displayed if the test fails, or if the test is
  /// run with `--nocapture`.
  ///
  /// By default, the mock layer's name is the  name of the test
  /// (*technically*, the name of the thread where it was created, which is
  /// the name of the test unless tests are run with `--test-threads=1`).
  /// When a test has only one mock layer, this is sufficient. However,
  /// some tests may include multiple layers, in order to test
  /// interactions between multiple layers. In that case, it can be
  /// helpful to give each layers a separate name to distinguish where the
  /// debugging output comes from.
  ///
  /// # Examples
  ///
  /// In the following example, we create two layers, both
  /// expecting to receive an event. As we only record a single
  /// event, the test will fail:
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer_1, handle_1) = layer::mock().named("layer-1").event(expect::event()).run_with_handle();
  ///
  /// let (layer_2, handle_2) = layer::mock().named("layer-2").event(expect::event()).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer_2.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  /// {
  ///   let _subscriber = tracing_subscriber::registry()
  ///     .with(layer_1.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///     .set_default();
  ///
  ///   tracing::info!("a");
  /// }
  ///
  /// let handle_1_result = handle_1.finished();
  /// let handle_2_result = handle_2.finished();
  /// strict_test_support::ensure(
  ///   handle_1_result.is_err() || handle_2_result.is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// In the test output, we see that the layer which didn't
  /// received the event was the one named `layer-2`, which is
  /// correct as the layer named `layer-1` was the default
  /// when the event was recorded:
  ///
  /// ```text
  /// [main::layer-2] more notifications expected: [
  ///     Event(
  ///         MockEvent,
  ///     ),
  /// ]', tracing-mock/src/subscriber.rs:472:13
  /// ```
  #[must_use]
  pub fn named(mut self, name: impl fmt::Display) -> Self {
    if self.name.is_empty() {
      self.name = name.to_string();
    } else {
      self.name.push_str("::");
      self.name.push_str(&name.to_string());
    }
    self
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
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer, handle) = layer::mock().event(expect::event()).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// tracing::info!("event");
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// A span is entered before the event, causing the test to fail:
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer, handle) = layer::mock().event(expect::event()).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// let span = tracing::info_span!("span");
  /// let _guard = span.enter();
  /// tracing::info!("event");
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
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
  /// [`ExpectedSpan`] directly. [`NewSpan`] can be used to test
  /// span fields and the span ancestry.
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
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let span = expect::span()
  ///   .at_level(tracing::Level::INFO)
  ///   .named("the span we're testing")
  ///   .with_fields(expect::field("testing").with_value(&"yes"));
  /// let (layer, handle) = layer::mock().new_span(span).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// _ = tracing::info_span!("the span we're testing", testing = "yes");
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// An unmet span expectation can be inspected through a returned error:
  ///
  /// ```
  /// use strict_test_support::TestFailure;
  /// use strict_test_support::ensure;
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  ///
  /// # fn main() -> Result<(), TestFailure> {
  /// let span = expect::span()
  ///   .at_level(tracing::Level::INFO)
  ///   .named("the span we're testing")
  ///   .with_fields(expect::field("testing").with_value(&"yes"));
  /// let (layer, handle) = layer::mock().new_span(span).run_with_handle();
  ///
  /// drop(layer);
  /// let result = handle.finished();
  /// ensure(result.is_err(), "missing new span returns an error")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`ExpectedSpan`]: struct@crate::span::ExpectedSpan
  /// [`NewSpan`]: struct@crate::span::NewSpan
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
  /// [`exit`], since an entered span will typically be exited. If used
  /// together with [`only`], this is likely necessary, because the span
  /// will be dropped before the test completes (except in rare cases,
  /// such as if [`std::mem::forget`] is used).
  ///
  /// If the span that is entered doesn't match the [`ExpectedSpan`],
  /// or if something else (such as an event) is recorded first,
  /// then the expectation will fail.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let span = expect::span().at_level(tracing::Level::INFO).named("the span we're testing");
  /// let (layer, handle) = layer::mock().enter(&span).exit(&span).only().run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// {
  ///   let span = tracing::info_span!("the span we're testing");
  ///   let _entered = span.enter();
  /// }
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// An unmet enter expectation can be inspected through a returned error:
  ///
  /// ```
  /// use strict_test_support::TestFailure;
  /// use strict_test_support::ensure;
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  ///
  /// # fn main() -> Result<(), TestFailure> {
  /// let span = expect::span().at_level(tracing::Level::INFO).named("the span we're testing");
  /// let (layer, handle) = layer::mock().enter(&span).exit(&span).only().run_with_handle();
  ///
  /// drop(layer);
  /// let result = handle.finished();
  /// ensure(result.is_err(), "missing span enter returns an error")?;
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

  /// Adds an expectation that exiting a span matching the
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
  /// **Note**: Ensure that the guard returned by [`Span::enter`]
  /// is dropped before calling [`MockHandle::finished`].
  ///
  /// # Examples
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let span = expect::span().at_level(tracing::Level::INFO).named("the span we're testing");
  /// let (layer, handle) = layer::mock().enter(&span).exit(&span).only().run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  /// {
  ///   let span = tracing::info_span!("the span we're testing");
  ///   let _entered = span.enter();
  /// }
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// An unmet exit expectation can be inspected through a returned error:
  ///
  /// ```
  /// use strict_test_support::TestFailure;
  /// use strict_test_support::ensure;
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  ///
  /// # fn main() -> Result<(), TestFailure> {
  /// let span = expect::span().at_level(tracing::Level::INFO).named("the span we're testing");
  /// let (layer, handle) = layer::mock().enter(&span).exit(&span).only().run_with_handle();
  ///
  /// drop(layer);
  /// let result = handle.finished();
  /// ensure(result.is_err(), "missing span exit returns an error")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`enter`]: fn@Self::enter
  /// [`MockHandle::finished`]: fn@crate::subscriber::MockHandle::finished
  /// [`Span::enter`]: fn@tracing::Span::enter
  #[must_use]
  pub fn exit<S>(mut self, span: S) -> Self
  where
    S: Into<ExpectedSpan>,
  {
    self.expected.push_back(Expect::Exit(span.into()));
    self
  }

  /// Adds an expectation that closing a span matching the
  /// [`ExpectedSpan`] will be recorded next.
  ///
  /// This expectation matches [`Layer::on_close`], which is called
  /// when the subscriber considers the span fully closed.
  ///
  /// If the span that is closed doesn't match the [`ExpectedSpan`],
  /// or if something else (such as an event) is recorded first,
  /// then the expectation will fail.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let span = expect::span().at_level(tracing::Level::INFO).named("the span we're testing");
  /// let (layer, handle) = layer::mock().close_span(&span).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// _ = tracing::info_span!("the span we're testing");
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`Layer::on_close`]: tracing_subscriber::layer::Layer::on_close
  #[must_use]
  pub fn close_span<S>(mut self, span: S) -> Self
  where
    S: Into<ExpectedSpan>,
  {
    self.expected.push_back(Expect::CloseSpan(span.into()));
    self
  }

  /// Adds an expectation that [`Layer::on_register_dispatch`] will
  /// be called next.
  ///
  /// **Note**: This expectation is usually fulfilled automatically when
  /// a layer (wrapped in a subscriber) is set as the default via
  /// [`tracing::subscriber::with_default`] or
  /// [`tracing::subscriber::set_global_default`], so explicitly expecting
  /// this is not usually necessary. However, it may be useful when testing
  /// custom layer implementations that manually call `on_register_dispatch`.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer, handle) = layer::mock().on_register_dispatch().run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// // The layer's on_register_dispatch was called when the subscriber was set as default
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// If the `on_register_dispatch` call doesn't make it to the `MockLayer`,
  /// in case it's wrapped in another Layer that doesn't forward the call,
  /// then the expectation will fail.
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// # use std::marker::PhantomData;
  ///
  /// # use tracing::{Event, Subscriber};
  /// # use tracing_mock::layer;
  /// # use tracing_subscriber::{
  /// #     layer::{Context, SubscriberExt},
  /// #     util::SubscriberInitExt,
  /// #     Layer,
  /// # };
  ///
  /// struct WrapLayer<S: Subscriber, L: Layer<S>> {
  ///   inner: L,
  ///   _pd:   PhantomData<S>,
  /// }
  ///
  /// impl<S: Subscriber, L: Layer<S>> Layer<S> for WrapLayer<S, L> {
  ///   fn on_register_dispatch(
  ///     &self,
  ///     subscriber: &tracing::Dispatch,
  ///   ) -> tracing_core::subscriber::SubscriberResult {
  ///     // Doesn't forward to `self.inner`
  ///     let _ = subscriber;
  ///     Ok(())
  ///   }
  ///
  ///   fn on_event(
  ///     &self,
  ///     event: &Event<'_>,
  ///     ctx: Context<'_, S>,
  ///   ) -> tracing_core::subscriber::SubscriberResult {
  ///     self.inner.on_event(event, ctx)
  ///   }
  /// }
  /// let (layer, handle) = layer::mock().on_register_dispatch().run_with_handle();
  /// let wrap_layer = WrapLayer {
  ///   inner: layer,
  ///   _pd:   PhantomData::<_>,
  /// };
  ///
  /// let subscriber = tracing_subscriber::registry().with(wrap_layer).set_default();
  ///
  /// // The layer's on_register_dispatch is called when the subscriber is set as default
  /// drop(subscriber);
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`Layer::on_register_dispatch`]: tracing_subscriber::layer::Layer::on_register_dispatch
  #[must_use]
  pub fn on_register_dispatch(mut self) -> Self {
    self.expected.push_back(Expect::OnRegisterDispatch);
    self
  }

  /// Expects that no further traces are received.
  ///
  /// The call to `only` should appear immediately before the final
  /// call to [`run`] or [`run_with_handle`], as any expectations which
  /// are added after `only` will not be considered.
  ///
  /// # Examples
  ///
  /// Consider this simple test. It passes even though we only
  /// expect a single event, but receive three:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer, handle) = layer::mock().event(expect::event()).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// tracing::info!("a");
  /// tracing::info!("b");
  /// tracing::info!("c");
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
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer, handle) = layer::mock().event(expect::event()).only().run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// tracing::info!("a");
  /// tracing::info!("b");
  /// tracing::info!("c");
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`run`]: fn@Self::run
  /// [`run_with_handle`]: fn@Self::run_with_handle
  #[must_use]
  pub fn only(mut self) -> Self {
    self.expected.push_back(Expect::Nothing);
    self
  }

  /// Consume this builder and return a [`MockLayer`] which can
  /// be set as the default subscriber.
  ///
  /// This function is similar to [`run_with_handle`], but it doesn't
  /// return a [`MockHandle`]. This is useful if the desired
  /// expectations can be checked externally to the subscriber.
  ///
  /// # Examples
  ///
  /// The following test is used within the `tracing-subscriber`
  /// codebase:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::Subscriber;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let unfiltered = layer::named("unfiltered").run().boxed();
  /// let info = layer::named("info").run().with_filter(tracing_core::LevelFilter::INFO).boxed();
  /// let debug = layer::named("debug")
  ///   .run()
  ///   .with_filter(tracing_core::LevelFilter::DEBUG)
  ///   .boxed();
  ///
  /// let subscriber = tracing_subscriber::registry().with(vec![unfiltered, info, debug]);
  ///
  /// strict_test_support::ensure(
  ///   subscriber.max_level_hint().is_none(),
  ///   "mock layer stack reports no max-level hint",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`MockHandle`]: struct@crate::subscriber::MockHandle
  /// [`run_with_handle`]: fn@Self::run_with_handle
  #[must_use]
  pub fn run(self) -> MockLayer {
    MockLayer {
      expected: Arc::new(Mutex::new(self.expected)),
      failures: SharedFailures::default(),
      name:     self.name,
      current:  Mutex::new(Vec::new()),
    }
  }

  /// Consume this builder and return a [`MockLayer`] which can
  /// be set as the default subscriber and a [`MockHandle`] which can
  /// be used to validate the provided expectations.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  /// use tracing_subscriber::Layer;
  /// use tracing_subscriber::layer::SubscriberExt;
  /// use tracing_subscriber::util::SubscriberInitExt;
  ///
  /// let (layer, handle) = layer::mock().event(expect::event()).run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// tracing::info!("event");
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`MockHandle`]: struct@crate::subscriber::MockHandle
  /// [`MockLayer`]: struct@crate::layer::MockLayer
  #[must_use]
  pub fn run_with_handle(self) -> (MockLayer, MockHandle) {
    let expected = Arc::new(Mutex::new(self.expected));
    let failures = SharedFailures::default();
    let handle = MockHandle::new(Arc::clone(&expected), failures.clone(), self.name.clone());
    let subscriber = MockLayer {
      expected,
      failures,
      name: self.name,
      current: Mutex::new(Vec::new()),
    };
    (subscriber, handle)
  }
}

impl<'a, S> From<&SpanRef<'a, S>> for ActualSpan
where
  S: LookupSpan<'a>,
{
  fn from(span_ref: &SpanRef<'a, S>) -> Self {
    Self::new(span_ref.id(), Some(span_ref.metadata()))
  }
}

impl MockLayer {
  /// Records a formatted expectation failure.
  fn record_failure(&self, args: fmt::Arguments<'_>) {
    self.failures.record(ExpectationError::from_args(args));
  }

  /// Records a fallible expectation result.
  fn record_result<T>(&self, result: ExpectationResult<T>) -> Option<T> {
    self.failures.record_result(result)
  }

  /// Checks an event's observed scope against expected spans.
  fn check_event_scope<C>(&self, current_scope: Option<Scope<'_, C>>, expected_scope: &mut [ExpectedSpan]) -> ExpectationResult
  where
    C: for<'lookup> LookupSpan<'lookup>,
  {
    let mut observed_scope = current_scope.into_iter().flatten();
    let mut matched_count = 0_usize;
    for (expected, actual) in expected_scope.iter_mut().zip(&mut observed_scope) {
      let actual_span = ActualSpan::from(&actual);
      expected.check(
        &actual_span,
        format_args!("the {matched_count}th span in the event's scope to be"),
        &self.name,
      )?;
      matched_count = matched_count
        .checked_add(1)
        .ok_or_else(|| ExpectationError::from_args(format_args!("[{}] event scope match count overflowed", self.name)))?;
    }
    let mut missing = String::new();
    for expected in expected_scope.iter().skip(matched_count) {
      if !missing.is_empty() {
        missing.push_str(", ");
      }
      missing.push_str(&expected.to_string());
    }
    if !missing.is_empty() {
      return Err(ExpectationError::from_args(format_args!(
        "\n[{}] did not observe all expected spans in event scope!\n[{}] missing: {}",
        self.name, self.name, missing
      )));
    }
    if observed_scope.next().is_some() {
      return Err(ExpectationError::from_args(format_args!(
        "\n[{}] did not expect all spans in the actual event scope!",
        self.name
      )));
    }
    Ok(())
  }
}

impl<C> Layer<C> for MockLayer
where
  C: Subscriber + for<'a> LookupSpan<'a>,
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

  fn register_callsite(&self, _metadata: &'static tracing::Metadata<'static>) -> SubscriberResult<tracing_core::Interest> {
    Ok(tracing_core::Interest::always())
  }

  fn on_record(&self, _: Id, _: &Record<'_>, _: Context<'_, C>) -> SubscriberResult {
    self.record_failure(format_args!(
      "so far, we don't have any tests that need an `on_record` implementation.\nif you just wrote one that does, feel free to implement \
       it!"
    ));
    Ok(())
  }

  fn on_event(&self, event: &Event<'_>, cx: Context<'_, C>) -> SubscriberResult {
    let name = event.metadata().name();
    let next = {
      let mut expected = self.expected.lock();
      expected.pop_front()
    };
    match next {
      None => {}
      Some(Expect::Event(mut expected)) => {
        if self
          .record_result(expected.check(event, || context_get_ancestry(&event, &cx), &self.name))
          .is_some()
          && let Some(expected_scope) = expected.scope_mut()
        {
          let _result = self.record_result(self.check_event_scope(cx.event_scope(event), expected_scope));
        }
      }
      Some(ex) => {
        let _result = self.record_result(ex.bad(&self.name, format_args!("observed event `{name}`")));
      }
    }
    Ok(())
  }

  fn on_follows_from(&self, _span: Id, _follows: Id, _: Context<'_, C>) -> SubscriberResult {
    self.record_failure(format_args!(
      "so far, we don't have any tests that need an `on_follows_from` implementation.\nif you just wrote one that does, feel free to \
       implement it!"
    ));
    Ok(())
  }

  fn on_new_span(&self, span: &Attributes<'_>, id: Id, cx: Context<'_, C>) -> SubscriberResult {
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
        self.record_failure(format_args!("[{}] could not set expected span ID: {}", self.name, error));
      }

      let _result = self.record_result(expected_span.check(span, || context_get_ancestry(&span, &cx), &self.name));
    }
    Ok(())
  }

  fn on_enter(&self, id: Id, cx: Context<'_, C>) -> SubscriberResult {
    let Some(span) = cx.span(id) else {
      self.record_failure(format_args!("[{}] no span for ID `{}`", self.name, id.into_u64()));
      return Ok(());
    };
    let next = {
      let mut expected = self.expected.lock();
      expected.pop_front()
    };
    match next {
      None => {}
      Some(Expect::Enter(ref expected_span)) => {
        let _result = self.record_result(expected_span.check(&(&span).into(), "to enter", &self.name));
      }
      Some(ex) => {
        let _result = self.record_result(ex.bad(&self.name, format_args!("entered span `{}`", span.name())));
      }
    }
    self.current.lock().push(id);
    Ok(())
  }

  fn on_exit(&self, id: Id, cx: Context<'_, C>) -> SubscriberResult {
    let Some(span) = cx.span(id) else {
      self.record_failure(format_args!("[{}] no span for ID `{}`", self.name, id.into_u64()));
      return Ok(());
    };
    let next = {
      let mut expected = self.expected.lock();
      expected.pop_front()
    };
    match next {
      None => {}
      Some(Expect::Exit(ref expected_span)) => {
        let _result = self.record_result(expected_span.check(&(&span).into(), "to exit", &self.name));
        let curr = self.current.lock().pop();
        if curr.as_ref() != Some(&id) {
          let current_name = curr
            .as_ref()
            .and_then(|current_id| cx.span(*current_id))
            .map_or("<unknown>", |state| state.name());
          self.record_failure(format_args!(
            "[{}] exited span `{}`, but the current span was `{}`",
            self.name,
            span.name(),
            current_name
          ));
        }
      }
      Some(ex) => {
        let _result = self.record_result(ex.bad(&self.name, format_args!("exited span `{}`", span.name())));
      }
    }
    Ok(())
  }

  fn on_close(&self, id: Id, cx: Context<'_, C>) -> SubscriberResult {
    let span = cx.span(id);
    let name = span.as_ref().map(SpanRef::name);
    if let Some(mut expected) = self.expected.try_lock() {
      let was_expected = match expected.front() {
        Some(expectation) if expectation.close_span().is_some() => {
          let Some(expected_span) = expectation.close_span() else {
            return Ok(());
          };
          if let Some(ref observed_span) = span {
            let _result = self.record_result(expected_span.check(&observed_span.into(), "to close a span", &self.name));
          } else {
            let actual_span = (&id).into();
            let _result = self.record_result(expected_span.check(&actual_span, "to close a span", &self.name));
          }
          true
        }
        Some(&Expect::Event(_)) => {
          self.record_failure(format_args!(
            "[{}] expected an event, but closed span {} (id={}) instead",
            self.name,
            name.unwrap_or("<unknown name>"),
            id.into_u64()
          ));
          true
        }
        Some(_) | None => false,
      };
      if was_expected {
        let _matched = expected.pop_front();
      }
    }
    Ok(())
  }

  fn on_id_change(&self, _old: Id, _new: Id, _ctx: Context<'_, C>) -> SubscriberResult {
    self.record_failure(format_args!("well-behaved subscribers should never change span IDs"));
    Ok(())
  }
}

/// Resolves ancestry through a layer context.
fn context_get_ancestry<C>(item: &impl HasAncestry, ctx: &Context<'_, C>) -> ExpectationResult<ActualAncestry>
where
  C: Subscriber + for<'a> LookupSpan<'a>,
{
  get_ancestry(
    item,
    || ctx.lookup_current().map(|span_ref| span_ref.id()),
    |span_id| ctx.span(*span_id).map(|span_ref| (&span_ref).into()),
  )
}

impl fmt::Debug for MockLayer {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut debug = f.debug_struct("ExpectSubscriber");
    let _name = debug.field("name", &self.name);

    if let Some(expected) = self.expected.try_lock() {
      let _expected = debug.field("expected", &expected);
    } else {
      let _expected = debug.field("expected", &format_args!("<locked>"));
    }
    let _failures = debug.field("failures", &self.failures);

    if let Some(current) = self.current.try_lock() {
      let mut current_ids = String::new();
      for id in &*current {
        if !current_ids.is_empty() {
          current_ids.push_str(", ");
        }
        current_ids.push_str(&id.into_u64().to_string());
      }
      let _current = debug.field("current", &current_ids);
    } else {
      let _current = debug.field("current", &format_args!("<locked>"));
    }

    debug.finish()
  }
}
