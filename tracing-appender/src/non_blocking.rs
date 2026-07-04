//! A non-blocking, off-thread writer.
//!
//! This spawns a dedicated worker thread which is responsible for writing log
//! lines to the provided writer. When a line is written using the returned
//! `NonBlocking` struct's `make_writer` method, it will be enqueued to be
//! written by the worker thread.
//!
//! The queue has a fixed capacity, and if it becomes full, any logs written
//! to it will be dropped until capacity is once again available. This may
//! occur if logs are consistently produced faster than the worker thread can
//! output them. The queue capacity and behavior when full (i.e., whether to
//! drop logs or to exert backpressure to slow down senders) can be configured
//! using [`NonBlockingBuilder::default()`][builder].
//! This function returns the default configuration. It is equivalent to:
//!
//! ```rust
//! # use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
//! # fn doc() -> (NonBlocking, WorkerGuard) {
//! tracing_appender::non_blocking(std::io::stdout())
//! # }
//! ```
//! [builder]: NonBlockingBuilder::default
//!
//! <br/> This function returns a tuple of `NonBlocking` and `WorkerGuard`.
//! `NonBlocking` implements [`MakeWriter`] which integrates with `tracing_subscriber`.
//! `WorkerGuard` is a drop guard that is responsible for flushing any remaining logs when
//! the program terminates.
//!
//! Note that the `WorkerGuard` returned by `non_blocking` _must_ be assigned to a binding that
//! is not `_`, as `_` will result in the `WorkerGuard` being dropped immediately.
//! Unintentional drops of `WorkerGuard` remove the guarantee that logs will be flushed
//! during a program's termination, in a panic or otherwise.
//!
//! See [`WorkerGuard`] for examples of using the guard.
//!
//! # Examples
//!
//! ``` rust
//! # fn docs() {
//! let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());
//! let subscriber = tracing_subscriber::fmt().with_writer(non_blocking);
//! tracing::subscriber::with_default(subscriber.finish(), || {
//!   tracing::event!(tracing::Level::INFO, "Hello");
//! });
//! # }
//! ```
/// Background worker implementation for non-blocking writers.
#[path = "worker.rs"]
mod worker;

use std::io;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::SendTimeoutError;
use crossbeam_channel::Sender;
use crossbeam_channel::bounded;
use thiserror::Error;
use tracing_subscriber::fmt::MakeWriter;

use self::worker::Worker;
use crate::Msg;

/// The default maximum number of buffered log lines.
///
/// If [`NonBlocking`] is lossy, it will drop spans/events at capacity.
/// If [`NonBlocking`] is _not_ lossy, backpressure will be exerted on
/// senders, causing them to block their respective threads until there
/// is available capacity.
///
/// Recommended to be a power of 2.
pub const DEFAULT_BUFFERED_LINES_LIMIT: usize = 128_000;

/// Timeout for sending the shutdown message to the worker.
const SHUTDOWN_SIGNAL_TIMEOUT: Duration = Duration::from_millis(100);

/// Timeout for waiting until the worker acknowledges shutdown.
const WORKER_FLUSH_TIMEOUT: Duration = Duration::from_secs(1);

/// Error returned when a non-blocking worker thread cannot be spawned.
#[derive(Debug, Error)]
#[error("failed to spawn `tracing-appender` non-blocking worker thread")]
pub struct WorkerSpawnError {
  /// Underlying operating-system thread-spawn error.
  #[from]
  source: io::Error,
}

impl WorkerSpawnError {
  /// Returns the [`io::ErrorKind`] from the underlying spawn error.
  #[must_use]
  pub fn kind(&self) -> io::ErrorKind {
    self.source.kind()
  }
}

