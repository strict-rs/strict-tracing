//! Define the ancestry of an event or span.
//!
//! See the documentation on the [`ExpectedAncestry`] enum for further details.

use std::fmt;

use tracing_core::Event;
use tracing_core::span::Attributes;
use tracing_core::span::{
  self,
};

use crate::failure::ExpectationError;
use crate::failure::ExpectationResult;
use crate::span::ActualSpan;
use crate::span::ExpectedSpan;

/// The ancestry of an event or span.
///
/// An event or span can have an explicitly assigned parent, or be an explicit root. Otherwise,
/// an event or span may have a contextually assigned parent or in the final case will be a
/// contextual root.
#[derive(Debug, Eq, PartialEq)]
pub enum ExpectedAncestry {
  /// The event or span has an explicitly assigned parent (created with `parent: span_id`) span.
  HasExplicitParent(ExpectedSpan),
  /// The event or span is an explicitly defined root. It was created with `parent: None` and
  /// has no parent.
  IsExplicitRoot,
  /// The event or span has a contextually assigned parent span. It has no explicitly assigned
  /// parent span, nor has it been explicitly defined as a root (it was created without the
  /// `parent:` directive). There was a span in context when this event or span was created.
  HasContextualParent(ExpectedSpan),
  /// The event or span is a contextual root. It has no explicitly assigned parent, nor has it
  /// been explicitly defined as a root (it was created without the `parent:` directive).
  /// Additionally, no span was in context when this event or span was created.
  IsContextualRoot,
}

/// The observed ancestry for an event or span.
pub(crate) enum ActualAncestry {
  /// The observed item has an explicit parent span.
  HasExplicitParent(ActualSpan),
  /// The observed item is an explicit root.
  IsExplicitRoot,
  /// The observed item has a contextual parent span.
  HasContextualParent(ActualSpan),
  /// The observed item is a contextual root.
  IsContextualRoot,
}

impl ExpectedAncestry {
  /// Validates observed ancestry against this expectation.
  #[track_caller]
  pub(crate) fn check(&self, actual_ancestry: &ActualAncestry, ctx: impl fmt::Display, collector_name: &str) -> ExpectationResult {
    match *self {
      Self::IsExplicitRoot if matches!(actual_ancestry, ActualAncestry::IsExplicitRoot) => Ok(()),
      Self::IsContextualRoot if matches!(actual_ancestry, ActualAncestry::IsContextualRoot) => Ok(()),
      Self::HasExplicitParent(ref expected_parent) => {
        if let ActualAncestry::HasExplicitParent(ref actual_parent) = *actual_ancestry {
          return expected_parent.check(actual_parent, format_args!("{ctx} to have an explicit parent span"), collector_name);
        }
        self.fail_mismatch(actual_ancestry, ctx, collector_name)
      }
      Self::HasContextualParent(ref expected_parent) => {
        if let ActualAncestry::HasContextualParent(ref actual_parent) = *actual_ancestry {
          return expected_parent.check(
            actual_parent,
            format_args!("{ctx} to have a contextual parent span"),
            collector_name,
          );
        }
        self.fail_mismatch(actual_ancestry, ctx, collector_name)
      }
      Self::IsExplicitRoot | Self::IsContextualRoot => self.fail_mismatch(actual_ancestry, ctx, collector_name),
    }
  }

  /// Reports an ancestry kind mismatch.
  fn fail_mismatch(&self, actual_ancestry: &ActualAncestry, ctx: impl fmt::Display, collector_name: &str) -> ExpectationResult {
    let expected_description = match *self {
      Self::IsExplicitRoot => "be an explicit root",
      Self::HasExplicitParent(_) => "have an explicit parent span",
      Self::IsContextualRoot => "be a contextual root",
      Self::HasContextualParent(_) => "have a contextual parent span",
    };

    let actual_description = match *actual_ancestry {
      ActualAncestry::IsExplicitRoot => "is actually an explicit root",
      ActualAncestry::HasExplicitParent(ref _actual_parent) => "actually has an explicit parent span",
      ActualAncestry::IsContextualRoot => "is actually a contextual root",
      ActualAncestry::HasContextualParent(ref _actual_parent) => "actually has a contextual parent span",
    };

    Err(ExpectationError::from_args(format_args!(
      "[{collector_name}] expected {ctx} to {expected_description}, but it {actual_description}"
    )))
  }
}

