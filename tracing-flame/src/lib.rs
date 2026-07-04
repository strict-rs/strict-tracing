//! A Tracing [Layer][`FlameLayer`] for generating a folded stack trace for generating flamegraphs
//! and flamecharts with [`inferno`]
//!
//! # Overview
//!
//! [`tracing`] is a framework for instrumenting Rust programs to collect
//! scoped, structured, and async-aware diagnostics. `tracing-flame` provides helpers
//! for consuming `tracing` instrumentation that can later be visualized as a
//! flamegraph/flamechart. Flamegraphs/flamecharts are useful for identifying performance
//! issues bottlenecks in an application. For more details, see Brendan Gregg's [post]
//! on flamegraphs.
//!
//! *Compiler support: [requires `rustc` 1.96+][msrv]*
//!
//! [msrv]: #supported-rust-versions
//! [post]: http://www.brendangregg.com/flamegraphs.html
//!
//! ## Usage
//!
//! This crate is meant to be used in a two step process:
//!
//! 1. Capture textual representation of the spans that are entered and exited with [`FlameLayer`].
//! 2. Feed the textual representation into `inferno-flamegraph` to generate the flamegraph or
//!    flamechart.
//!
//! *Note*: when using a buffered writer as the writer for a `FlameLayer`, it is necessary to
//! ensure that the buffer has been flushed before the data is passed into
//! [`inferno-flamegraph`]. For more details on how to flush the internal writer
//! of the `FlameLayer`, see the docs for [`FlushGuard`].
//!
//! ## Layer Setup
//!
//! ```rust
//! use std::fs::File;
//! use std::io::BufWriter;
//!
//! use tracing_flame::FlameLayer;
//! use tracing_subscriber::fmt;
//! use tracing_subscriber::prelude::*;
//! use tracing_subscriber::registry::Registry;
//!
//! fn setup_global_subscriber() -> Result<impl Drop, Box<dyn std::error::Error + Send + Sync>> {
//!   let fmt_layer = fmt::Layer::default();
//!
//!   let (flame_layer, _guard) = FlameLayer::with_file("./tracing.folded")?;
//!
//!   let subscriber = Registry::default().with(fmt_layer).with(flame_layer);
//!
//!   tracing::subscriber::set_global_default(subscriber)?;
//!   Ok(_guard)
//! }
//!
//! // your code here ..
//! ```
//!
//! As an alternative, you can provide _any_ type that implements `std::io::Write` to
//! `FlameLayer::new`.
//!
//! ## Generating the Image
//!
//! To convert the textual representation of a flamegraph to a visual one, first install `inferno`:
//!
//! ```console
//! cargo install inferno
//! ```
//!
//! Then, pass the file created by `FlameLayer` into `inferno-flamegraph`:
//!
//! ```console
//! # flamegraph
//! cat tracing.folded | inferno-flamegraph > tracing-flamegraph.svg
//!
//! # flamechart
//! cat tracing.folded | inferno-flamegraph --flamechart > tracing-flamechart.svg
//! ```
//!
//! ## Differences between `flamegraph`s and `flamechart`s
//!
//! By default, `inferno-flamegraph` creates flamegraphs. Flamegraphs operate by
//! that collapsing identical stack frames and sorting them on the frame's names.
//!
//! This behavior is great for multithreaded programs and long-running programs
//! where the same frames occur _many_ times, for short durations, because it reduces
//! noise in the graph and gives the reader a better idea of the
//! overall time spent in each part of the application.
//!
//! However, it is sometimes desirable to preserve the _exact_ ordering of events
//! as they were emitted by `tracing-flame`, so that it is clear when each
//! span is entered relative to others and get an accurate visual trace of
//! the execution of your program. This representation is best created with a
//! _flamechart_, which _does not_ sort or collapse identical stack frames.
//!
//! [`inferno`]: https://docs.rs/inferno
//! [`inferno-flamegraph`]: https://docs.rs/inferno/0.9.5/inferno/index.html#producing-a-flame-graph
//!
//! ## Supported Rust Versions
//!
//! Tracing is built against the latest stable release. The minimum supported
//! version is 1.96. The current Tracing version is not guaranteed to build on
//! Rust versions earlier than the minimum supported version.
//!
//! Tracing follows the same compiler support policies as the rest of the Tokio
//! project. The current stable Rust compiler and the three most recent minor
//! versions before it will always be supported. For example, if the current
//! stable compiler version is 1.69, the minimum supported version will not be
//! increased past 1.66, three minor versions prior. Increasing the minimum
//! supported compiler version is not considered a semver breaking change as
//! long as doing so complies with this policy.
#![doc(
  html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/logo-type.png",
  html_favicon_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/favicon.ico",
  issue_tracker_base_url = "https://github.com/strict-rs/strict-tracing/issues/"
)]
#![cfg_attr(docsrs, deny(rustdoc::broken_intra_doc_links))]
use std::cell::Cell;
use std::convert::identity;
use std::fmt;
use std::fmt::Write as _;
use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::LazyLock;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use tracing::Subscriber;
use tracing::span;
use tracing::subscriber::SubscriberResult;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::registry::SpanRef;

