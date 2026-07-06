//! Example binary for tracing workspace checks.

use std::io;
use std::io::Write;
use std::sync::Arc;

use ansi_to_tui::IntoText as _;
use crossterm::event;
use parking_lot::Mutex;
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize as _;
use ratatui::widgets::Block;
use ratatui::widgets::Widget;
use ratatui_textarea::Input;
use ratatui_textarea::Key;
use ratatui_textarea::TextArea;
use tracing::subscriber::with_default;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::ParseError;
use tracing_subscriber::fmt::MakeWriter;

/// A list of preset filters to make it easier to explore the filter syntax.
///
/// The UI allows you to select a preset filter with the up/down arrow keys.
const PRESET_FILTERS: &[&str] = &[
  "trace", "debug", "info", "warn", "error", "[with_fields]", "[with_fields{answer}]", "[with_fields{label}]", "[with_fields{answer=42}]",
  "[with_fields{label=bar}]", "[with_fields{answer=99}]", "[with_fields{label=nope}]", "[with_fields{nonexistent}]", "other_crate=info",
  "other_crate=debug", "trace,other_crate=warn", "warn,other_crate=info",
];

fn main() -> io::Result<()> {
  let terminal = ratatui::init();
  let result = App::new().run(terminal);
  ratatui::restore();
  result
}

/// Terminal app state for interactively evaluating `EnvFilter` strings.
struct App {
  /// Editable filter text area.
  filter:       TextArea<'static>,
  /// Currently selected preset index.
  preset_index: usize,
  /// Whether the app should exit its event loop.
  exit:         bool,
  /// Last evaluated log widget or parse error.
  log_widget:   Result<LogWidget, ParseError>,
}

impl App {
  /// Creates a new instance of the application, ready to run
  #[allow(
    clippy::single_call_fn,
    reason = "keeps terminal state setup separate from the event loop"
  )]
  fn new() -> Self {
    let initial_filter = PRESET_FILTERS.first().map_or_else(String::new, |filter| (*filter).to_owned());
    let mut filter = TextArea::new(vec![initial_filter]);
    let title = "Env Filter Explorer. <Esc> to quit, <Up>/<Down> to select preset";
    filter.set_block(Block::bordered().title(title));
    Self {
      filter,
      preset_index: 0,
      exit: false,
      log_widget: Ok(LogWidget::default()),
    }
  }

  /// The application's main loop until the user exits.
  fn run(mut self, mut terminal: DefaultTerminal) -> io::Result<()> {
    while !self.exit {
      self.log_widget = self.evaluate_filter();
      let _frame = terminal.draw(|frame| self.render(frame))?;
      self.handle_event()?;
    }
    Ok(())
  }

  /// Render the application with a filter input area and a log output area.
  fn render(&self, frame: &mut Frame<'_>) {
    let layout = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]);
    let [filter_area, main_area] = layout.areas(frame.area());
    frame.render_widget(&self.filter, filter_area);
    match self.log_widget.as_ref() {
      Ok(log_widget) => frame.render_widget(log_widget, main_area),
      Err(error) => frame.render_widget(error.to_string().red(), main_area),
    }
  }

  /// Handles a single terminal event (e.g. mouse, keyboard, resize).
  fn handle_event(&mut self) -> io::Result<()> {
    let event = event::read()?;
    let input = Input::from(event);
    let key = input.key;
    if key == Key::Enter {
      return Ok(());
    }
    if key == Key::Esc {
      self.exit = true;
    } else if key == Key::Up {
      self.select_previous_preset();
    } else if key == Key::Down {
      self.select_next_preset();
    } else {
      self.add_input(input);
    }
    Ok(())
  }

  /// Selects the previous preset filter in the list.
  fn select_previous_preset(&mut self) {
    self.select_preset(self.preset_index.saturating_sub(1));
  }

  /// Selects the next preset filter in the list.
  fn select_next_preset(&mut self) {
    let last_index = PRESET_FILTERS.len().saturating_sub(1);
    let next_index = self.preset_index.saturating_add(1).min(last_index);
    self.select_preset(next_index);
  }

  /// Selects a preset filter by index and updates the filter text area.
  fn select_preset(&mut self, index: usize) {
    if let Some(filter) = PRESET_FILTERS.get(index) {
      self.preset_index = index;
      self.filter.select_all();
      let _deleted = self.filter.delete_line_by_head();
      let _inserted = self.filter.insert_str(*filter);
    }
  }

  /// Handles normal keyboard input by adding it to the filter text area.
  fn add_input(&mut self, input: Input) {
    let _input = self.filter.input(input);
  }

  /// Evaluates the current filter and returns a log widget with the filtered logs or an error.
  fn evaluate_filter(&self) -> Result<LogWidget, ParseError> {
    let filter = self.filter.lines().first().map_or_else(String::new, ToOwned::to_owned);
    let env_filter = EnvFilter::builder().parse(filter)?;
    let log_widget = LogWidget::default();
    let subscriber = tracing_subscriber::fmt()
      .with_env_filter(env_filter)
      .with_writer(log_widget.clone())
      .finish();
    with_default(subscriber, || {
      simulate_logging();
      other_crate_span();
    });
    Ok(log_widget)
  }
}