impl fmt::Display for ExpectedAncestry {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::HasExplicitParent(ref parent) => write!(f, "explicit parent {parent}"),
      Self::IsExplicitRoot => f.write_str("explicit root"),
      Self::HasContextualParent(ref parent) => write!(f, "contextual parent {parent}"),
      Self::IsContextualRoot => f.write_str("contextual root"),
    }
  }
}

/// Common parent/root accessors for traced items with ancestry metadata.
pub(crate) trait HasAncestry {
  /// Returns whether the item should use the current contextual parent.
  fn is_contextual(&self) -> bool;

  /// Returns whether the item was explicitly declared as a root.
  fn is_root(&self) -> bool;

  /// Returns the explicit parent ID when one was supplied.
  fn parent(&self) -> Option<&span::Id>;
}

impl HasAncestry for &Event<'_> {
  fn is_contextual(&self) -> bool {
    (*self).is_contextual()
  }

  fn is_root(&self) -> bool {
    (*self).is_root()
  }

  fn parent(&self) -> Option<&span::Id> {
    (*self).parent()
  }
}

impl HasAncestry for &Attributes<'_> {
  fn is_contextual(&self) -> bool {
    (*self).is_contextual()
  }

  fn is_root(&self) -> bool {
    (*self).is_root()
  }

  fn parent(&self) -> Option<&span::Id> {
    (*self).parent()
  }
}

/// Determines the ancestry of an actual span or event.
///
/// The rules for determining the ancestry are as follows:
///
/// +------------+--------------+-----------------+---------------------+
/// | Contextual | Current Span | Explicit Parent | Ancestry            |
/// +------------+--------------+-----------------+---------------------+
/// | Yes        | Yes          | -               | `HasContextualParent` |
/// | Yes        | No           | -               | `IsContextualRoot`    |
/// | No         | -            | Yes             | `HasExplicitParent`   |
/// | No         | -            | No              | `IsExplicitRoot`      |
/// +------------+--------------+-----------------+---------------------+
pub(crate) fn get_ancestry(
  item: &impl HasAncestry,
  lookup_current: impl FnOnce() -> Option<span::Id>,
  actual_span: impl FnOnce(&span::Id) -> Option<ActualSpan>,
) -> ExpectationResult<ActualAncestry> {
  if item.is_contextual() {
    lookup_current().map_or(Ok(ActualAncestry::IsContextualRoot), |parent_id| {
      let parent_id_value = parent_id.into_u64();
      actual_span(&parent_id).map_or_else(
        || {
          Err(ExpectationError::from_args(format_args!(
            "tracing-mock: contextual parent with ID `{parent_id_value}` cannot be looked up. Was it recorded correctly?"
          )))
        },
        |contextual_parent_span| Ok(ActualAncestry::HasContextualParent(contextual_parent_span)),
      )
    })
  } else if item.is_root() {
    Ok(ActualAncestry::IsExplicitRoot)
  } else {
    let Some(parent_id) = item.parent() else {
      return Err(ExpectationError::from_args(format_args!(
        "tracing-mock: is_contextual=false is_root=false but no explicit parent found. This is a bug!"
      )));
    };
    let parent_id_value = parent_id.into_u64();
    actual_span(parent_id).map_or_else(
      || {
        Err(ExpectationError::from_args(format_args!(
          "tracing-mock: explicit parent with ID `{parent_id_value}` cannot be looked up. Is the provided span ID valid?"
        )))
      },
      |explicit_parent_span| Ok(ActualAncestry::HasExplicitParent(explicit_parent_span)),
    )
  }
}