/// Error types for fallible `tracing-flame` operations.
mod error;

pub use error::FlameError;

/// Shared process start instant used to seed per-thread elapsed timing.
static START: LazyLock<Instant> = LazyLock::new(Instant::now);

thread_local! {
    static LAST_EVENT: Cell<Instant> = Cell::new(*START);

    static THREAD_NAME: String = {
        let current_thread = thread::current();
        let mut thread_name = format!("{:?}", current_thread.id());
        if let Some(name) = current_thread.name() {
            thread_name.push('-');
            thread_name.push_str(name);
        }
        thread_name
    };
}

/// Shared writer command sender owned by layers and flush guards.
struct WriterHandle<W> {
  /// Channel used to send writes and flush requests to the writer thread.
  commands: mpsc::Sender<WriterCommand>,
  /// Carries the writer type without requiring `W: Sync` on the layer.
  _writer:  PhantomData<fn() -> W>,
}

impl<W> fmt::Debug for WriterHandle<W> {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("WriterHandle").finish_non_exhaustive()
  }
}

impl<W> Clone for WriterHandle<W> {
  fn clone(&self) -> Self {
    Self {
      commands: self.commands.clone(),
      _writer:  PhantomData,
    }
  }
}

impl<W> WriterHandle<W>
where
  W: Write + Send + 'static,
{
  /// Spawns the background writer loop for a `FlameLayer` writer.
  #[allow(
    clippy::single_call_fn,
    reason = "constructor isolates writer thread and channel setup from the public layer constructor"
  )]
  fn new(mut writer: W) -> Self {
    let (commands, receiver) = mpsc::channel();
    let writer_thread = thread::spawn(move || {
      while let Ok(command) = receiver.recv() {
        match command {
          WriterCommand::WriteLine(line) => {
            let _write_result: io::Result<()> = writeln!(writer, "{line}");
          }
          WriterCommand::Flush(flush_complete) => {
            let flush_result = writer.flush();
            let _send_result: Result<(), mpsc::SendError<io::Result<()>>> = flush_complete.send(flush_result);
          }
        }
      }
    });
    drop(writer_thread);

    Self {
      commands,
      _writer: PhantomData,
    }
  }

  /// Sends a folded stack line to the writer loop.
  fn write_line(&self, line: String) {
    let _send_result: Result<(), mpsc::SendError<WriterCommand>> = self.commands.send(WriterCommand::WriteLine(line));
  }

  /// Flushes the writer loop after all previously sent lines.
  fn flush(&self) -> io::Result<()> {
    let (flush_complete, flush_result) = mpsc::channel();

    match self.commands.send(WriterCommand::Flush(flush_complete)) {
      Ok(()) => flush_result.recv().map_or(Ok(()), identity),
      Err(_) => Ok(()),
    }
  }
}