/// A guard that flushes spans/events associated to a [`NonBlocking`] on a drop
///
/// Writing to a [`NonBlocking`] writer will **not** immediately write a span or event to the
/// underlying output. Instead, the span or event will be written by a dedicated logging thread at
/// some later point. To increase throughput, the non-blocking writer will flush to the underlying
/// output on a periodic basis rather than every time a span or event is written. This means that if
/// the program terminates abruptly (such as through an uncaught `panic` or a `std::process::exit`),
/// some spans or events may not be written.
///
/// Since spans/events and events recorded near a crash are often necessary for diagnosing the
/// failure, `WorkerGuard` provides a mechanism to ensure that _all_ buffered logs are flushed to
/// their output. `WorkerGuard` should be assigned in the `main` function or whatever the entrypoint
/// of the program is. This will ensure that the guard will be dropped during an unwinding or when
/// `main` exits successfully.
///
/// # Examples
///
/// ``` rust
/// # fn doc() {
/// let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());
/// let subscriber = tracing_subscriber::fmt().with_writer(non_blocking);
/// tracing::subscriber::with_default(subscriber.finish(), || {
///   // Emit some tracing events within context of the non_blocking `_guard` and tracing subscriber
///   tracing::event!(tracing::Level::INFO, "Hello");
/// });
/// // Exiting the context of `main` will drop the `_guard` and any remaining logs should get flushed
/// # }
/// ```
#[must_use]
#[derive(Debug)]
pub struct WorkerGuard {
  /// Worker thread handle retained for the guard's lifetime.
  _guard:   Option<JoinHandle<()>>,
  /// Sender used to request worker shutdown.
  sender:   Sender<Msg>,
  /// Sender used to wait for worker shutdown acknowledgement.
  shutdown: Sender<()>,
}

/// A non-blocking writer.
///
/// While the line between "blocking" and "non-blocking" IO is fuzzy, writing to a file is typically
/// considered to be a _blocking_ operation. For an application whose `Subscriber` writes spans and
/// events as they are emitted, an application might find the latency profile to be unacceptable.
/// `NonBlocking` moves the writing out of an application's data path by sending spans and events
/// to a dedicated logging thread.
///
/// This struct implements [`MakeWriter`] from the `tracing-subscriber`
/// crate. Therefore, it can be used with the [`tracing_subscriber::fmt`][fmt] module
/// or with any other subscriber/layer implementation that uses the `MakeWriter` trait.
///
/// [fmt]: mod@tracing_subscriber::fmt
#[derive(Clone, Debug)]
pub struct NonBlocking {
  /// Counter for lines dropped by the non-blocking queue.
  error_counter:     ErrorCounter,
  /// Queue sender shared by writer clones.
  channel:           Sender<Msg>,
  /// Whether full queues drop lines instead of blocking writers.
  is_lossy:          bool,
  /// Stored worker spawn failure returned by writes from infallible constructors.
  worker_error_kind: Option<io::ErrorKind>,
}

/// Tracks the number of times a log line was dropped by the background thread.
///
/// If the non-blocking writer is not configured in [lossy mode], the error
/// count should always be 0.
///
/// [lossy mode]: NonBlockingBuilder::lossy
#[derive(Clone, Debug)]
pub struct ErrorCounter(Arc<AtomicUsize>);

