//! Subscriber layer support for making span trace capture inspectable.

use std::any::Any;
use std::any::TypeId;
use std::any::type_name;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use tracing::Dispatch;
use tracing::Metadata;
use tracing::Subscriber;
use tracing::span;
use tracing::subscriber::SubscriberResult;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::format::FormatFields;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::layer::{
  self,
};
use tracing_subscriber::registry::LookupSpan;

use crate::WithContext;

/// A subscriber [`Layer`] that enables capturing [`SpanTrace`]s.
///
/// Optionally, this type may be constructed with a [field formatter] to use
/// when formatting the fields of each span in a trace. When no formatter is
/// provided, the [default format] is used instead.
///
/// [`Layer`]: tracing_subscriber::layer::Layer
/// [`SpanTrace`]: super::SpanTrace
/// [field formatter]: tracing_subscriber::fmt::FormatFields
/// [default format]: tracing_subscriber::fmt::format::DefaultFields
pub struct ErrorLayer<S, F = DefaultFields> {
  /// Formats span fields before storing them in span extensions.
  format: F,

  /// Recovers formatted span context through the captured subscriber type.
  get_context: WithContext,

  /// Tracks the subscriber type that this layer is attached to.
  _subscriber: PhantomData<fn(S)>,
}

impl<S, F> Layer<S> for ErrorLayer<S, F>
where
  S: Subscriber + for<'span> LookupSpan<'span>,
  F: for<'writer> FormatFields<'writer> + 'static,
{
  /// Notifies this layer that a new span was constructed with the given
  /// `Attributes` and `Id`.
  fn on_new_span(&self, attrs: &span::Attributes<'_>, id: span::Id, ctx: layer::Context<'_, S>) -> SubscriberResult {
    let Some(span_ref) = ctx.span(id) else {
      return Ok(());
    };

    if span_ref.extensions().get::<FormattedFields<F>>().is_some() {
      return Ok(());
    }

    let mut formatted_fields = FormattedFields::<F>::new(String::new());
    let Ok(()) = self.format.format_fields(formatted_fields.as_writer(), attrs) else {
      return Ok(());
    };

    if span_ref.extensions().get::<FormattedFields<F>>().is_some() {
      return Ok(());
    }

    drop(span_ref.extensions_mut().insert(formatted_fields));
    Ok(())
  }

  fn downcast_ref_by_id(&self, requested: TypeId) -> Option<&dyn Any> {
    if requested == TypeId::of::<Self>() {
      Some(self)
    } else if requested == TypeId::of::<WithContext>() {
      Some(&self.get_context)
    } else {
      None
    }
  }
}

impl<S, F> ErrorLayer<S, F>
where
  F: for<'writer> FormatFields<'writer> + 'static,
  S: Subscriber + for<'span> LookupSpan<'span>,
{
  /// Returns a new `ErrorLayer` with the provided [field formatter].
  ///
  /// [field formatter]: tracing_subscriber::fmt::FormatFields
  #[allow(
    clippy::single_call_fn,
    reason = "public constructor remains the API for installing a custom field formatter"
  )]
  pub fn new(format: F) -> Self {
    Self {
      format,
      get_context: WithContext::new(Self::visit_context),
      _subscriber: PhantomData,
    }
  }

  /// Visits the captured span and its scope with formatter-specific fields.
  #[allow(
    clippy::single_call_fn,
    reason = "named function item is stored as the type-erased SpanTrace context callback"
  )]
  fn visit_context(dispatch: &Dispatch, id: span::Id, visitor: &mut dyn FnMut(&'static Metadata<'static>, &str) -> bool) {
    let Some(subscriber) = dispatch.downcast_ref::<S>() else {
      return;
    };
    let Some(captured_span) = subscriber.span(id) else {
      return;
    };

    for scope_span in captured_span.scope() {
      let formatted_fields = {
        let extensions = scope_span.extensions();
        extensions
          .get::<FormattedFields<F>>()
          .map_or(Cow::Borrowed(""), |stored_fields| Cow::Owned(stored_fields.fields().to_owned()))
      };

      let continue_visiting = visitor(scope_span.metadata(), formatted_fields.as_ref());
      if !continue_visiting {
        break;
      }
    }
  }
}

impl<S> Default for ErrorLayer<S>
where
  S: Subscriber + for<'span> LookupSpan<'span>,
{
  fn default() -> Self {
    Self::new(DefaultFields::default())
  }
}

impl<S, F: fmt::Debug> fmt::Debug for ErrorLayer<S, F> {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter
      .debug_struct("ErrorLayer")
      .field("format", &self.format)
      .field("get_context", &self.get_context)
      .field("_subscriber", &format_args!("{}", type_name::<S>()))
      .finish()
  }
}