/// Commands sent to the writer loop.
enum WriterCommand {
  /// Write one folded stack line.
  WriteLine(
    /// The folded stack line, without a trailing newline.
    String,
  ),

  /// Flush the underlying writer.
  Flush(
    /// Completion channel carrying the flush result.
    mpsc::Sender<io::Result<()>>,
  ),
}

/// A `Layer` that records span open/close events as folded flamegraph stack
/// samples.
///
/// The output of `FlameLayer` emulates the output of commands like `perf` once
/// they've been collapsed by `inferno-flamegraph`. The output of this layer
/// should look similar to the output of the following commands:
///
/// ```sh
/// perf record --call-graph dwarf target/release/mybin
/// perf script | inferno-collapse-perf > stacks.folded
/// ```
///
/// # Sample Counts
///
/// Because `tracing-flame` doesn't use sampling, the number at the end of each
/// folded stack trace does not represent a number of samples of that stack.
/// Instead, the numbers on each line are the number of nanoseconds since the
/// last event in the same thread.
///
/// # Dropping and Flushing
///
/// If you use a global subscriber the drop implementations on your various
/// layers will not get called when your program exits. This means that if
/// you're using a buffered writer as the inner writer for the `FlameLayer`
/// you're not guaranteed to see all the events that have been emitted in the
/// file by default.
///
/// To ensure all data is flushed when the program exits, `FlameLayer` exposes
/// the [`flush_on_drop`] function, which returns a [`FlushGuard`]. The `FlushGuard`
/// will flush the writer when it is dropped. If necessary, it can also be used to manually
/// flush the writer.
///
/// [`flush_on_drop`]: FlameLayer::flush_on_drop
#[derive(Debug)]
pub struct FlameLayer<S, W> {
  /// Shared writer command sender.
  writer: WriterHandle<W>,
  /// Output formatting switches.
  config: Config,
  /// Subscriber type marker.
  _inner: PhantomData<S>,
}

/// Output formatting configuration for folded stack lines.
#[derive(Debug)]
struct Config {
  /// Whether to include samples where no spans are open.
  empty_samples: EmptySamples,

  /// Whether to record the `thread_id` in each stack.
  thread_names: ThreadNames,

  /// Whether to display the `module_path`.
  module_path: ModulePath,

  /// Whether to display the source file and line.
  file_and_line: FileAndLine,
}

impl Config {
  /// Returns true when empty samples should be omitted.
  const fn omits_empty_samples(&self) -> bool {
    matches!(self.empty_samples, EmptySamples::Omit)
  }

  /// Returns true when module paths should be shown.
  const fn shows_module_path(&self) -> bool {
    matches!(self.module_path, ModulePath::Include)
  }

  /// Returns true when source file and line should be shown.
  const fn shows_file_and_line(&self) -> bool {
    matches!(self.file_and_line, FileAndLine::Include)
  }

  /// Writes the thread prefix for a folded stack line.
  fn write_thread_prefix(&self, stack: &mut String) {
    match self.thread_names {
      ThreadNames::Collapsed => stack.push_str("all-threads"),
      ThreadNames::PerThread => THREAD_NAME.with(|name| stack.push_str(name.as_str())),
    }
  }
}

/// Whether to include samples with no active spans.
#[derive(Debug)]
enum EmptySamples {
  /// Include samples with no active spans.
  Include,
  /// Omit samples with no active spans.
  Omit,
}

/// Whether folded stack lines include per-thread names.
#[derive(Debug)]
enum ThreadNames {
  /// Include each thread name in folded stack lines.
  PerThread,
  /// Collapse all threads under one synthetic name.
  Collapsed,
}

/// Whether folded stack frames include module paths.
#[derive(Debug)]
enum ModulePath {
  /// Include module paths.
  Include,
  /// Omit module paths.
  Omit,
}

