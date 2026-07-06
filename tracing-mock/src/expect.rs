//! Construct expectations for traces which should be received
//!
//! This module contains constructors for expectations defined
//! in the [`event`], [`span`], and [`field`] modules.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::expect;
//! use tracing_mock::subscriber;
//!
//! let (subscriber, handle) = subscriber::mock()
//!     // Expect an event with message
//!     .event(expect::event().with_fields(expect::msg("message")))
//!     .only()
//!     .run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!   tracing::info!("message");
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
use std::fmt;

use crate::ancestry::ExpectedAncestry;
use crate::event::ExpectedEvent;
use crate::failure::ExpectationError;
use crate::failure::ExpectationResult;
use crate::field::ExpectedField;
use crate::field::ExpectedFields;
use crate::field::ExpectedValue;
use crate::span::ExpectedId;
use crate::span::ExpectedSpan;
use crate::span::NewSpan;

/// An expectation queued by a mock subscriber or layer.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Expect {
  /// Expect an event.
  Event(ExpectedEvent),
  /// Expect one span to follow from another.
  FollowsFrom {
    /// The span expected to follow another span.
    consequence: ExpectedSpan,
    /// The span expected to be followed from.
    cause:       ExpectedSpan,
  },
  /// Expect a span enter.
  Enter(ExpectedSpan),
  /// Expect a span exit.
  Exit(ExpectedSpan),
  /// Expect a span clone.
  CloneSpan(ExpectedSpan),
  /// Expect a span close.
  CloseSpan(ExpectedSpan),
  /// Expect fields to be recorded on a span.
  Visit(ExpectedSpan, ExpectedFields),
  /// Expect a new span.
  NewSpan(NewSpan),
  /// Expect dispatch registration.
  OnRegisterDispatch,
  /// Expect no further observations.
  Nothing,
}

/// Create a new [`ExpectedEvent`].
///
/// For details on how to add additional expectations to the expected
/// event, see the [`event`] module and the [`ExpectedEvent`] struct.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let (subscriber, handle) = subscriber::mock().event(expect::event()).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   tracing::info!(field.name = "field_value");
/// });
///
/// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
/// # Ok(())
/// # }
/// ```
///
/// If we expect an event and instead record something else, the test
/// will fail:
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let (subscriber, handle) = subscriber::mock().event(expect::event()).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   let span = tracing::info_span!("span");
///   let _guard = span.enter();
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
pub fn event() -> ExpectedEvent {
  ExpectedEvent::default()
}

/// Construct a new [`ExpectedSpan`].
///
/// For details on how to add additional expectations to the expected
/// span, see the [`span`] module and the [`ExpectedSpan`] and
/// [`NewSpan`] structs.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let (subscriber, handle) = subscriber::mock()
///   .new_span(expect::span())
///   .enter(expect::span())
///   .run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   let span = tracing::info_span!("span");
///   let _guard = span.enter();
/// });
///
/// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
/// # Ok(())
/// # }
/// ```
///
/// If we expect to enter a span and instead record something else, the test
/// will fail:
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let (subscriber, handle) = subscriber::mock().enter(expect::span()).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   tracing::info!(field.name = "field_value");
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
pub fn span() -> ExpectedSpan {
  ExpectedSpan::default()
}

/// Construct a new [`ExpectedField`].
///
/// For details on how to set the value of the expected field and
/// how to expect multiple fields, see the [`field`] module and the
/// [`ExpectedField`] and [`ExpectedFields`] structs.
/// span, see the [`span`] module and the [`ExpectedSpan`] and
/// [`NewSpan`] structs.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let event = expect::event().with_fields(expect::field("field.name").with_value(&"field_value"));
///
/// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   tracing::info!(field.name = "field_value");
/// });
///
/// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
/// # Ok(())
/// # }
/// ```
///
/// A different field value will cause the test to fail:
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let event = expect::event().with_fields(expect::field("field.name").with_value(&"field_value"));
///
/// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
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
#[must_use]
pub fn field<K>(name: K) -> ExpectedField
where
  String: From<K>,
{
  ExpectedField {
    name:  name.into(),
    value: ExpectedValue::Any,
  }
}

/// Construct a new message [`ExpectedField`].
///
/// For details on how to set the value of the message field and
/// how to expect multiple fields, see the [`field`] module and the
/// [`ExpectedField`] and [`ExpectedFields`] structs.
///
/// This is equivalent to
/// `expect::field("message").with_value(message)`.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let event = expect::event().with_fields(expect::msg("message"));
///
/// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   tracing::info!("message");
/// });
///
/// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
/// # Ok(())
/// # }
/// ```
///
/// A different message value will cause the test to fail:
///
/// ```
/// # fn main() -> Result<(), strict_test_support::TestFailure> {
/// use tracing_mock::expect;
/// use tracing_mock::subscriber;
///
/// let event = expect::event().with_fields(expect::msg("message"));
///
/// let (subscriber, handle) = subscriber::mock().event(event).run_with_handle();
///
/// tracing::subscriber::with_default(subscriber, || {
///   tracing::info!("different message");
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
pub fn msg(message: impl fmt::Display) -> ExpectedField {
  ExpectedField {
    name:  "message".to_owned(),
    value: ExpectedValue::Debug(message.to_string()),
  }
}

