//! Tests reload, registry, context, and layered subscriber contracts.
#![cfg(all(feature = "fmt", feature = "registry"))]

#[cfg(test)]
mod tests {
  use std::io;
  use std::io::Write;
  use std::sync::Arc;

  use parking_lot::Mutex;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_lacks;
  use strict_test_support::ensure_ok;
  use tracing::Dispatch;
  use tracing::Level;
  use tracing::Subscriber;
  use tracing::dispatcher::with_default;
  use tracing_core::Event;
  use tracing_core::span;
  use tracing_core::subscriber::SubscriberResult;
  use tracing_subscriber::Registry;
  use tracing_subscriber::filter::LevelFilter;
  use tracing_subscriber::fmt;
  use tracing_subscriber::fmt::MakeWriter;
  use tracing_subscriber::layer::Context;
  use tracing_subscriber::layer::Layer;
  use tracing_subscriber::prelude::*;
  use tracing_subscriber::registry::LookupSpan;
  use tracing_subscriber::reload;

  /// Reload layer type used by registry-based tests.
  type RegistryReloadLayer = reload::Layer<LevelFilter, Registry>;

  /// Reload handle type used by registry-based tests.
  type RegistryReloadHandle = reload::Handle<LevelFilter, Registry>;

  /// In-memory writer used to assert formatted output.
  #[derive(Clone, Debug, Default)]
  struct MemoryWriter {
    /// Shared output bytes.
    bytes: Arc<Mutex<Vec<u8>>>,
  }

  impl MemoryWriter {
    /// Returns the collected output as UTF-8 text.
    fn output(&self) -> String {
      String::from_utf8_lossy(&self.bytes.lock()).into_owned()
    }
  }