/// Whether folded stack frames include source file and line numbers.
#[derive(Debug)]
enum FileAndLine {
  /// Include source file and line numbers.
  Include,
  /// Omit source file and line numbers.
  Omit,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      empty_samples: EmptySamples::Include,
      thread_names:  ThreadNames::PerThread,
      module_path:   ModulePath::Include,
      file_and_line: FileAndLine::Omit,
    }
  }
}

/// An RAII guard for managing flushing a global writer that is
/// otherwise inaccessible.
///
/// This type is only needed when using
/// `tracing::subscriber::set_global_default`, which prevents the drop
/// implementation of layers from running when the program exits.
#[must_use]
#[derive(Debug)]
pub struct FlushGuard<W>
where
  W: Write + Send + 'static,
{
  /// Shared writer command sender.
  writer: WriterHandle<W>,
}

impl<S, W> FlameLayer<S, W>
where
  S: Subscriber + for<'span> LookupSpan<'span>,
  W: Write + Send + 'static,
{
  /// Returns a new `FlameLayer` that outputs all folded stack samples to the
  /// provided writer.
  #[allow(
    clippy::single_call_fn,
    reason = "public constructor remains the API for layers over arbitrary writers"
  )]
  pub fn new(writer: W) -> Self {
    // Initialize the start used by all threads when initializing the
    // LAST_EVENT when constructing the layer
    let _start = LazyLock::force(&START);
    Self {
      writer: WriterHandle::new(writer),
      config: Config::default(),
      _inner: PhantomData,
    }
  }

  /// Returns a `FlushGuard` which will flush the `FlameLayer`'s writer when
  /// it is dropped, or when `flush` is manually invoked on the guard.
  pub fn flush_on_drop(&self) -> FlushGuard<W> {
    FlushGuard {
      writer: self.writer.clone(),
    }
  }

  /// Configures whether or not periods of time where no spans are entered
  /// should be included in the output.
  ///
  /// Defaults to `true`.
  ///
  /// Setting this feature to false can help with situations where no span is
  /// active for large periods of time. This can include time spent idling, or
  /// doing uninteresting work that isn't being measured.
  /// When a large number of empty samples are recorded, the flamegraph
  /// may be harder to interpret and navigate, since the recorded spans will
  /// take up a correspondingly smaller percentage of the graph. In some
  /// cases, a large number of empty samples may even hide spans which
  /// would otherwise appear in the flamegraph.
  #[must_use]
  pub const fn with_empty_samples(mut self, enabled: bool) -> Self {
    self.config.empty_samples = if enabled {
      EmptySamples::Include
    } else {
      EmptySamples::Omit
    };
    self
  }

  /// Configures whether or not spans from different threads should be
  /// collapsed into one pool of events.
  ///
  /// Defaults to `false`.
  ///
  /// Setting this feature to true can help with applications that distribute
  /// work evenly across many threads, such as thread pools. In such
  /// cases it can be difficult to get an overview of where the application
  /// as a whole spent most of its time, because work done in the same
  /// span may be split up across many threads.
  #[must_use]
  pub const fn with_threads_collapsed(mut self, enabled: bool) -> Self {
    self.config.thread_names = if enabled {
      ThreadNames::Collapsed
    } else {
      ThreadNames::PerThread
    };
    self
  }

  /// Configures whether or not module paths should be included in the output.
  #[must_use]
  pub const fn with_module_path(mut self, enabled: bool) -> Self {
    self.config.module_path = if enabled { ModulePath::Include } else { ModulePath::Omit };
    self
  }

  /// Configures whether or not file and line should be included in the output.
  #[must_use]
  pub const fn with_file_and_line(mut self, enabled: bool) -> Self {
    self.config.file_and_line = if enabled {
      FileAndLine::Include
    } else {
      FileAndLine::Omit
    };
    self
  }
}

impl<W> FlushGuard<W>
where
  W: Write + Send + 'static,
{
  /// Flush the internal writer of the `FlameLayer`, ensuring that all
  /// intermediately buffered contents reach their destination.
  ///
  /// # Errors
  ///
  /// Returns an error when the underlying writer cannot be flushed.
  pub fn flush(&self) -> Result<(), FlameError> {
    self.writer.flush().map_err(FlameError::flush_file)
  }
}

