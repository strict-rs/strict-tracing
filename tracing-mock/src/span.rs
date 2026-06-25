//! Define expectations to match and validate spans.
//!
//! The [`ExpectedSpan`] and [`NewSpan`] structs define expectations
//! for spans to be matched by the mock subscriber API in the
//! [`subscriber`] module.
//!
//! Expected spans should be created with [`expect::span`] and a
//! chain of method calls describing the expectations made about the
//! span. Expectations about the lifecycle of the span can be set on the [`MockSubscriber`].
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::{expect, subscriber};
//!
//! let span = expect::span()
//!     .named("interesting_span")
//!     .at_level(tracing::Level::INFO);
//!
//! let (subscriber, handle) = subscriber::mock()
//!     .enter(&span)
//!     .exit(&span)
//!     .run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!    let span = tracing::info_span!("interesting_span");
//!     let _guard = span.enter();
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! Instead of passing an `ExpectedSpan`, the subscriber methods will also accept
//! anything that implements `Into<String>` which is shorthand for
//! `expect::span().named(name)`.
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::subscriber;
//!
//! let (subscriber, handle) = subscriber::mock()
//!     .enter("interesting_span")
//!     .run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!    let span = tracing::info_span!("interesting_span");
//!     let _guard = span.enter();
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//
//! The following example asserts the name, level, parent, and fields of the span:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::{expect, subscriber};
//!
//! let span = expect::span()
//!     .named("interesting_span")
//!     .at_level(tracing::Level::INFO);
//! let new_span = span
//!     .clone()
//!     .with_fields(expect::field("field.name").with_value(&"field_value"))
//!     .with_ancestry(expect::has_explicit_parent("parent_span"));
//!
//! let (subscriber, handle) = subscriber::mock()
//!     .new_span("parent_span")
//!     .new_span(new_span)
//!     .enter(&span)
//!     .exit(&span)
//!     .run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!     let parent = tracing::info_span!("parent_span");
//!
//!     let span = tracing::info_span!(
//!         parent: parent.id(),
//!         "interesting_span",
//!         field.name = "field_value",
//!     );
//!     let _guard = span.enter();
//! });
//!
//! strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
//! # Ok(())
//! # }
//! ```
//!
//! All expectations must be met for the test to pass. For example,
//! the following test will fail due to a mismatch in the spans' names:
//!
//! ```
//! # fn main() -> Result<(), strict_test_support::TestFailure> {
//! use tracing_mock::{expect, subscriber};
//!
//! let span = expect::span()
//!     .named("interesting_span")
//!     .at_level(tracing::Level::INFO);
//!
//! let (subscriber, handle) = subscriber::mock()
//!     .enter(&span)
//!     .exit(&span)
//!     .run_with_handle();
//!
//! tracing::subscriber::with_default(subscriber, || {
//!    let span = tracing::info_span!("another_span");
//!    let _guard = span.enter();
//! });
//!
//! strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
//! # Ok(())
//! # }
//! ```
//!
//! [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
//! [`subscriber`]: mod@crate::subscriber
//! [`expect::span`]: fn@crate::expect::span
use std::{
    error, fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    ancestry::{ActualAncestry, ExpectedAncestry},
    failure::{ExpectationError, ExpectationResult},
    field::ExpectedFields,
    metadata::{ExpectedMetadata, display_level},
};
use tracing_core::{
    Metadata,
    span::{Attributes, Id},
};

/// A mock span.
///
/// This is intended for use with the mock subscriber API in the
/// [`subscriber`] module.
///
/// [`subscriber`]: mod@crate::subscriber
#[derive(Clone, Default, Eq, PartialEq)]
pub struct ExpectedSpan {
    /// Expected span ID, if constrained.
    pub(crate) id: Option<ExpectedId>,
    /// Expected span metadata.
    pub(crate) metadata: ExpectedMetadata,
}

impl<I> From<I> for ExpectedSpan
where
    I: Into<String>,
{
    fn from(name: I) -> Self {
        Self::default().named(name)
    }
}

impl From<&ExpectedId> for ExpectedSpan {
    fn from(id: &ExpectedId) -> Self {
        Self::default().with_id(id.clone())
    }
}

impl From<&Self> for ExpectedSpan {
    fn from(span: &Self) -> Self {
        span.clone()
    }
}

