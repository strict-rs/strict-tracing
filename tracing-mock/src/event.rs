//! An [`ExpectedEvent`] defines an event to be matched by the mock
//! subscriber API in the [`subscriber`] module.
//!
//! The expected event should be created with [`expect::event`] and a
//! chain of method calls to describe the expectations we wish to make
//! about the event.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing::subscriber::with_default;
//! use tracing_mock::expect;
//! use tracing_mock::subscriber;
//!
//! let event = expect::event()
//!   .at_level(tracing::Level::INFO)
//!   .with_fields(expect::field("field.name").with_value(&"field_value"));
//!
//! let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
//!
//! with_default(subscriber, || {
//!   tracing::info!(field.name = "field_value");
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! [`subscriber`]: mod@crate::subscriber
//! [`expect::event`]: fn@crate::expect::event
use std::fmt;

use crate::ancestry::ActualAncestry;
use crate::ancestry::ExpectedAncestry;
use crate::failure::ExpectationError;
use crate::failure::ExpectationResult;
use crate::field;
use crate::metadata::ExpectedMetadata;
use crate::metadata::display_level;
use crate::span;

/// An expected event.
///
/// For a detailed description and examples, see the documentation for
/// the methods and the [`event`] module.
///
/// [`event`]: mod@crate::event
#[derive(Default, Eq, PartialEq)]
pub struct ExpectedEvent {
  /// Field expectations for this event.
  pub(super) fields:   Option<field::ExpectedFields>,
  /// Ancestry expectations for this event.
  pub(super) ancestry: Option<ExpectedAncestry>,
  /// Scope expectations for this event.
  pub(super) in_spans: Option<Vec<span::ExpectedSpan>>,
  /// Metadata expectations for this event.
  pub(super) metadata: ExpectedMetadata,
}

impl ExpectedEvent {
  /// Sets a name to expect when matching an event.
  ///
  /// By default, an event's name takes takes the form:
  /// `event <file>:<line>` where `<file>` and `<line>` refer to the
  /// location in the source code where the event was generated.
  ///
  /// To override the name of an event, it has to be constructed
  /// directly, rather than by using the `tracing` crate's macros.
  ///
  /// In general, there are not many use cases for expecting an
  /// event with a particular name, as the value includes the file
  /// name and line number. Assertions about event names are
  /// therefore quite fragile, since they will change as the source
  /// code is modified.
  #[must_use]
  pub fn named<I>(self, name: I) -> Self
  where
    I: Into<String>,
  {
    Self {
      metadata: ExpectedMetadata {
        name: Some(name.into()),
        ..self.metadata
      },
      ..self
    }
  }