/// Returns a new, unset `ExpectedId`.
///
/// The `ExpectedId` needs to be attached to a [`NewSpan`] or an
/// [`ExpectedSpan`] passed to [`MockSubscriber::new_span`] to
/// ensure that it gets set. When the a clone of the same
/// `ExpectedSpan` is attached to an [`ExpectedSpan`] and passed to
/// any other method on [`MockSubscriber`] that accepts it, it will
/// ensure that it is exactly the same span used across those
/// distinct expectations.
///
/// For more details on how to use this struct, see the documentation
/// on [`ExpectedSpan::with_id`].
///
/// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
/// [`MockSubscriber::new_span`]: fn@crate::subscriber::MockSubscriber::new_span
#[must_use]
pub fn id() -> ExpectedId {
  ExpectedId::new_unset()
}

/// Convenience function that returns [`ExpectedAncestry::IsContextualRoot`].
#[must_use]
pub const fn is_contextual_root() -> ExpectedAncestry {
  ExpectedAncestry::IsContextualRoot
}

/// Convenience function that returns [`ExpectedAncestry::HasContextualParent`] with
/// provided name.
#[must_use]
pub fn has_contextual_parent<S: Into<ExpectedSpan>>(span: S) -> ExpectedAncestry {
  ExpectedAncestry::HasContextualParent(span.into())
}

/// Convenience function that returns [`ExpectedAncestry::IsExplicitRoot`].
#[must_use]
pub const fn is_explicit_root() -> ExpectedAncestry {
  ExpectedAncestry::IsExplicitRoot
}

/// Convenience function that returns [`ExpectedAncestry::HasExplicitParent`] with
/// provided name.
#[must_use]
pub fn has_explicit_parent<S: Into<ExpectedSpan>>(span: S) -> ExpectedAncestry {
  ExpectedAncestry::HasExplicitParent(span.into())
}

impl Expect {
  /// Returns the expected span for a pending clone expectation.
  #[allow(
    clippy::single_call_fn,
    reason = "variant accessor keeps clone-span matching readable at the subscriber hook boundary"
  )]
  pub(crate) const fn clone_span(&self) -> Option<&ExpectedSpan> {
    match *self {
      Self::CloneSpan(ref span) => Some(span),
      Self::Event(_)
      | Self::FollowsFrom {
        ..
      }
      | Self::Enter(_)
      | Self::Exit(_)
      | Self::CloseSpan(_)
      | Self::Visit(..)
      | Self::NewSpan(_)
      | Self::OnRegisterDispatch
      | Self::Nothing => None,
    }
  }

  /// Returns the expected span for a pending close expectation.
  pub(crate) const fn close_span(&self) -> Option<&ExpectedSpan> {
    match *self {
      Self::CloseSpan(ref span) => Some(span),
      Self::Event(_)
      | Self::FollowsFrom {
        ..
      }
      | Self::Enter(_)
      | Self::Exit(_)
      | Self::CloneSpan(_)
      | Self::Visit(..)
      | Self::NewSpan(_)
      | Self::OnRegisterDispatch
      | Self::Nothing => None,
    }
  }

  /// Reports that a different observation occurred while this expectation was pending.
  pub(crate) fn bad(&self, subscriber_name: impl AsRef<str>, what: fmt::Arguments<'_>) -> ExpectationResult {
    let name = subscriber_name.as_ref();
    match *self {
      Self::Event(ref event) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected event {event}\n[{name}] but instead {what}",
      ))),
      Self::FollowsFrom {
        ref consequence,
        ref cause,
      } => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected consequence {consequence} to follow cause {cause} but instead {what}",
      ))),
      Self::Enter(ref span) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected to enter {span}\n[{name}] but instead {what}",
      ))),
      Self::Exit(ref span) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected to exit {span}\n[{name}] but instead {what}",
      ))),
      Self::CloneSpan(ref span) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected to clone {span}\n[{name}] but instead {what}",
      ))),
      Self::CloseSpan(ref span) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected to close {span}\n[{name}] but instead {what}",
      ))),
      Self::Visit(ref span, ref fields) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected {span} to record {fields}\n[{name}] but instead {what}",
      ))),
      Self::NewSpan(ref span) => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected {span}\n[{name}] but instead {what}"
      ))),
      Self::OnRegisterDispatch => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected on_register_dispatch to be called\n[{name}] but instead {what}"
      ))),
      Self::Nothing => Err(ExpectationError::from_args(format_args!(
        "\n[{name}] expected nothing else to happen\n[{name}] but {what} instead",
      ))),
    }
  }
}