/// A writer that collects logs into a buffer and can be displayed as a widget.
#[derive(Clone, Default, Debug)]
struct LogWidget {
  /// Captured formatted log output.
  buffer: Arc<Mutex<Vec<u8>>>,
}

impl Write for LogWidget {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let mut buffer = self.buffer.lock();
    buffer.write(buf)
  }

  fn flush(&mut self) -> io::Result<()> {
    let mut buffer = self.buffer.lock();
    buffer.flush()
  }
}

impl<'a> MakeWriter<'a> for LogWidget {
  type Writer = Self;

  fn make_writer(&'a self) -> Self::Writer {
    self.clone()
  }
}

impl Widget for &LogWidget {
  /// Displays the logs that have been collected in the buffer.
  ///
  /// If the buffer is empty, it displays "No matching logs".
  fn render(self, area: Rect, buf: &mut Buffer) {
    let logs = {
      let buffer = self.buffer.lock();
      String::from_utf8_lossy(&buffer).into_owned()
    };
    if logs.is_empty() {
      "No matching logs".render(area, buf);
      return;
    }
    match logs.into_text() {
      Ok(text) => text.render(area, buf),
      Err(error) => format!("Error parsing output: {error}").render(area, buf),
    }
  }
}

/// Emits logs across levels and spans for the active filter.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the generated log workload named for interactive filter evaluation"
)]
#[tracing::instrument]
fn simulate_logging() {
  tracing::info!("This is an info message");
  tracing::error!("This is an error message");
  tracing::warn!("This is a warning message");
  tracing::debug!("This is a debug message");
  tracing::trace!("This is a trace message");

  with_fields(42, "bar");
  with_fields(99, "nope");

  trace_span();
  debug_span();
  info_span();
  warn_span();
  error_span();
}

/// Emits an event with named fields used by the filter presets.
#[tracing::instrument]
fn with_fields(answer: u32, label: &'static str) {
  tracing::info!(answer, label, "This is an info message with fields");
}

/// Emits logs inside a trace-level span.
#[allow(
  clippy::single_call_fn,
  reason = "keeps a distinct trace-level span available to the filter explorer"
)]
#[tracing::instrument(level = "trace")]
fn trace_span() {
  tracing::error!("Error message inside a span with trace level");
  tracing::info!("Info message inside a span with trace level");
  tracing::trace!("Trace message inside a span with trace level");
}

/// Emits logs inside a debug-level span.
#[allow(
  clippy::single_call_fn,
  reason = "keeps a distinct debug-level span available to the filter explorer"
)]
#[tracing::instrument]
fn debug_span() {
  tracing::error!("Error message inside a span with debug level");
  tracing::info!("Info message inside a span with debug level");
  tracing::debug!("Debug message inside a span with debug level");
}

/// Emits logs inside an info-level span.
#[allow(
  clippy::single_call_fn,
  reason = "keeps a distinct info-level span available to the filter explorer"
)]
#[tracing::instrument]
fn info_span() {
  tracing::error!("Error message inside a span with info level");
  tracing::info!("Info message inside a span with info level");
  tracing::debug!("Debug message inside a span with info level");
}

/// Emits logs inside a warn-level span.
#[allow(
  clippy::single_call_fn,
  reason = "keeps a distinct warn-level span available to the filter explorer"
)]
#[tracing::instrument]
fn warn_span() {
  tracing::error!("Error message inside a span with warn level");
  tracing::info!("Info message inside a span with warn level");
  tracing::debug!("Debug message inside a span with warn level");
}

/// Emits logs inside an error-level span.
#[allow(
  clippy::single_call_fn,
  reason = "keeps a distinct error-level span available to the filter explorer"
)]
#[tracing::instrument]
fn error_span() {
  tracing::error!("Error message inside a span with error level");
  tracing::info!("Info message inside a span with error level");
  tracing::debug!("Debug message inside a span with error level");
}

/// Emits logs from a non-default target.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the alternate target span available to the filter explorer"
)]
#[tracing::instrument(target = "other_crate")]
fn other_crate_span() {
  tracing::error!(target: "other_crate", "An error message from another crate");
  tracing::warn!(target: "other_crate", "A warning message from another crate");
  tracing::info!(target: "other_crate", "An info message from another crate");
  tracing::debug!(target: "other_crate", "A debug message from another crate");
  tracing::trace!(target: "other_crate", "A trace message from another crate");
}