  /// Adds fields to expect when matching an event.
  ///
  /// If an event is recorded with fields that do not match the provided
  /// [`ExpectedFields`], this expectation will fail.
  ///
  /// If the provided field is not present on the recorded event, or
  /// if the value for that field is different, then the expectation
  /// will fail.
  ///
  /// More information on the available validations is available in
  /// the [`ExpectedFields`] documentation.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(expect::field("field.name").with_value(&"field_value"));
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///   tracing::info!(field.name = "field_value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// A different field value will cause the expectation to fail:
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_fields(expect::field("field.name").with_value(&"field_value"));
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///   tracing::info!(field.name = "different_field_value");
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`ExpectedFields`]: struct@crate::field::ExpectedFields
  #[must_use]
  pub fn with_fields<I>(self, fields: I) -> Self
  where
    I: Into<field::ExpectedFields>,
  {
    Self {
      fields: Some(fields.into()),
      ..self
    }
  }

  /// Sets the [`Level`](tracing::Level) to expect when matching an event.
  ///
  /// If an event is recorded at a different level, this expectation
  /// will fail.
  ///
  /// # Examples
  ///
  /// ```no_run
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().at_level(tracing::Level::WARN);
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///   tracing::warn!("this message is bad news");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// Expecting an event at `INFO` level will fail if the event is
  /// recorded at any other level:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().at_level(tracing::Level::INFO);
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///   tracing::warn!("this message is bad news");
  /// });
  ///
  /// strict_test_support::ensure(
  ///   handle.finished().is_err(),
  ///   "mock expectation mismatch returns an error",
  /// )?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn at_level(self, level: tracing::Level) -> Self {
    Self {
      metadata: ExpectedMetadata {
        level: Some(level),
        ..self.metadata
      },
      ..self
    }
  }

  /// Sets the target to expect when matching events.
  ///
  /// If an event is recorded with a different target, this expectation will fail.
  ///
  /// # Examples
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::{expect, subscriber};
  ///
  /// let event = expect::event()
  ///     .with_target("some_target");
  ///
  /// let (subscriber, handle) = subscriber::mock()
  ///     .event(event)
  ///     .run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///     tracing::info!(target: "some_target", field = &"value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// The test will fail if the target is different:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::{expect, subscriber};
  ///
  /// let event = expect::event()
  ///     .with_target("some_target");
  ///
  /// let (subscriber, handle) = subscriber::mock()
  ///     .event(event)
  ///     .run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///     tracing::info!(target: "a_different_target", field = &"value");
  /// });
  ///
  /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn with_target<I>(self, target: I) -> Self
  where
    I: Into<String>,
  {
    Self {
      metadata: ExpectedMetadata {
        target: Some(target.into()),
        ..self.metadata
      },
      ..self
    }
  }

  /// Configures this `ExpectedEvent` to expect the specified
  /// [`ExpectedAncestry`]. An event's ancestry indicates whether is has a
  /// parent or is a root, and whether the parent is explicitly or
  /// contextually assigned.
  ///
  /// An _explicit_ parent span is one passed to the `event!` macro in the
  /// `parent:` field. If no `parent:` field is specified, then the event
  /// will have a contextually determined parent or be a contextual root if
  /// there is no parent.
  ///
  /// If the parent is different from the provided one, this expectation
  /// will fail.
  ///
  /// # Examples
  ///
  /// An explicit or contextual can be matched on an `ExpectedSpan`.
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::{expect, subscriber};
  ///
  /// let parent = expect::span()
  ///     .named("parent_span")
  ///     .with_target("custom-target")
  ///     .at_level(tracing::Level::INFO);
  /// let event = expect::event()
  ///     .with_ancestry(expect::has_explicit_parent(parent));
  ///
  /// let (subscriber, handle) = subscriber::mock()
  ///     .event(event)
  ///     .run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///     let parent = tracing::info_span!(target: "custom-target", "parent_span");
  ///     tracing::info!(parent: parent.id(), field = &"value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  /// The functions `expect::has_explicit_parent` and
  /// `expect::has_contextual_parent` take `Into<ExpectedSpan>`, so a string
  /// passed directly will match on a span with that name, or an
  /// [`ExpectedId`] can be passed to match a span with that Id.
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::{expect, subscriber};
  ///
  /// let event = expect::event()
  ///     .with_ancestry(expect::has_explicit_parent("parent_span"));
  ///
  /// let (subscriber, handle) = subscriber::mock()
  ///     .event(event)
  ///     .run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///     let parent = tracing::info_span!("parent_span");
  ///     tracing::info!(parent: parent.id(), field = &"value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// In the following example, we expect that the matched event is
  /// an explicit root:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::{expect, subscriber};
  ///
  /// let event = expect::event()
  ///     .with_ancestry(expect::is_explicit_root());
  ///
  /// let (subscriber, handle) = subscriber::mock()
  ///     .enter(expect::span())
  ///     .event(event)
  ///     .run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///     let _guard = tracing::info_span!("contextual parent").entered();
  ///     tracing::info!(parent: None, field = &"value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// When `expect::has_contextual_parent("parent_name")` is passed to
  /// `with_ancestry` then the provided string is the name of the contextual
  /// parent span to expect.
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_ancestry(expect::has_contextual_parent("parent_span"));
  ///
  /// let (subscriber, handle) =
  ///   subscriber::mock().enter(expect::span()).event(event).run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///   let parent = tracing::info_span!("parent_span");
  ///   let _guard = parent.enter();
  ///   tracing::info!(field = &"value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// Matching an event recorded outside of a span, a contextual
  /// root:
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::expect;
  /// use tracing_mock::subscriber;
  ///
  /// let event = expect::event().with_ancestry(expect::is_contextual_root());
  ///
  /// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///   tracing::info!(field = &"value");
  /// });
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// In the example below, the expectation fails because the event is
  /// recorded with an explicit parent, however a contextual parent is
  /// expected.
  ///
  /// ```
  /// # fn main() -> Result<(), strict_test_support::TestFailure> {
  /// use tracing::subscriber::with_default;
  /// use tracing_mock::{expect, subscriber};
  ///
  /// let event = expect::event()
  ///     .with_ancestry(expect::has_contextual_parent("parent_span"));
  ///
  /// let (subscriber, handle) = subscriber::mock()
  ///     .enter(expect::span())
  ///     .event(event)
  ///     .run_with_handle();
  ///
  /// with_default(subscriber, || {
  ///     let parent = tracing::info_span!("parent_span");
  ///     tracing::info!(parent: parent.id(), field = &"value");
  /// });
  ///
  /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`ExpectedId`]: struct@crate::span::ExpectedId
  #[must_use]
  pub fn with_ancestry(self, ancenstry: ExpectedAncestry) -> Self {
    Self {
      ancestry: Some(ancenstry),
      ..self
    }
  }

  /// Validates that the event is emitted within the scope of the
  /// provided `spans`.
  ///
  /// The spans must be provided reverse hierarchy order, so the
  /// closest span to the event would be first, followed by its
  /// parent, and so on.
  ///
  /// If the spans provided do not match the hierarchy of the
  /// recorded event, the expectation will fail.
  ///
  /// **Note**: This validation currently only works with a
  /// [`MockLayer`]. If used with a [`MockSubscriber`], the
  /// expectation will fail directly as it is unimplemented.
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
  /// let event = expect::event()
  ///   .in_scope([expect::span().named("parent_span"), expect::span().named("grandparent_span")]);
  ///
  /// let (layer, handle) = layer::mock()
  ///   .enter(expect::span())
  ///   .enter(expect::span())
  ///   .event(event)
  ///   .run_with_handle();
  ///
  /// let _subscriber = tracing_subscriber::registry()
  ///   .with(layer.with_filter(tracing_subscriber::filter::filter_fn(move |_meta| true)))
  ///   .set_default();
  ///
  /// let grandparent = tracing::info_span!("grandparent_span");
  /// let _gp_guard = grandparent.enter();
  /// let parent = tracing::info_span!("parent_span");
  /// let _p_guard = parent.enter();
  /// tracing::info!(field = &"value");
  ///
  /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// Unmet scope expectations can be inspected through returned errors:
  ///
  /// ```
  /// use strict_test_support::TestFailure;
  /// use strict_test_support::ensure;
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  ///
  /// # fn main() -> Result<(), TestFailure> {
  /// let event = expect::event()
  ///   .in_scope([expect::span().named("parent_span"), expect::span().named("grandparent_span")]);
  ///
  /// let (layer, handle) = layer::mock().enter(expect::span()).event(event).run_with_handle();
  ///
  /// drop(layer);
  /// let result = handle.finished();
  /// ensure(result.is_err(), "missing scoped event returns an error")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// It is also possible to test that an event has no parent spans
  /// by passing `None` to `in_scope`. If the event is within a
  /// span, the test will fail:
  ///
  /// ```
  /// use strict_test_support::TestFailure;
  /// use strict_test_support::ensure;
  /// use tracing_mock::expect;
  /// use tracing_mock::layer;
  ///
  /// # fn main() -> Result<(), TestFailure> {
  /// let event = expect::event().in_scope(None);
  ///
  /// let (layer, handle) = layer::mock().enter(expect::span()).event(event).run_with_handle();
  ///
  /// drop(layer);
  /// let result = handle.finished();
  /// ensure(result.is_err(), "missing parentless event returns an error")?;
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [`MockLayer`]: struct@crate::layer::MockLayer
  /// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
  #[cfg(feature = "tracing-subscriber")]
  #[must_use]
  pub fn in_scope(self, spans: impl IntoIterator<Item = span::ExpectedSpan>) -> Self {
    Self {
      in_spans: Some(spans.into_iter().collect()),
      ..self
    }
  }

  /// Returns mutable scope expectations for layer event checks.
  #[cfg(feature = "tracing-subscriber")]
  pub(crate) fn scope_mut(&mut self) -> Option<&mut [span::ExpectedSpan]> {
    self.in_spans.as_deref_mut()
  }

  /// Checks an observed event against this expectation.
  pub(crate) fn check(
    &mut self,
    event: &tracing::Event<'_>,
    get_ancestry: impl FnOnce() -> ExpectationResult<ActualAncestry>,
    subscriber_name: &str,
  ) -> ExpectationResult {
    let meta = event.metadata();
    let name = meta.name();
    self.metadata.check(meta, format_args!("event \"{name}\""), subscriber_name)?;
    if !meta.is_event() {
      return Err(ExpectationError::from_args(format_args!(
        "[{}] expected {}, but got metadata `{}` with target `{}`",
        subscriber_name,
        self,
        meta.name(),
        meta.target()
      )));
    }
    if let Some(ref mut expected_fields) = self.fields {
      let mut checker = expected_fields.checker(name, subscriber_name);
      event.record(&mut checker);
      checker.finish()?;
    }

    if let Some(ref expected_ancestry) = self.ancestry {
      let actual_ancestry = get_ancestry()?;
      expected_ancestry.check(&actual_ancestry, event.metadata().name(), subscriber_name)?;
    }

    Ok(())
  }
}