impl fmt::Display for Expect {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::Event(ref event) => write!(f, "event {event}"),
      Self::FollowsFrom {
        ref consequence,
        ref cause,
      } => {
        write!(f, "consequence {consequence} to follow cause {cause}")
      }
      Self::Enter(ref span) => write!(f, "enter {span}"),
      Self::Exit(ref span) => write!(f, "exit {span}"),
      Self::CloneSpan(ref span) => write!(f, "clone {span}"),
      Self::CloseSpan(ref span) => write!(f, "close {span}"),
      Self::Visit(ref span, ref fields) => write!(f, "{span} to record {fields}"),
      Self::NewSpan(ref span) => write!(f, "{span}"),
      Self::OnRegisterDispatch => f.write_str("on_register_dispatch"),
      Self::Nothing => f.write_str("nothing else"),
    }
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_some;

  use super::Expect;
  use crate::expect;

  #[test]
  fn clone_and_close_span_accessors_only_accept_matching_expectations() -> Result<(), TestFailure> {
    let copied_reference = Expect::CloneSpan(expect::span().named("cloned_span"));
    let lifecycle_end = Expect::CloseSpan(expect::span().named("closed_span"));
    let event = Expect::Event(expect::event());

    ensure(
      copied_reference.clone_span().is_some(),
      "clone-span accessor exposes clone expectations",
    )?;
    ensure(
      copied_reference.close_span().is_none(),
      "clone-span accessor rejects close expectations",
    )?;
    ensure(
      lifecycle_end.close_span().is_some(),
      "close-span accessor exposes close expectations",
    )?;
    ensure(
      lifecycle_end.clone_span().is_none(),
      "close-span accessor rejects clone expectations",
    )?;
    ensure(event.clone_span().is_none(), "clone-span accessor rejects event expectations")?;
    ensure(event.close_span().is_none(), "close-span accessor rejects event expectations")
  }

  #[test]
  fn display_and_mismatch_messages_describe_every_expectation_variant() -> Result<(), TestFailure> {
    ensure_expectation_text(
      &Expect::Event(expect::event().with_fields(expect::field("message").with_value(&"event message"))),
      &["event"],
      &["expected event", "but instead observed replacement"],
    )?;
    ensure_expectation_text(
      &Expect::FollowsFrom {
        consequence: expect::span().named("consequence_span"),
        cause:       expect::span().named("cause_span"),
      },
      &["consequence", "consequence_span", "follow cause", "cause_span"],
      &["expected consequence", "consequence_span", "cause_span", "observed replacement"],
    )?;
    ensure_expectation_text(&Expect::Enter(expect::span().named("entered_span")), &["enter", "entered_span"], &[
      "expected to enter", "entered_span", "observed replacement",
    ])?;
    ensure_expectation_text(&Expect::Exit(expect::span().named("exited_span")), &["exit", "exited_span"], &[
      "expected to exit", "exited_span", "observed replacement",
    ])?;
    ensure_expectation_text(
      &Expect::CloneSpan(expect::span().named("cloned_span")),
      &["clone", "cloned_span"],
      &["expected to clone", "cloned_span", "observed replacement"],
    )?;
    ensure_expectation_text(
      &Expect::CloseSpan(expect::span().named("closed_span")),
      &["close", "closed_span"],
      &["expected to close", "closed_span", "observed replacement"],
    )?;
    ensure_expectation_text(
      &Expect::Visit(
        expect::span().named("recorded_span"),
        expect::field("answer").with_value(&42_i64).into(),
      ),
      &["recorded_span", "record", "answer"],
      &["expected", "recorded_span", "record", "answer", "observed replacement"],
    )?;
    ensure_expectation_text(
      &Expect::NewSpan(
        expect::span()
          .named("new_span")
          .with_fields(expect::field("mode").with_value(&"fast")),
      ),
      &["new_span", "mode"],
      &["expected", "new_span", "observed replacement"],
    )?;
    ensure_expectation_text(&Expect::OnRegisterDispatch, &["on_register_dispatch"], &[
      "expected on_register_dispatch",
      "observed replacement",
    ])?;
    ensure_expectation_text(&Expect::Nothing, &["nothing else"], &[
      "expected nothing else", "observed replacement",
    ])
  }

  fn ensure_expectation_text(expectation: &Expect, display_fragments: &[&str], error_fragments: &[&str]) -> Result<(), TestFailure> {
    let display = format!("{expectation}");
    for fragment in display_fragments {
      ensure_contains(&display, fragment, "expectation display includes configured fragment")?;
    }

    let expectation_error = ensure_some(
      expectation.bad("expect-tests", format_args!("observed replacement")).err(),
      "mismatch helper returns an error",
    )?;
    let rendered_error = expectation_error.to_string();
    for fragment in error_fragments {
      ensure_contains(&rendered_error, fragment, "mismatch error includes configured fragment")?;
    }

    Ok(())
  }
}