impl NonBlocking {
  /// Returns a new `NonBlocking` writer wrapping the provided `writer`.
  ///
  /// The returned `NonBlocking` writer will have the [default configuration][default] values.
  /// Other configurations can be specified using the [builder] interface.
  /// Use [`try_new`](Self::try_new) to handle worker thread spawn failures during construction.
  /// If the worker cannot be spawned, this constructor returns a writer whose writes return
  /// the stored spawn error.
  ///
  /// [default]: NonBlockingBuilder::default
  /// [builder]: NonBlockingBuilder
  #[allow(
    clippy::single_call_fn,
    reason = "public convenience constructor remains part of the crate API alongside the fallible builder"
  )]
  pub fn new<T: Write + Send + 'static>(writer: T) -> (Self, WorkerGuard) {
    NonBlockingBuilder::default().finish(writer)
  }

  /// Returns a new `NonBlocking` writer, reporting worker thread spawn failures.
  ///
  /// The returned `NonBlocking` writer will have the [default configuration][default] values.
  /// Other configurations can be specified using the [builder] interface.
  ///
  /// [default]: NonBlockingBuilder::default
  /// [builder]: NonBlockingBuilder
  ///
  /// # Errors
  ///
  /// Returns [`WorkerSpawnError`] if the operating system refuses to spawn the
  /// background worker thread.
  pub fn try_new<T: Write + Send + 'static>(writer: T) -> Result<(Self, WorkerGuard), WorkerSpawnError> {
    NonBlockingBuilder::default().try_finish(writer)
  }

  /// Creates a non-blocking writer with the provided builder configuration.
  fn create<T: Write + Send + 'static>(
    writer: T,
    buffered_lines_limit: usize,
    is_lossy: bool,
    thread_name: String,
  ) -> Result<(Self, WorkerGuard), WorkerSpawnError> {
    let (sender, receiver) = bounded(buffered_lines_limit);

    let (shutdown_sender, shutdown_receiver) = bounded(0);

    let worker = Worker::new(receiver, writer, shutdown_receiver);
    let worker_guard = WorkerGuard::new(Some(worker.worker_thread(thread_name)?), sender.clone(), shutdown_sender);

    Ok((
      Self {
        channel: sender,
        error_counter: ErrorCounter::new(),
        is_lossy,
        worker_error_kind: None,
      },
      worker_guard,
    ))
  }

  /// Creates a writer that reports a worker spawn failure when written to.
  #[allow(
    clippy::single_call_fn,
    reason = "fallback constructor keeps failed writer state paired with its inert guard"
  )]
  fn create_failed(buffered_lines_limit: usize, is_lossy: bool, worker_error_kind: io::ErrorKind) -> (Self, WorkerGuard) {
    let (sender, receiver) = bounded(buffered_lines_limit);
    let (shutdown_sender, shutdown_receiver) = bounded(0);
    drop(receiver);
    drop(shutdown_receiver);

    let worker_guard = WorkerGuard::new(None, sender.clone(), shutdown_sender);

    (
      Self {
        channel: sender,
        error_counter: ErrorCounter::new(),
        is_lossy,
        worker_error_kind: Some(worker_error_kind),
      },
      worker_guard,
    )
  }

  /// Returns a counter for the number of times logs where dropped. This will always return zero if
  /// `NonBlocking` is not lossy.
  #[must_use]
  pub fn error_counter(&self) -> ErrorCounter {
    self.error_counter.clone()
  }
}

/// A builder for [`NonBlocking`].
#[derive(Debug)]
pub struct NonBlockingBuilder {
  /// Maximum number of lines buffered before applying full-queue behavior.
  buffered_lines_limit: usize,
  /// Whether full queues drop lines instead of blocking writers.
  is_lossy:             bool,
  /// Name assigned to the background worker thread.
  thread_name:          String,
}

impl NonBlockingBuilder {
  /// Sets the number of lines to buffer before dropping logs or exerting backpressure on senders
  #[must_use]
  pub const fn buffered_lines_limit(mut self, buffered_lines_limit: usize) -> Self {
    self.buffered_lines_limit = buffered_lines_limit;
    self
  }

  /// Sets whether `NonBlocking` should be lossy or not.
  ///
  /// If set to `true`, logs will be dropped when the buffered limit is reached. If `false`,
  /// backpressure will be exerted on senders, blocking them until the buffer has capacity again.
  ///
  /// By default, the built `NonBlocking` will be lossy.
  #[must_use]
  pub const fn lossy(mut self, is_lossy: bool) -> Self {
    self.is_lossy = is_lossy;
    self
  }

  /// Override the worker thread's name.
  ///
  /// The default worker thread name is "tracing-appender".
  #[must_use]
  pub fn thread_name(mut self, name: &str) -> Self {
    self.thread_name.clear();
    self.thread_name.push_str(name);
    self
  }