/// A mock new span.
///
/// **Note**: This struct contains expectations that can only be asserted
/// on when expecting a new span via [`MockSubscriber::new_span`]. They
/// cannot be validated on [`MockSubscriber::enter`],
/// [`MockSubscriber::exit`], or any other method on [`MockSubscriber`]
/// that takes an `ExpectedSpan`.
///
/// For more details on how to use this struct, see the documentation
/// on the [`subscriber`] module.
///
/// [`subscriber`]: mod@crate::subscriber
/// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
/// [`MockSubscriber::enter`]: fn@crate::subscriber::MockSubscriber::enter
/// [`MockSubscriber::exit`]: fn@crate::subscriber::MockSubscriber::exit
/// [`MockSubscriber::new_span`]: fn@crate::subscriber::MockSubscriber::new_span
#[derive(Default, Eq, PartialEq)]
pub struct NewSpan {
    /// Base span expectations.
    pub(crate) span: ExpectedSpan,
    /// Expected fields on the new span.
    pub(crate) fields: ExpectedFields,
    /// Expected ancestry for the new span.
    pub(crate) ancestry: Option<ExpectedAncestry>,
}

/// Actual span data observed by a mock collector.
pub(crate) struct ActualSpan {
    /// Observed span ID.
    id: Id,
    /// Observed span metadata, when available.
    metadata: Option<&'static Metadata<'static>>,
}

impl ActualSpan {
    /// Creates observed span data from an ID and optional metadata.
    pub(crate) const fn new(id: Id, metadata: Option<&'static Metadata<'static>>) -> Self {
        Self { id, metadata }
    }

    /// The Id of the actual span.
    pub(crate) const fn id(&self) -> Id {
        self.id
    }

    /// The metadata for the actual span if it is available.
    pub(crate) const fn metadata(&self) -> Option<&'static Metadata<'static>> {
        self.metadata
    }
}

impl From<&Id> for ActualSpan {
    fn from(id: &Id) -> Self {
        Self::new(*id, None)
    }
}

/// A mock span ID.
///
/// This ID makes it possible to link together calls to different
/// [`MockSubscriber`] span methods that take an [`ExpectedSpan`] in
/// addition to those that take a [`NewSpan`].
///
/// Use [`expect::id`] to construct a new, unset `ExpectedId`.
///
/// For more details on how to use this struct, see the documentation
/// on [`ExpectedSpan::with_id`].
///
/// [`expect::id`]: fn@crate::expect::id
/// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
#[derive(Clone, Default)]
pub struct ExpectedId {
    /// Shared storage for the actual span ID once it is observed.
    inner: Arc<AtomicU64>,
}

impl ExpectedSpan {
    /// Sets a name to expect when matching a span.
    ///
    /// If an event is recorded with a name that differs from the one provided to this method, the
    /// expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span().named("span name");
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("span name");
    ///     let _guard = span.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// If only the name of the span needs to be validated, then
    /// instead of using the `named` method, a string can be passed
    /// to the [`MockSubscriber`] functions directly.
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::subscriber;
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter("span name")
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("span name");
    ///     let _guard = span.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// When the span name is different, the expectation will fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span().named("span name");
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("a different span name");
    ///     let _guard = span.enter();
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
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

