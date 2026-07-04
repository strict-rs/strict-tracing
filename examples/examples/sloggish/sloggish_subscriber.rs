//! A simple example demonstrating how one might implement a custom
//! subscriber.
//!
//! This subscriber implements a tree-structured logger similar to
//! the "compact" formatter in [`slog-term`]. The demo mimics the
//! example output in the screenshot in the [`slog` README].
//!
//! Note that this logger isn't ready for actual production use.
//! Several corners were cut to make the example simple.
//!
//! [`slog-term`]: https://docs.rs/slog-term/2.4.0/slog_term/
//! [`slog` README]: https://github.com/slog-rs/slog#terminal-output-example
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::io::{
  self,
};
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::SystemTime;

use nu_ansi_term::Color;
use nu_ansi_term::Style;
use parking_lot::Mutex;
use tracing::Event;
use tracing::Id;
use tracing::Level;
use tracing::Metadata;
use tracing::Subscriber;
use tracing::field::Field;
use tracing::field::Visit;
use tracing::span::Attributes;
use tracing::span::Record;
use tracing::subscriber::SetGlobalDefaultError;
use tracing::subscriber::SubscriberResult;
use tracing::subscriber::set_global_default;

/// Field name used by `tracing` macros for the rendered log message.
const MESSAGE_FIELD_NAME: &str = "message";

thread_local! {
    /// Stack of span IDs entered by the current thread.
    static CURRENT_SPANS: RefCell<Vec<Id>> = const { RefCell::new(Vec::new()) };
}

/// Tracks the currently executing span on a per-thread basis.
#[derive(Clone)]
struct CurrentSpanPerThread {
  /// Thread-local span stack backing the current-span lookup.
  current: &'static thread::LocalKey<RefCell<Vec<Id>>>,
}

impl CurrentSpanPerThread {
  /// Returns the [`Id`](::Id) of the span in which the current thread is
  /// executing, or `None` if it is not inside of a span.
  fn id(&self) -> Option<Id> {
    self.current.with(|current| {
      let Ok(active_spans) = current.try_borrow() else {
        return None;
      };
      active_spans.last().copied()
    })
  }

  /// Pushes a span ID onto the current thread's active stack.
  fn enter(&self, span_id: Id) -> bool {
    self.current.with(|current| {
      let Ok(mut active_spans) = current.try_borrow_mut() else {
        return false;
      };
      active_spans.push(span_id);
      true
    })
  }

  /// Pops the most recently entered span ID from the current thread.
  fn exit(&self) -> bool {
    self.current.with(|current| {
      let Ok(mut active_spans) = current.try_borrow_mut() else {
        return false;
      };
      active_spans.pop().is_some()
    })
  }
}

/// Minimal tree-shaped subscriber used by the `sloggish` example.
struct SloggishSubscriber {
  /// Per-thread stack used to assign parent IDs to new spans.
  current:         CurrentSpanPerThread,
  /// Number of spaces emitted for each visual nesting level.
  indent_amount:   usize,
  /// Standard error handle used for example output.
  stderr:          io::Stderr,
  /// Visual stack of span IDs already printed as the active branch.
  visual_stack:    Mutex<Vec<Id>>,
  /// Span field storage keyed by subscriber-assigned IDs.
  spans_by_id:     Mutex<HashMap<Id, Span>>,
  /// Next candidate span ID assigned by this subscriber.
  next_span_id:    AtomicU64,
  /// Deterministic timestamp emitted for each example event.
  event_timestamp: SystemTime,
}

/// Installs a `sloggish` subscriber as the process-global default.
#[allow(
  clippy::single_call_fn,
  reason = "keeps custom subscriber installation separate from the event workload"
)]
pub(super) fn set_as_global_default(indent_amount: usize) -> Result<(), SetGlobalDefaultError> {
  let subscriber = SloggishSubscriber {
    current: CurrentSpanPerThread {
      current: &CURRENT_SPANS
    },
    indent_amount,
    stderr: io::stderr(),
    visual_stack: Mutex::new(Vec::new()),
    spans_by_id: Mutex::new(HashMap::new()),
    next_span_id: AtomicU64::new(1),
    event_timestamp: SystemTime::UNIX_EPOCH,
  };
  set_global_default(subscriber)
}

/// Stored data for a span that may later be printed on entry.
struct Span {
  /// Parent span active when this span was created.
  parent: Option<Id>,
  /// Fields recorded for the span.
  fields: Vec<SpanField>,
}