impl fmt::Display for ExpectedEvent {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "an event{}", self.metadata)
  }
}

impl fmt::Debug for ExpectedEvent {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut debug = f.debug_struct("MockEvent");

    if let Some(ref name) = self.metadata.name {
      let _builder = debug.field("name", name);
    }

    if let Some(ref target) = self.metadata.target {
      let _builder = debug.field("target", target);
    }

    if let Some(ref level) = self.metadata.level {
      let _builder = debug.field("level", &format_args!("{}", display_level(*level)));
    }

    if let Some(ref fields) = self.fields {
      let _builder = debug.field("fields", fields);
    }

    if let Some(ref parent) = self.ancestry {
      let _builder = debug.field("parent", &format_args!("{parent}"));
    }

    if let Some(ref in_spans) = self.in_spans {
      let _builder = debug.field("in_spans", in_spans);
    }

    debug.finish()
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use tracing::subscriber::with_default;

  use crate::ancestry::ExpectedAncestry;
  use crate::expect;
  use crate::subscriber;

  #[test]
  fn event_metadata_and_fields_accept_matching_event() -> Result<(), TestFailure> {
    let event = expect::event()
      .with_target("mock_event_target")
      .at_level(tracing::Level::INFO)
      .with_fields(expect::field("answer").with_value(&42_i64));
    let (subscriber, handle) = subscriber::mock().event(event).only().run_with_handle();

    with_default(subscriber, || {
      tracing::info!(target: "mock_event_target", answer = 42_i64, "event metadata and fields match");
    });

    ensure_ok(handle.finished(), "matching event metadata and fields finish cleanly")
  }