impl<W> Drop for FlushGuard<W>
where
  W: Write + Send + 'static,
{
  fn drop(&mut self) {
    if let Err(flush_error) = self.flush() {
      flush_error.report();
    }
  }
}

impl<S> FlameLayer<S, BufWriter<File>>
where
  S: Subscriber + for<'span> LookupSpan<'span>,
{
  /// Constructs a `FlameLayer` that outputs to a `BufWriter` to the given path, and a
  /// `FlushGuard` to ensure the writer is flushed.
  ///
  /// # Errors
  ///
  /// Returns an error when the output file cannot be created.
  pub fn with_file(path: impl AsRef<Path>) -> Result<(Self, FlushGuard<BufWriter<File>>), FlameError> {
    let output_path = path.as_ref();
    let file = File::create(output_path).map_err(|source| FlameError::create_file(output_path.to_path_buf(), source))?;
    let writer = BufWriter::new(file);
    let layer = Self::new(writer);
    let guard = layer.flush_on_drop();
    Ok((layer, guard))
  }
}

impl<S, W> Layer<S> for FlameLayer<S, W>
where
  S: Subscriber + for<'span> LookupSpan<'span>,
  W: Write + Send + 'static,
{
  fn on_enter(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
    let samples = Self::time_since_last_event();

    let Some(first) = ctx.span(id) else {
      return Ok(());
    };

    if self.config.omits_empty_samples() && first.parent().is_none() {
      return Ok(());
    }

    let mut stack = String::new();
    self.config.write_thread_prefix(&mut stack);

    if let Some(second) = first.parent() {
      for parent in second.scope().root_to_leaf() {
        stack.push(';');
        write_stack_frame(&mut stack, &parent, &self.config);
      }
    }

    write_elapsed_sample(&mut stack, samples);

    self.writer.write_line(stack);

    Ok(())
  }

  fn on_exit(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult {
    let samples = Self::time_since_last_event();
    let Some(first) = ctx.span(id) else {
      return Ok(());
    };

    let mut stack = String::new();
    self.config.write_thread_prefix(&mut stack);

    for parent in first.scope().root_to_leaf() {
      stack.push(';');
      write_stack_frame(&mut stack, &parent, &self.config);
    }

    write_elapsed_sample(&mut stack, samples);

    self.writer.write_line(stack);

    Ok(())
  }
}

impl<S, W> FlameLayer<S, W>
where
  S: Subscriber + for<'span> LookupSpan<'span>,
  W: Write + Send + 'static,
{
  /// Returns the elapsed time since the last event on this thread.
  fn time_since_last_event() -> Duration {
    let now = Instant::now();

    let previous_event = LAST_EVENT.with(|event| {
      let previous_event = event.get();
      event.set(now);
      previous_event
    });

    now.saturating_duration_since(previous_event)
  }
}

/// Writes one folded stack frame into the destination line.
fn write_stack_frame<S>(dest: &mut String, span: &SpanRef<'_, S>, config: &Config)
where
  S: Subscriber + for<'span> LookupSpan<'span>,
{
  if config.shows_module_path()
    && let Some(module_path) = span.metadata().module_path()
  {
    dest.push_str(module_path);
    dest.push_str("::");
  }

  dest.push_str(span.name());

  if config.shows_file_and_line() {
    if let Some(file) = span.metadata().file() {
      dest.push(':');
      dest.push_str(file);
    }

    if let Some(line) = span.metadata().line() {
      let _format_result: fmt::Result = write!(dest, ":{line}");
    }
  }
}

/// Writes the elapsed sample duration suffix into the destination line.
fn write_elapsed_sample(dest: &mut String, elapsed: Duration) {
  dest.push(' ');
  let elapsed_nanos = elapsed.as_nanos();
  let _format_result: fmt::Result = write!(dest, "{elapsed_nanos}");
}