    /// Sets the `ID` to expect when matching a span.
    ///
    /// The [`ExpectedId`] can be used to differentiate spans that are
    /// otherwise identical. An [`ExpectedId`] needs to be attached to
    /// an `ExpectedSpan` or [`NewSpan`] which is passed to
    /// [`MockSubscriber::new_span`]. The same [`ExpectedId`] can then
    /// be used to match the exact same span when passed to
    /// [`MockSubscriber::enter`], [`MockSubscriber::exit`], and
    /// [`MockSubscriber::close_span`].
    ///
    /// This is especially useful when `tracing-mock` is being used to
    /// test the traces being generated within your own crate, in which
    /// case you may need to distinguish between spans which have
    /// identical metadata but different field values, which can
    /// otherwise only be checked in [`MockSubscriber::new_span`].
    ///
    /// # Examples
    ///
    /// Here we expect that the span that is created first is entered
    /// second:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    /// let id1 = expect::id();
    /// let span1 = expect::span().named("span").with_id(id1.clone());
    /// let id2 = expect::id();
    /// let span2 = expect::span().named("span").with_id(id2.clone());
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(&span1)
    ///     .new_span(&span2)
    ///     .enter(&span2)
    ///     .enter(&span1)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     fn create_span() -> tracing::Span {
    ///         tracing::info_span!("span")
    ///     }
    ///
    ///     let span1 = create_span();
    ///     let span2 = create_span();
    ///
    ///     let _guard2 = span2.enter();
    ///     let _guard1 = span1.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Since `ExpectedId` implements `Into<ExpectedSpan>`, in cases where
    /// only checking on Id is desired, a shorthand version of the previous
    /// example can be used.
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    /// let id1 = expect::id();
    /// let id2 = expect::id();
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(&id1)
    ///     .new_span(&id2)
    ///     .enter(&id2)
    ///     .enter(&id1)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     fn create_span() -> tracing::Span {
    ///         tracing::info_span!("span")
    ///     }
    ///
    ///     let span1 = create_span();
    ///     let span2 = create_span();
    ///
    ///     let _guard2 = span2.enter();
    ///     let _guard1 = span1.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// If the order that the spans are entered changes, the test will
    /// fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    /// let id1 = expect::id();
    /// let span1 = expect::span().named("span").with_id(id1.clone());
    /// let id2 = expect::id();
    /// let span2 = expect::span().named("span").with_id(id2.clone());
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(&span1)
    ///     .new_span(&span2)
    ///     .enter(&span2)
    ///     .enter(&span1)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     fn create_span() -> tracing::Span {
    ///         tracing::info_span!("span")
    ///     }
    ///
    ///     let span1 = create_span();
    ///     let span2 = create_span();
    ///
    ///     let _guard1 = span1.enter();
    ///     let _guard2 = span2.enter();
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`MockSubscriber::new_span`]: fn@crate::subscriber::MockSubscriber::new_span
    /// [`MockSubscriber::enter`]: fn@crate::subscriber::MockSubscriber::enter
    /// [`MockSubscriber::exit`]: fn@crate::subscriber::MockSubscriber::exit
    /// [`MockSubscriber::close_span`]: fn@crate::subscriber::MockSubscriber::close_span
    #[must_use]
    pub fn with_id(self, id: ExpectedId) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    /// Sets the [`Level`](tracing::Level) to expect when matching a span.
    ///
    /// If an span is record with a level that differs from the one provided to this method, the expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO);
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!("span");
    ///     let _guard = span.enter();
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Expecting a span at `INFO` level will fail if the event is
    /// recorded at any other level:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .at_level(tracing::Level::INFO);
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::warn_span!("a serious span");
    ///     let _guard = span.enter();
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
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

    /// Sets the target to expect when matching a span.
    ///
    /// If an event is recorded with a target that doesn't match the
    /// provided target, this expectation will fail.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_target("some_target");
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!(target: "some_target", "span");
    ///     let _guard = span.enter();
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
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_target("some_target");
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .enter(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!(target: "a_different_target", "span");
    ///     let _guard = span.enter();
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

    /// Configures this `ExpectedSpan` to expect the specified
    /// [`ExpectedAncestry`]. A span's ancestry indicates whether it has a
    /// parent or is a root span and whether the parent is explitly or
    /// contextually assigned.
    ///
    /// **Note**: This method returns a [`NewSpan`] and as such, this
    /// expectation can only be validated when expecting a new span via
    /// [`MockSubscriber::new_span`]. It cannot be validated on
    /// [`MockSubscriber::enter`], [`MockSubscriber::exit`], or any other
    /// method on [`MockSubscriber`] that takes an `ExpectedSpan`.
    ///
    /// An _explicit_ parent span is one passed to the `span!` macro in the
    /// `parent:` field. If no `parent:` field is specified, then the span
    /// will have a contextually determined parent or be a contextual root if
    /// there is no parent.
    ///
    /// If the ancestry is different from the provided one, this expectation
    /// will fail.
    ///
    /// # Examples
    ///
    /// An explicit or contextual parent can be matched on an `ExpectedSpan`.
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let parent = expect::span()
    ///     .named("parent_span")
    ///     .with_target("custom-target")
    ///     .at_level(tracing::Level::INFO);
    /// let span = expect::span()
    ///     .with_ancestry(expect::has_explicit_parent(&parent));
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(&parent)
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let parent = tracing::info_span!(target: "custom-target", "parent_span");
    ///     tracing::info_span!(parent: parent.id(), "span");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The functions `expect::has_explicit_parent` and
    /// `expect::has_contextual_parent` take `Into<ExpectedSpan>`, so a string
    /// passed directly will match on a span with that name, or an
    /// [`ExpectedId`] can be passed to match a span with that Id.
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_ancestry(expect::has_explicit_parent("parent_span"));
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(expect::span().named("parent_span"))
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let parent = tracing::info_span!("parent_span");
    ///     tracing::info_span!(parent: parent.id(), "span");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// In the following example, the expected span is an explicit root:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_ancestry(expect::is_explicit_root());
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info_span!(parent: None, "span");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// In the example below, the expectation fails because the
    /// span is *contextually*—as opposed to explicitly—within the span
    /// `parent_span`:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let parent_span = expect::span().named("parent_span");
    /// let span = expect::span()
    ///     .with_ancestry(expect::has_explicit_parent("parent_span"));
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(&parent_span)
    ///     .enter(&parent_span)
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let parent = tracing::info_span!("parent_span");
    ///     let _guard = parent.enter();
    ///     tracing::info_span!("span");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// In the following example, we expect that the matched span is
    /// a contextually-determined root:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_ancestry(expect::is_contextual_root());
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info_span!("span");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// In the example below, the expectation fails because the
    /// span is *contextually*—as opposed to explicitly—within the span
    /// `parent_span`:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let parent_span = expect::span().named("parent_span");
    /// let span = expect::span()
    ///     .with_ancestry(expect::has_explicit_parent("parent_span"));
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(&parent_span)
    ///     .enter(&parent_span)
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let parent = tracing::info_span!("parent_span");
    ///     let _guard = parent.enter();
    ///     tracing::info_span!("span");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
    /// [`MockSubscriber::enter`]: fn@crate::subscriber::MockSubscriber::enter
    /// [`MockSubscriber::exit`]: fn@crate::subscriber::MockSubscriber::exit
    /// [`MockSubscriber::new_span`]: fn@crate::subscriber::MockSubscriber::new_span
    #[must_use]
    pub fn with_ancestry(self, ancestry: ExpectedAncestry) -> NewSpan {
        NewSpan {
            ancestry: Some(ancestry),
            span: self,
            ..Default::default()
        }
    }

    /// Adds fields to expect when matching a span.
    ///
    /// **Note**: This method returns a [`NewSpan`] and as such, this
    /// expectation can only be validated when expecting a new span via
    /// [`MockSubscriber::new_span`]. It cannot be validated on
    /// [`MockSubscriber::enter`], [`MockSubscriber::exit`], or any other
    /// method on [`MockSubscriber`] that takes an `ExpectedSpan`.
    ///
    /// If a span is recorded with fields that do not match the provided
    /// [`ExpectedFields`], this expectation will fail.
    ///
    /// If the provided field is not present on the recorded span or
    /// if the value for that field diffs, then the expectation
    /// will fail.
    ///
    /// More information on the available validations is available in
    /// the [`ExpectedFields`] documentation.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_fields(expect::field("field.name").with_value(&"field_value"));
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info_span!("span", field.name = "field_value");
    /// });
    ///
    /// strict_test_support::ensure_ok(handle.finished(), "mock expectations finished")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// A different field value will cause the expectation to fail:
    ///
    /// ```
    /// # fn main() -> Result<(), strict_test_support::TestFailure> {
    /// use tracing_mock::{expect, subscriber};
    ///
    /// let span = expect::span()
    ///     .with_fields(expect::field("field.name").with_value(&"field_value"));
    ///
    /// let (subscriber, handle) = subscriber::mock()
    ///     .new_span(span)
    ///     .run_with_handle();
    ///
    /// tracing::subscriber::with_default(subscriber, || {
    ///     tracing::info_span!("span", field.name = "different_field_value");
    /// });
    ///
    /// strict_test_support::ensure(handle.finished().is_err(), "mock expectation mismatch returns an error")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`ExpectedFields`]: struct@crate::field::ExpectedFields
    /// [`MockSubscriber`]: struct@crate::subscriber::MockSubscriber
    /// [`MockSubscriber::enter`]: fn@crate::subscriber::MockSubscriber::enter
    /// [`MockSubscriber::exit`]: fn@crate::subscriber::MockSubscriber::exit
    /// [`MockSubscriber::new_span`]: fn@crate::subscriber::MockSubscriber::new_span
    #[must_use]
    pub fn with_fields<I>(self, fields: I) -> NewSpan
    where
        I: Into<ExpectedFields>,
    {
        NewSpan {
            span: self,
            fields: fields.into(),
            ..Default::default()
        }
    }

    /// Returns the expected span ID, if one is configured.
    pub(crate) const fn id(&self) -> Option<&ExpectedId> {
        self.id.as_ref()
    }

    /// Returns the expected span name, if one is configured.
    pub(crate) fn name(&self) -> Option<&str> {
        self.metadata.name.as_ref().map(String::as_ref)
    }

    /// Returns the expected span level, if one is configured.
    pub(crate) const fn level(&self) -> Option<tracing::Level> {
        self.metadata.level
    }

    /// Returns the expected span target, if one is configured.
    pub(crate) fn target(&self) -> Option<&str> {
        self.metadata.target.as_deref()
    }

    /// Checks an observed span against this expectation.
    pub(crate) fn check(
        &self,
        actual: &ActualSpan,
        ctx: impl fmt::Display,
        subscriber_name: &str,
    ) -> ExpectationResult {
        if let Some(ref expected_id) = self.id {
            expected_id.check(actual.id(), format_args!("{ctx} a span"), subscriber_name)?;
        }

        match actual.metadata() {
            Some(actual_metadata) => self.metadata.check(actual_metadata, ctx, subscriber_name)?,
            None => {
                if self.metadata.has_expectations() {
                    return Err(ExpectationError::from_args(format_args!(
                        "[{subscriber_name}] expected {ctx} a span with valid metadata, \
                        but got one with unknown Id={actual_id}",
                        actual_id = actual.id().into_u64()
                    )));
                }
            }
        }

        Ok(())
    }
}