  /// Completes the builder, returning the configured `NonBlocking`.
  ///
  /// Use [`try_finish`](Self::try_finish) to handle worker thread spawn failures during
  /// construction. If the worker cannot be spawned, this method returns a writer whose
  /// writes return the stored spawn error.
  pub fn finish<T: Write + Send + 'static>(self, writer: T) -> (NonBlocking, WorkerGuard) {
    let buffered_lines_limit = self.buffered_lines_limit;
    let is_lossy = self.is_lossy;

    match NonBlocking::create(writer, buffered_lines_limit, is_lossy, self.thread_name) {
      Ok(pair) => pair,
      Err(error) => NonBlocking::create_failed(buffered_lines_limit, is_lossy, error.kind()),
    }
  }

  /// Completes the builder, returning the configured `NonBlocking`.
  ///
  /// # Errors
  ///
  /// Returns [`WorkerSpawnError`] if the operating system refuses to spawn the
  /// background worker thread.
  pub fn try_finish<T: Write + Send + 'static>(self, writer: T) -> Result<(NonBlocking, WorkerGuard), WorkerSpawnError> {
    NonBlocking::create(writer, self.buffered_lines_limit, self.is_lossy, self.thread_name)
  }
}

impl Default for NonBlockingBuilder {
  fn default() -> Self {
    Self {
      buffered_lines_limit: DEFAULT_BUFFERED_LINES_LIMIT,
      is_lossy:             true,
      thread_name:          "tracing-appender".to_owned(),
    }
  }
}

impl Write for NonBlocking {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let buffer_size = buf.len();

    match self.worker_error_kind {
      Some(error_kind) => Err(error_kind.into()),
      None if self.is_lossy => {
        if self.channel.try_send(Msg::Line(buf.to_vec())).is_err() {
          self.error_counter.incr_saturating();
        }
        Ok(buffer_size)
      }
      None => match self.channel.send(Msg::Line(buf.to_vec())) {
        Ok(()) => Ok(buffer_size),
        Err(_error) => Err(io::ErrorKind::BrokenPipe.into()),
      },
    }
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }

  #[inline]
  fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
    let bytes_written = self.write(buf)?;
    if bytes_written == buf.len() {
      Ok(())
    } else {
      Err(io::ErrorKind::WriteZero.into())
    }
  }
}

impl<'a> MakeWriter<'a> for NonBlocking {
  type Writer = Self;

  fn make_writer(&'a self) -> Self::Writer {
    Self::clone(self)
  }
}

impl WorkerGuard {
  /// Creates a guard for a worker thread and its shutdown channels.
  const fn new(handle: Option<JoinHandle<()>>, sender: Sender<Msg>, shutdown: Sender<()>) -> Self {
    Self {
      _guard: handle,
      sender,
      shutdown,
    }
  }
}

impl Drop for WorkerGuard {
  fn drop(&mut self) {
    match self.sender.send_timeout(Msg::Shutdown, SHUTDOWN_SIGNAL_TIMEOUT) {
      Ok(()) => {
        // Attempt to wait for `Worker` to flush all messages before dropping. This happens
        // when the `Worker` calls `recv()` on a zero-capacity channel. Use `send_timeout`
        // so that drop is not blocked indefinitely.
        // TODO: Make timeout configurable.
        let _shutdown_result = self.shutdown.send_timeout((), WORKER_FLUSH_TIMEOUT);
      }
      Err(SendTimeoutError::Disconnected(_message) | SendTimeoutError::Timeout(_message)) => {}
    }
  }
}

// === impl ErrorCounter ===

impl ErrorCounter {
  /// Creates a zeroed dropped-line counter.
  #[must_use]
  fn new() -> Self {
    Self(Arc::new(AtomicUsize::new(0)))
  }

