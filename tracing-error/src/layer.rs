//! Subscriber layer support for making span trace capture inspectable.

use std::any::Any;
use std::any::TypeId;
use std::any::type_name;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

use tracing::Dispatch;
use tracing::Subscriber;
use tracing::span;
use tracing::subscriber::SubscriberResult;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::fmt::format::FormatFields;
use tracing_subscriber::layer;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::registry::LookupSpan;

use crate::SpanTraceVisitor;
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
  fn visit_context(dispatch: &Dispatch, id: span::Id, visitor: &mut SpanTraceVisitor<'_>) {
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

#[cfg(test)]
mod tests {
  use std::any::TypeId;
  use std::fmt;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use tracing::subscriber::with_default;
  use tracing_subscriber::Registry;
  use tracing_subscriber::field::RecordFields;
  use tracing_subscriber::fmt::FormatFields;
  use tracing_subscriber::fmt::format::Writer;
  use tracing_subscriber::prelude::*;

  use super::DefaultFields;
  use super::ErrorLayer;
  use super::Layer;
  use crate::SpanTrace;
  use crate::SpanTraceStatus;
  use crate::WithContext;

  #[test]
  fn downcast_ref_by_id_exposes_layer_and_context_only() -> Result<(), TestFailure> {
    let layer = ErrorLayer::<Registry>::default();

    let layer_ref = <ErrorLayer<Registry> as Layer<Registry>>::downcast_ref_by_id(&layer, TypeId::of::<ErrorLayer<Registry>>());
    let context_ref = <ErrorLayer<Registry> as Layer<Registry>>::downcast_ref_by_id(&layer, TypeId::of::<WithContext>());
    let unrelated_ref = <ErrorLayer<Registry> as Layer<Registry>>::downcast_ref_by_id(&layer, TypeId::of::<String>());

    ensure(layer_ref.is_some(), "error layer downcasts to its concrete type")?;
    ensure(context_ref.is_some(), "error layer downcasts to its context callback")?;
    ensure(unrelated_ref.is_none(), "error layer rejects unrelated downcast types")
  }

  #[test]
  fn debug_output_names_formatter_context_and_subscriber_type() -> Result<(), TestFailure> {
    let layer = ErrorLayer::<Registry>::new(DefaultFields::default());
    let rendered = format!("{layer:?}");

    ensure(rendered.contains("ErrorLayer"), "debug output names the layer type")?;
    ensure(rendered.contains("DefaultFields"), "debug output names the field formatter")?;
    ensure(rendered.contains("WithContext"), "debug output includes the context callback")?;
    ensure(rendered.contains("Registry"), "debug output includes the subscriber type")
  }

  #[derive(Debug)]
  struct FailingFields;

  impl<'writer> FormatFields<'writer> for FailingFields {
    fn format_fields<R: RecordFields>(&self, _writer: Writer<'writer>, _fields: R) -> fmt::Result {
      Err(fmt::Error)
    }
  }

  #[test]
  fn formatting_failure_keeps_span_trace_visitable_with_empty_fields() -> Result<(), TestFailure> {
    let subscriber = Registry::default().with(ErrorLayer::<Registry, FailingFields>::new(FailingFields));
    let trace = with_default(subscriber, || {
      let span = tracing::info_span!("failing fields", answer = 42);
      let _guard = span.enter();
      SpanTrace::capture()
    });
    let mut visited_name = String::new();
    let mut visited_fields = String::from("not visited");

    trace.with_spans(|metadata, fields| {
      visited_name = metadata.name().to_owned();
      visited_fields = fields.to_owned();
      true
    });

    ensure(
      trace.status() == SpanTraceStatus::CAPTURED,
      "formatter failures still capture the span",
    )?;
    ensure_eq(
      &visited_name,
      &"failing fields".to_owned(),
      "span trace still visits the captured span",
    )?;
    ensure(visited_fields.is_empty(), "formatter failures leave captured fields empty")
  }
}