  impl Write for MemoryWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      self.bytes.lock().extend_from_slice(buf);
      Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  impl<'writer> MakeWriter<'writer> for MemoryWriter {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
      Self::clone(self)
    }
  }

  /// Span extension inserted by the observing layer.
  #[derive(Clone, Debug, Eq, PartialEq)]
  struct SpanNote {
    /// Stored span label.
    label: String,
  }

  /// Span extension replaced during event observation.
  #[derive(Clone, Debug, Eq, PartialEq)]
  struct ReplaceNote {
    /// Stored replacement label.
    label: &'static str,
  }

  /// Span extension removed during event observation.
  #[derive(Clone, Debug, Eq, PartialEq)]
  struct RemoveNote {
    /// Stored removal label.
    label: &'static str,
  }

  /// Span extension mutated during event observation.
  #[derive(Clone, Debug, Eq, PartialEq)]
  struct MutableNote {
    /// Stored mutable label.
    label: String,
  }

  /// Registry context observation captured for one event.
  #[derive(Clone, Debug, Default, Eq, PartialEq)]
  struct ContextObservation {
    /// Name of the current span.
    current_name: Option<String>,
    /// Name of the event parent span.
    event_span_name: Option<String>,
    /// Leaf-to-root scope names.
    leaf_to_root: Vec<String>,
    /// Root-to-leaf scope names.
    root_to_leaf: Vec<String>,
    /// Extension note read from the current event span.
    span_note: Option<String>,
    /// Whether inserting an already-present extension returned the attempted value.
    duplicate_insert_returned_attempt: bool,
    /// Label returned by replacing an existing extension.
    replaced_note: Option<&'static str>,
    /// Label returned by removing an existing extension.
    removed_note: Option<&'static str>,
    /// Label read after mutating an existing extension.
    mutated_note: Option<String>,
  }

  /// Layer that records registry context and span extension behavior.
  #[derive(Clone, Debug, Default)]
  struct ObservingLayer {
    /// Captured context observations.
    observations: Arc<Mutex<Vec<ContextObservation>>>,
  }

  impl ObservingLayer {
    /// Returns captured observations.
    fn observations(&self) -> Vec<ContextObservation> {
      self.observations.lock().clone()
    }
  }

  impl<S> Layer<S> for ObservingLayer
  where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
  {
    fn on_new_span(&self, _attrs: &span::Attributes<'_>, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
      if let Some(span_ref) = ctx.span(id) {
        let mut extensions = span_ref.extensions_mut();
        let _duplicate_note = extensions.insert(SpanNote {
          label: span_ref.name().to_owned(),
        });
        let _previous_replace = extensions.insert(ReplaceNote {
          label: "before"
        });
        let _previous_remove = extensions.insert(RemoveNote {
          label: "removed"
        });
        let _previous_mutable = extensions.insert(MutableNote {
          label: "before".to_owned(),
        });
      }
      Ok(())
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
      let current_name = ctx.lookup_current().map(|span_ref| span_ref.name().to_owned());
      let event_span_name = ctx.event_span(event).map(|span_ref| span_ref.name().to_owned());
      let leaf_to_root = ctx
        .event_scope(event)
        .map(|scope| scope.map(|span_ref| span_ref.name().to_owned()).collect::<Vec<_>>())
        .unwrap_or_default();
      let root_to_leaf = ctx
        .event_scope(event)
        .map(|scope| {
          scope
            .root_to_leaf()
            .map(|span_ref| span_ref.name().to_owned())
            .collect::<Vec<_>>()
        })
        .unwrap_or_default();

      let mut observation = ContextObservation {
        current_name,
        event_span_name,
        leaf_to_root,
        root_to_leaf,
        ..ContextObservation::default()
      };

      let Some(span_ref) = ctx.event_span(event) else {
        self.observations.lock().push(observation);
        return Ok(());
      };

      let mut extensions = span_ref.extensions_mut();
      observation.span_note = extensions.get_mut::<SpanNote>().map(|note| note.label.clone());
      observation.duplicate_insert_returned_attempt = extensions
        .insert(SpanNote {
          label: "attempt".to_owned(),
        })
        .is_some();
      observation.replaced_note = extensions
        .replace(ReplaceNote {
          label: "after"
        })
        .map(|note| note.label);
      observation.removed_note = extensions.remove::<RemoveNote>().map(|note| note.label);
      if let Some(note) = extensions.get_mut::<MutableNote>() {
        note.label = "after".to_owned();
        observation.mutated_note = Some(note.label.clone());
      }
      drop(extensions);

      self.observations.lock().push(observation);
      Ok(())
    }
  }

  /// Layer that records span close notifications.
  #[derive(Clone, Debug, Default)]
  struct CloseLayer {
    /// Names of spans observed during close callbacks.
    closed_names: Arc<Mutex<Vec<String>>>,
  }

  impl CloseLayer {
    /// Returns captured close names.
    fn closed_names(&self) -> Vec<String> {
      self.closed_names.lock().clone()
    }
  }

  impl<S> Layer<S> for CloseLayer
  where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
  {
    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
      if let Some(span_ref) = ctx.span(id) {
        self.closed_names.lock().push(span_ref.name().to_owned());
      }
      Ok(())
    }
  }

  /// Layer that records event hook order.
  #[derive(Clone, Debug)]
  struct OrderLayer {
    /// Layer label.
    label: &'static str,
    /// Shared hook order sink.
    order: Arc<Mutex<Vec<&'static str>>>,
  }

  impl<S> Layer<S> for OrderLayer
  where
    S: Subscriber,
  {
    fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> SubscriberResult {
      self.order.lock().push(self.label);
      Ok(())
    }
  }

  #[test]
  fn reload_filter_replaces_output_policy() -> Result<(), TestFailure> {
    let writer = MemoryWriter::default();
    let (filter, handle) = reload::Layer::new(LevelFilter::INFO);
    let subscriber = tracing_subscriber::registry().with(filter).with(
      fmt::layer()
        .with_writer(writer.clone())
        .with_ansi(false)
        .without_time()
        .with_level(false)
        .with_target(false),
    );
    let dispatch = Dispatch::new(subscriber);

    with_default(&dispatch, || -> Result<(), TestFailure> {
      tracing::debug!("debug before reload");
      tracing::info!("info before reload");
      ensure_ok(handle.reload(LevelFilter::DEBUG), "reload filter accepts a new level")?;
      tracing::debug!("debug after reload");
      Ok(())
    })?;

    let output = writer.output();
    ensure_contains(&output, "info before reload", "initial info-level filter records info events")?;
    ensure_lacks(&output, "debug before reload", "initial info-level filter rejects debug events")?;
    ensure_contains(&output, "debug after reload", "reloaded debug-level filter records debug events")
  }

  #[test]
  fn reload_handle_reports_closed_after_reload_layer_is_dropped() -> Result<(), TestFailure> {
    let handle = {
      let (_filter, handle): (RegistryReloadLayer, RegistryReloadHandle) = reload::Layer::new(LevelFilter::INFO);
      handle
    };

    ensure(
      handle.reload(LevelFilter::TRACE).is_err(),
      "reload handle reports an error after the reload layer drops",
    )
  }

  #[test]
  fn registry_context_reports_current_scope_and_extension_round_trips() -> Result<(), TestFailure> {
    let layer = ObservingLayer::default();
    let subscriber = tracing_subscriber::registry().with(layer.clone());

    with_default(&Dispatch::new(subscriber), || {
      let root = tracing::info_span!("root_span");
      let _root_entered = root.enter();
      let child = tracing::info_span!("child_span");
      let _child_entered = child.enter();
      tracing::info!("inside child");
    });

    let observations = layer.observations();
    ensure(observations.len() == 1, "one event observation is captured")?;
    let Some(observation) = observations.first() else {
      return ensure(false, "event observation is present");
    };
    ensure(
      observation.current_name.as_deref() == Some("child_span"),
      "context lookup reports the current span",
    )?;
    ensure(
      observation.event_span_name.as_deref() == Some("child_span"),
      "event_span reports the contextual event parent",
    )?;
    ensure(
      observation.leaf_to_root == ["child_span".to_owned(), "root_span".to_owned()],
      "event scope iterates leaf to root",
    )?;
    ensure(
      observation.root_to_leaf == ["root_span".to_owned(), "child_span".to_owned()],
      "root_to_leaf reverses event scope order",
    )?;
    ensure(
      observation.span_note.as_deref() == Some("child_span"),
      "extensions expose values inserted by on_new_span",
    )?;
    ensure(
      observation.duplicate_insert_returned_attempt,
      "duplicate extension insert returns the attempted value",
    )?;
    ensure(
      observation.replaced_note == Some("before"),
      "extension replace returns the previous value",
    )?;
    ensure(
      observation.removed_note == Some("removed"),
      "extension remove returns the stored value",
    )?;
    ensure(
      observation.mutated_note.as_deref() == Some("after"),
      "extension get_mut allows in-place mutation",
    )
  }

  #[test]
  fn registry_closes_spans_after_the_last_handle_is_dropped() -> Result<(), TestFailure> {
    let layer = CloseLayer::default();
    let subscriber = tracing_subscriber::registry().with(layer.clone());

    with_default(&Dispatch::new(subscriber), || -> Result<(), TestFailure> {
      let span = tracing::info_span!("closed_after_last_handle");
      let cloned_span = span.clone();
      drop(span);
      ensure(
        layer.closed_names().is_empty(),
        "span is not closed while a clone still holds its ID",
      )?;
      drop(cloned_span);
      ensure(
        layer.closed_names() == ["closed_after_last_handle".to_owned()],
        "span closes after the last handle drops",
      )
    })
  }

  #[test]
  fn layered_subscriber_calls_inner_layer_before_outer_layer() -> Result<(), TestFailure> {
    let order = Arc::new(Mutex::new(Vec::new()));
    let inner = OrderLayer {
      label: "inner",
      order: Arc::clone(&order),
    };
    let outer = OrderLayer {
      label: "outer",
      order: Arc::clone(&order),
    };
    let subscriber = tracing_subscriber::registry().with(inner).with(outer);

    with_default(&Dispatch::new(subscriber), || {
      tracing::event!(Level::INFO, "ordered event");
    });

    ensure(
      *order.lock() == ["inner", "outer"],
      "layered subscriber invokes inner event hooks before outer hooks",
    )
  }
}