  /// Returns the number of log lines that have been dropped.
  ///
  /// If the non-blocking writer is not configured in [lossy mode], the error
  /// count should always be 0.
  ///
  /// [lossy mode]: NonBlockingBuilder::lossy
  #[must_use]
  pub fn dropped_lines(&self) -> usize {
    self.0.load(Ordering::Acquire)
  }

  /// Increments the dropped-line count without overflowing.
  fn incr_saturating(&self) {
    let mut curr = self.0.load(Ordering::Acquire);
    // We don't need to enter the CAS loop if the current value is already
    // `usize::MAX`.
    if curr == usize::MAX {
      return;
    }

    // This is implemented as a CAS loop rather than as a simple
    // `fetch_add`, because we don't want to wrap on overflow. Instead, we
    // need to ensure that saturating addition is performed.
    loop {
      let val = curr.saturating_add(1);
      match self.0.compare_exchange(curr, val, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => return,
        Err(actual) => curr = actual,
      }
    }
  }
}
/// Tests for non-blocking writer queue behavior.
#[cfg(test)]
mod test {
  use std::sync::mpsc;
  use std::thread::JoinHandle;
  use std::thread::{
    self,
  };
  use std::time::Duration;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use tracing::subscriber;

  use super::*;

  /// Number of writer threads spawned by the multi-threaded test.
  const THREAD_COUNT: usize = 10;

  /// Timeout used while collecting events written by worker threads.
  const EVENT_RECV_TIMEOUT: Duration = Duration::from_secs(5);

  /// Test writer that forwards written buffers over a synchronous channel.
  struct MockWriter {
    /// Channel used to expose writes to the test thread.
    sender: mpsc::SyncSender<String>,
  }

  impl MockWriter {
    /// Creates a mock writer and its receiving half.
    #[must_use]
    fn new(capacity: usize) -> (Self, mpsc::Receiver<String>) {
      let (sender, receiver) = mpsc::sync_channel(capacity);
      (
        Self {
          sender,
        },
        receiver,
      )
    }
  }

  impl Write for MockWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
      let buffer_length = buf.len();
      let line = String::from_utf8_lossy(buf).into_owned();
      let _send_result = self.sender.send(line);
      Ok(buffer_length)
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  /// Joins a test thread and converts thread panics into test failures.
  fn join_test_thread(handle: JoinHandle<Result<(), TestFailure>>, context: &'static str) -> Result<(), TestFailure> {
    match handle.join() {
      Ok(result) => result,
      Err(_panic) => Err(TestFailure::Condition {
        context,
      }),
    }
  }

  /// Verifies non-lossy writers block instead of dropping lines at capacity.
  #[test]
  fn backpressure_exerted() -> Result<(), TestFailure> {
    let (mock_writer, receiver) = MockWriter::new(1);

    let (mut non_blocking, _guard) = NonBlockingBuilder::default()
      .lossy(false)
      .buffered_lines_limit(1)
      .finish(mock_writer);

    let error_count = non_blocking.error_counter();

    ensure_ok(non_blocking.write_all(b"Hello"), "write initial line")?;
    ensure_eq(&error_count.dropped_lines(), &0, "initial write does not drop lines")?;

    let handle = thread::spawn(move || ensure_ok(non_blocking.write_all(b", World"), "write blocked line"));

    // Sleep a little to ensure previously spawned thread gets blocked on write.
    thread::sleep(Duration::from_millis(100));
    // We should not drop logs when blocked.
    ensure_eq(&error_count.dropped_lines(), &0, "blocked write does not drop lines")?;

    // Read the first message to unblock sender.
    let mut line = ensure_ok(receiver.recv(), "receive initial line")?;
    ensure(line == "Hello", "initial line contents")?;

    // Wait for thread to finish.
    join_test_thread(handle, "writer thread should not panic")?;

    // Thread has joined, we should be able to read the message it sent.
    line = ensure_ok(receiver.recv(), "receive blocked line")?;
    ensure(line == ", World", "blocked line contents")
  }