/// Stored field key and rendered value for a span.
#[derive(Clone)]
struct SpanField {
  /// Static field name from the span metadata.
  name:  &'static str,
  /// Rendered value captured for later output.
  value: String,
}

/// Span entry data prepared after subscriber locks are released.
struct SpanPrintPlan {
  /// Visual indentation level for the entry.
  indent_level: usize,
  /// Span fields to print for the entry.
  fields:       Vec<SpanField>,
}

/// Visitor that writes event fields into the locked output stream.
struct EventFields<'a> {
  /// Locked standard error handle receiving event fields.
  stderr:     io::StderrLock<'a>,
  /// Whether at least one event field has already been emitted.
  has_fields: bool,
  /// First I/O failure observed while visiting fields.
  result:     io::Result<()>,
}

/// Display wrapper that renders a level using terminal colors.
struct ColorLevel<'a>(
  /// Level rendered by the wrapper.
  &'a Level,
);

/// Display adapter for values available only through `fmt::Debug`.
struct DebugValue<'a>(
  /// Erased value passed to a `Visit::record_debug` callback.
  &'a dyn fmt::Debug,
);

impl fmt::Display for ColorLevel<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self.0 {
      Level::TRACE => Color::Purple.paint("TRACE"),
      Level::DEBUG => Color::Blue.paint("DEBUG"),
      Level::INFO => Color::Green.paint("INFO "),
      Level::WARN => Color::Yellow.paint("WARN "),
      Level::ERROR => Color::Red.paint("ERROR"),
    }
    .fmt(f)
  }
}

impl fmt::Display for DebugValue<'_> {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(self.0, formatter)
  }
}

impl Visit for Span {
  fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
    let rendered_value = format!("{}", DebugValue(value));
    self.fields.push(SpanField {
      name:  field.name(),
      value: rendered_value,
    });
  }
}

impl EventFields<'_> {
  /// Finishes the event line and returns the first write failure, if any.
  fn finish(mut self) -> io::Result<()> {
    if self.result.is_ok() {
      self.result = writeln!(&mut self.stderr);
    }
    self.result
  }

  /// Records a displayable field value, preserving any prior write error.
  fn record_display(&mut self, field: &Field, field_value: impl fmt::Display) {
    if self.result.is_err() {
      return;
    }
    self.result = self.write_display(field, field_value);
  }

  /// Writes one field to the event line.
  fn write_display(&mut self, field: &Field, field_value: impl fmt::Display) -> io::Result<()> {
    let separator = if self.has_fields { "," } else { "" };
    write!(&mut self.stderr, "{separator} ")?;

    let rendered_value = format!("{field_value}");
    if field.name() == MESSAGE_FIELD_NAME {
      let styled_value = Style::new().bold().paint(rendered_value);
      write!(&mut self.stderr, "{styled_value}")?;
    } else {
      let styled_name = Style::new().bold().paint(field.name());
      write!(&mut self.stderr, "{styled_name}: {rendered_value}")?;
    }
    self.has_fields = true;
    Ok(())
  }
}

impl Visit for EventFields<'_> {
  fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
    self.record_display(field, DebugValue(value));
  }
}

impl SloggishSubscriber {
  /// Prints indentation for a visual nesting level.
  #[allow(
    clippy::single_call_fn,
    reason = "keeps span and event indentation calculations shared in subscriber output"
  )]
  fn print_indent(writer: &mut impl Write, indent_level: usize, indent_amount: usize) -> io::Result<()> {
    let Some(space_count) = indent_level.checked_mul(indent_amount) else {
      return Ok(());
    };

    for _ in 0..space_count {
      writer.write_all(b" ")?;
    }
    Ok(())
  }

  /// Writes a span entry prepared outside subscriber locks.
  #[allow(
    clippy::single_call_fn,
    reason = "keeps span-entry rendering separate from visual-stack bookkeeping"
  )]
  fn write_span_entry(writer: &mut impl Write, indent_amount: usize, plan: &SpanPrintPlan) -> io::Result<()> {
    Self::print_indent(writer, plan.indent_level, indent_amount)?;
    let mut fields = plan.fields.iter();
    if let Some(field) = fields.next() {
      let styled_name = Style::new().bold().paint(field.name);
      write!(writer, "{styled_name}: {}", field.value)?;
    }
    for field in fields {
      let styled_name = Style::new().bold().paint(field.name);
      write!(writer, ", {styled_name}: {}", field.value)?;
    }
    writeln!(writer)?;
    Ok(())
  }

  /// Writes the fixed event header before field visitation.
  #[allow(
    clippy::single_call_fn,
    reason = "keeps event-header rendering separate from field visitation"
  )]
  fn write_event_header(
    writer: &mut impl Write,
    indent_amount: usize,
    indent_level: usize,
    timestamp: SystemTime,
    event: &Event<'_>,
  ) -> io::Result<()> {
    Self::print_indent(writer, indent_level, indent_amount)?;
    write!(
      writer,
      "{timestamp} {level} {target}",
      timestamp = humantime::format_rfc3339_seconds(timestamp),
      level = ColorLevel(event.metadata().level()),
      target = event.metadata().target(),
    )?;
    Ok(())
  }

  /// Drops observer write failures because this compact example is best effort.
  fn ignore_observer_error(result: io::Result<()>) {
    match result {
      Ok(()) => {}
      Err(observer_error) => drop(observer_error),
    }
  }
}

