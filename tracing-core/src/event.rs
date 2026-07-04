//! Events represent single points in time during the execution of a program.
use crate::Metadata;
use crate::Parent;
use crate::dispatcher::get_default;
use crate::field;
use crate::span::Id;

/// `Event`s represent single points in time where something occurred during the
/// execution of a program.
///
/// An `Event` can be compared to a log record in unstructured logging, but with
/// two key differences:
/// - `Event`s exist _within the context of a [span]_. Unlike log lines, they may be located within
///   the trace tree, allowing visibility into the _temporal_ context in which the event occurred,
///   as well as the source code location.
/// - Like spans, `Event`s have structured key-value data known as _[fields]_, which may include
///   textual message. In general, a majority of the data associated with an event should be in the
///   event's fields rather than in the textual message, as the fields are more structured.
///
/// [span]: super::span
/// [fields]: super::field
#[derive(Debug)]
pub struct Event<'a> {
  /// Values recorded with the event.
  fields:   &'a field::ValueSet<'a>,
  /// Metadata describing the callsite that emitted the event.
  metadata: &'static Metadata<'static>,
  /// Parent relationship requested for the event.
  parent:   Parent,
}

impl<'a> Event<'a> {
  /// Constructs a new `Event` with the specified metadata and set of values,
  /// and observes it with the current subscriber.
  pub fn dispatch(metadata: &'static Metadata<'static>, fields: &'a field::ValueSet<'_>) {
    let event = Self::new(metadata, fields);
    get_default(|current| {
      let _ignored = current.event(&event);
    });
  }

  /// Returns a new `Event` in the current span, with the specified metadata
  /// and set of values.
  #[allow(
    clippy::single_call_fn,
    reason = "public Event constructor is used by downstream manual instrumentation"
  )]
  #[inline]
  #[must_use]
  pub const fn new(metadata: &'static Metadata<'static>, fields: &'a field::ValueSet<'a>) -> Self {
    Self {
      fields,
      metadata,
      parent: Parent::Current,
    }
  }

  /// Returns a new `Event` as a child of the specified span, with the
  /// provided metadata and set of values.
  #[allow(
    clippy::single_call_fn,
    reason = "public explicit-parent Event constructor is used by downstream manual instrumentation"
  )]
  #[inline]
  #[must_use]
  pub fn new_child_of(parent: impl Into<Option<Id>>, metadata: &'static Metadata<'static>, fields: &'a field::ValueSet<'a>) -> Self {
    let resolved_parent = parent.into().map_or(Parent::Root, Parent::Explicit);
    Self {
      fields,
      metadata,
      parent: resolved_parent,
    }
  }

  /// Constructs a new `Event` with the specified metadata and set of values,
  /// and observes it with the current subscriber and an explicit parent.
  pub fn child_of(parent: impl Into<Option<Id>>, metadata: &'static Metadata<'static>, fields: &'a field::ValueSet<'_>) {
    let event = Self::new_child_of(parent, metadata, fields);
    get_default(|current| {
      let _ignored = current.event(&event);
    });
  }

  /// Visits all the fields on this `Event` with the specified [visitor].
  ///
  /// [visitor]: super::field::Visit
  #[inline]
  pub fn record(&self, visitor: &mut dyn field::Visit) {
    self.fields.record(visitor);
  }

  /// Returns an iterator over the set of values on this `Event`.
  #[must_use]
  pub const fn fields(&self) -> field::Iter {
    self.fields.field_set().iter()
  }

  /// Returns [metadata] describing this `Event`.
  ///
  /// [metadata]: super::Metadata
  #[must_use]
  pub const fn metadata(&self) -> &'static Metadata<'static> {
    self.metadata
  }

  /// Returns true if the new event should be a root.
  #[must_use]
  pub const fn is_root(&self) -> bool {
    matches!(self.parent, Parent::Root)
  }

  /// Returns true if the new event's parent should be determined based on the
  /// current context.
  ///
  /// If this is true and the current thread is currently inside a span, then
  /// that span should be the new event's parent. Otherwise, if the current
  /// thread is _not_ inside a span, then the new event will be the root of its
  /// own trace tree.
  #[must_use]
  pub const fn is_contextual(&self) -> bool {
    matches!(self.parent, Parent::Current)
  }

  /// Returns the new event's explicitly-specified parent, if there is one.
  ///
  /// Otherwise (if the new event is a root or is a child of the current span),
  /// returns `None`.
  #[must_use]
  pub const fn parent(&self) -> Option<&Id> {
    match self.parent {
      Parent::Explicit(ref parent) => Some(parent),
      Parent::Root | Parent::Current => None,
    }
  }
}