  /// Writes one message and pauses long enough for worker scheduling.
  fn write_non_blocking(non_blocking: &mut NonBlocking, msg: &[u8]) -> Result<(), TestFailure> {
    ensure_ok(non_blocking.write_all(msg), "write non-blocking line")?;

    // Sleep a bit to prevent races.
    thread::sleep(Duration::from_millis(200));
    Ok(())
  }

  /// Verifies lossy writers count dropped lines when the queue is full.
  #[test]
  #[ignore = "flaky timing-sensitive channel backpressure test; see tokio-rs/tracing#751"]
  fn logs_dropped_if_lossy() -> Result<(), TestFailure> {
    let (mock_writer, receiver) = MockWriter::new(1);

    let (mut non_blocking, _guard) = NonBlockingBuilder::default()
      .lossy(true)
      .buffered_lines_limit(1)
      .finish(mock_writer);

    let error_count = non_blocking.error_counter();

    // First write will not block
    write_non_blocking(&mut non_blocking, b"Hello")?;
    ensure_eq(&error_count.dropped_lines(), &0, "first lossy write does not drop lines")?;

    // Second write will not block as Worker will have called `recv` on channel.
    // "Hello" is not yet consumed. MockWriter call to write_all will block until
    // "Hello" is consumed.
    write_non_blocking(&mut non_blocking, b", World")?;
    ensure_eq(&error_count.dropped_lines(), &0, "second lossy write does not drop lines")?;

    // Will sit in NonBlocking channel's buffer.
    write_non_blocking(&mut non_blocking, b"Test")?;
    ensure_eq(&error_count.dropped_lines(), &0, "buffered lossy write does not drop lines")?;

    // Allow a line to be written. "Hello" message will be consumed.
    // ", World" will be able to write to MockWriter.
    // "Test" will block on call to MockWriter's `write_all`
    let line = ensure_ok(receiver.recv(), "receive first lossy line")?;
    ensure(line == "Hello", "first lossy line contents")?;

    // This will block as NonBlocking channel is full.
    write_non_blocking(&mut non_blocking, b"Universe")?;
    ensure_eq(&error_count.dropped_lines(), &1, "full lossy queue drops one line")?;

    // Finally the second message sent will be consumed.
    let second_line = ensure_ok(receiver.recv(), "receive second lossy line")?;
    ensure(second_line == ", World", "second lossy line contents")?;
    ensure_eq(&error_count.dropped_lines(), &1, "lossy dropped-line count remains one")
  }

  /// Verifies cloned non-blocking writers can be used from multiple threads.
  #[test]
  fn multi_threaded_writes() -> Result<(), TestFailure> {
    let (mock_writer, receiver) = MockWriter::new(DEFAULT_BUFFERED_LINES_LIMIT);

    let (non_blocking, _guard) = NonBlockingBuilder::default().lossy(true).finish(mock_writer);

    let error_count = non_blocking.error_counter();
    let mut join_handles: Vec<JoinHandle<Result<(), TestFailure>>> = Vec::with_capacity(THREAD_COUNT);

    for _thread_index in 0..THREAD_COUNT {
      let writer = NonBlocking::clone(&non_blocking);
      join_handles.push(thread::spawn(move || {
        let subscriber = tracing_subscriber::fmt().with_writer(writer);
        subscriber::with_default(subscriber.finish(), || {
          tracing::event!(tracing::Level::INFO, "Hello");
        });
        Ok(())
      }));
    }

    for handle in join_handles {
      join_test_thread(handle, "event writer thread should not panic")?;
    }

    let mut hello_count = 0_usize;

    while let Ok(event_line) = receiver.recv_timeout(EVENT_RECV_TIMEOUT) {
      ensure(event_line.contains("Hello"), "event line contains message")?;
      hello_count = hello_count.saturating_add(1);
    }

    ensure_eq(&hello_count, &THREAD_COUNT, "all writer threads emit one line")?;
    ensure_eq(&error_count.dropped_lines(), &0, "multi-threaded writes do not drop lines")
  }
}