impl fmt::Debug for ExpectedSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("MockSpan");

        if let Some(id) = self.id() {
            let _builder = debug.field("id", &id);
        }

        if let Some(name) = self.name() {
            let _builder = debug.field("name", &name);
        }

        if let Some(level) = self.level() {
            let _builder = debug.field("level", &format_args!("{}", display_level(level)));
        }

        if let Some(target) = self.target() {
            let _builder = debug.field("target", &target);
        }

        debug.finish()
    }
}

impl fmt::Display for ExpectedSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.metadata.name.is_some() {
            write!(f, "a span{}", self.metadata)
        } else {
            write!(f, "any span{}", self.metadata)
        }
    }
}

impl<S> From<S> for NewSpan
where
    S: Into<ExpectedSpan>,
{
    fn from(span: S) -> Self {
        Self {
            span: span.into(),
            ..Default::default()
        }
    }
}

impl NewSpan {
    /// Configures this `NewSpan` to expect the specified [`ExpectedAncestry`].
    /// A span's ancestry indicates whether it has a parent or is a root span
    /// and whether the parent is explitly or contextually assigned.
    ///
    /// For more information and examples, see the documentation on
    /// [`ExpectedSpan::with_ancestry`].
    #[must_use]
    pub fn with_ancestry(self, ancestry: ExpectedAncestry) -> Self {
        Self {
            ancestry: Some(ancestry),
            ..self
        }
    }