  #[test]
  fn event_metadata_mismatch_is_reported_by_finished() -> Result<(), TestFailure> {
    let event = expect::event().at_level(tracing::Level::WARN);
    let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();

    with_default(subscriber, || {
      tracing::info!("event level does not match");
    });

    ensure(handle.finished().is_err(), "event level mismatch is reported")
  }

  #[test]
  fn event_contextual_parent_ancestry_matches_entered_span() -> Result<(), TestFailure> {
    let event = expect::event().with_ancestry(ExpectedAncestry::HasContextualParent(expect::span().named("parent_span")));
    let (subscriber, handle) = subscriber::mock()
      .new_span("parent_span")
      .enter("parent_span")
      .event(event)
      .exit("parent_span")
      .only()
      .run_with_handle();

    with_default(subscriber, || {
      let parent_span = tracing::info_span!("parent_span");
      let _guard = parent_span.enter();
      tracing::info!("inside parent");
    });

    ensure_ok(handle.finished(), "event ancestry matches the contextual parent span")
  }

  #[test]
  fn event_ancestry_mismatch_is_reported_by_finished() -> Result<(), TestFailure> {
    let event = expect::event().with_ancestry(ExpectedAncestry::IsContextualRoot);
    let (subscriber, handle) = subscriber::mock()
      .new_span("parent_span")
      .enter("parent_span")
      .event(event)
      .run_with_handle();

    with_default(subscriber, || {
      let parent_span = tracing::info_span!("parent_span");
      let _guard = parent_span.enter();
      tracing::info!("inside parent");
    });

    ensure(handle.finished().is_err(), "event ancestry mismatch is reported")
  }
}