impl Subscriber for SloggishSubscriber {
  fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
    Ok(true)
  }

  fn new_span(&self, attrs: &Attributes<'_>) -> SubscriberResult<Id> {
    let assigned_id = loop {
      let candidate_id = self.next_span_id.fetch_add(1, Ordering::SeqCst);
      if let Some(nonzero_id) = Id::try_from_u64(candidate_id) {
        break nonzero_id;
      }
    };

    let mut span_data = Span {
      parent: self.current.id(),
      fields: Vec::new(),
    };
    attrs.record(&mut span_data);
    let mut known_spans = self.spans_by_id.lock();
    let replaced_span = known_spans.insert(assigned_id, span_data);
    drop(replaced_span);
    drop(known_spans);
    Ok(assigned_id)
  }

  fn record(&self, span_id: Id, values: &Record<'_>) -> SubscriberResult {
    let mut known_spans = self.spans_by_id.lock();
    if let Some(span_data) = known_spans.get_mut(&span_id) {
      values.record(span_data);
    }
    drop(known_spans);
    Ok(())
  }

  fn record_follows_from(&self, _span: Id, _follows: Id) -> SubscriberResult {
    // This compact example does not display follows-from relationships.
    Ok(())
  }

  fn enter(&self, span_id: Id) -> SubscriberResult {
    let _span_was_recorded = self.current.enter(span_id);
    let (maybe_parent, fields_to_print) = {
      let known_spans = self.spans_by_id.lock();
      let span_parts = known_spans
        .get(&span_id)
        .map_or_else(|| (None, Vec::new()), |span_data| (span_data.parent, span_data.fields.clone()));
      drop(known_spans);
      span_parts
    };

    let print_plan = {
      let mut visual_stack = self.visual_stack.lock();
      let plan = if visual_stack.contains(&span_id) {
        None
      } else {
        let indent_level = if let Some(parent_position) = visual_stack
          .iter()
          .position(|active_id| maybe_parent.is_some_and(|parent_id| *active_id == parent_id))
        {
          let retained_len = parent_position.saturating_add(1);
          visual_stack.truncate(retained_len);
          retained_len
        } else {
          visual_stack.clear();
          0
        };
        visual_stack.push(span_id);
        Some(SpanPrintPlan {
          indent_level,
          fields: fields_to_print,
        })
      };
      drop(visual_stack);
      plan
    };

    if let Some(plan) = print_plan {
      let mut stderr = self.stderr.lock();
      Self::ignore_observer_error(Self::write_span_entry(&mut stderr, self.indent_amount, &plan));
    }
    Ok(())
  }

  fn event(&self, event: &Event<'_>) -> SubscriberResult {
    let indent_level = {
      let visual_stack = self.visual_stack.lock();
      visual_stack.len()
    };

    let mut stderr = self.stderr.lock();
    let header_result = Self::write_event_header(&mut stderr, self.indent_amount, indent_level, self.event_timestamp, event);
    if header_result.is_err() {
      Self::ignore_observer_error(header_result);
      return Ok(());
    }

    let mut visitor = EventFields {
      stderr,
      has_fields: false,
      result: Ok(()),
    };
    event.record(&mut visitor);
    Self::ignore_observer_error(visitor.finish());
    Ok(())
  }

  #[inline]
  fn exit(&self, _span_id: Id) -> SubscriberResult {
    // The visual stack is intentionally retained so future sibling spans can
    // be placed relative to the last printed branch.
    let _span_was_removed = self.current.exit();
    Ok(())
  }

  fn try_close(&self, _span_id: Id) -> SubscriberResult<bool> {
    // This example keeps span data for the duration of the process.
    Ok(false)
  }
}