    /// Adds fields to expect when matching a span.
    ///
    /// For more information and examples, see the documentation on
    /// [`ExpectedSpan::with_fields`].
    ///
    /// [`ExpectedSpan::with_fields`]: fn@crate::span::ExpectedSpan::with_fields
    #[must_use]
    pub fn with_fields<I>(self, fields: I) -> Self
    where
        I: Into<ExpectedFields>,
    {
        Self {
            fields: fields.into(),
            ..self
        }
    }

    /// Checks observed new-span attributes against this expectation.
    pub(crate) fn check(
        &mut self,
        span: &Attributes<'_>,
        get_ancestry: impl FnOnce() -> ExpectationResult<ActualAncestry>,
        subscriber_name: &str,
    ) -> ExpectationResult {
        let meta = span.metadata();
        let name = meta.name();
        self.span
            .metadata
            .check(meta, "a new span", subscriber_name)?;
        let mut checker = self.fields.checker(name, subscriber_name);
        span.record(&mut checker);
        checker.finish()?;

        if let Some(ref expected_ancestry) = self.ancestry {
            let actual_ancestry = get_ancestry()?;
            expected_ancestry.check(
                &actual_ancestry,
                format_args!("span `{name}`"),
                subscriber_name,
            )?;
        }

        Ok(())
    }
}

impl fmt::Display for NewSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a new span{}", self.span.metadata)?;
        if !self.fields.is_empty() {
            write!(f, " with {}", self.fields)?;
        }
        Ok(())
    }
}

impl fmt::Debug for NewSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("NewSpan");

        if let Some(name) = self.span.name() {
            let _builder = debug.field("name", &name);
        }

        if let Some(level) = self.span.level() {
            let _builder = debug.field("level", &format_args!("{}", display_level(level)));
        }

        if let Some(target) = self.span.target() {
            let _builder = debug.field("target", &target);
        }

        if let Some(ref parent) = self.ancestry {
            let _builder = debug.field("parent", &format_args!("{parent}"));
        }

        if !self.fields.is_empty() {
            let _builder = debug.field("fields", &self.fields);
        }

        debug.finish()
    }
}

impl PartialEq for ExpectedId {
    fn eq(&self, other: &Self) -> bool {
        self.inner.load(Ordering::Relaxed) == other.inner.load(Ordering::Relaxed)
    }
}

impl Eq for ExpectedId {}

impl fmt::Debug for ExpectedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ExpectedId").field(&self.inner).finish()
    }
}

impl ExpectedId {
    /// Sentinel value for an expected span ID that has not been set yet.
    const UNSET: u64 = 0;

    /// Creates a new expected ID in the unset state.
    #[allow(
        clippy::single_call_fn,
        reason = "constructor keeps the unset sentinel and atomic storage private to ExpectedId"
    )]
    pub(crate) fn new_unset() -> Self {
        Self {
            inner: Arc::new(AtomicU64::from(Self::UNSET)),
        }
    }

    /// Sets the actual span ID for this expected ID.
    pub(crate) fn set(&self, span_id: u64) -> Result<(), SetActualSpanIdError> {
        let _previous = self
            .inner
            .compare_exchange(Self::UNSET, span_id, Ordering::Relaxed, Ordering::Relaxed)
            .map_err(|current| SetActualSpanIdError {
                previous_span_id: current,
                new_span_id: span_id,
            })?;
        Ok(())
    }

    /// Checks an actual span ID against this expected ID.
    pub(crate) fn check(
        &self,
        actual: Id,
        ctx: fmt::Arguments<'_>,
        subscriber_name: &str,
    ) -> ExpectationResult {
        let expected_id = self.inner.load(Ordering::Relaxed);
        let actual_id = actual.into_u64();

        if expected_id == Self::UNSET {
            return Err(ExpectationError::from_args(format_args!(
                "\n[{subscriber_name}] expected {ctx} with an expected Id set,\n\
                [{subscriber_name}] but it hasn't been, perhaps this `ExpectedId` \
                wasn't used in a call to `new_span()`?"
            )));
        }

        if expected_id != actual_id {
            return Err(ExpectationError::from_args(format_args!(
                "\n[{subscriber_name}] expected {ctx} with Id `{expected_id}`,\n\
                [{subscriber_name}] but got one with Id `{actual_id}` instead",
            )));
        }

        Ok(())
    }
}

/// Error returned when an expected span ID was already set.
#[derive(Debug)]
pub(crate) struct SetActualSpanIdError {
    /// Previously recorded span ID.
    previous_span_id: u64,
    /// New span ID that could not be recorded.
    new_span_id: u64,
}

impl fmt::Display for SetActualSpanIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Could not set `ExpecedId` to {new}, \
            it had already been set to {previous}",
            new = self.new_span_id,
            previous = self.previous_span_id
        )
    }
}

impl error::Error for SetActualSpanIdError {}
