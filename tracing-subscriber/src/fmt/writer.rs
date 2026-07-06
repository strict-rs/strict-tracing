//! Abstractions for creating [`io::Write`] instances.
//!
//! [`io::Write`]: std::io::Write

use alloc::{boxed::Box, fmt, sync::Arc};
use std::{
    any::type_name,
    cmp,
    fs::File,
    io::{self, Write},
    str,
};
use tracing_core::Metadata;

/// A type that can create [`io::Write`] instances.
///
/// `MakeWriter` is used by [`fmt::Layer`] or [`fmt::Subscriber`] to print
/// formatted text representations of [`Event`]s.
///
/// This trait is already implemented for function pointers and
/// immutably-borrowing closures that return an instance of [`io::Write`], such
/// as [`io::stdout`] and [`io::stderr`].
///
/// # Examples
///
/// The simplest usage is to pass in a named function that returns a writer. For
/// example, to log all events to stderr, we could write:
/// ```
/// let subscriber = tracing_subscriber::fmt()
///     .with_writer(std::io::stderr)
///     .finish();
/// # drop(subscriber);
/// ```
///
/// Any function that returns a writer can be used:
///
/// ```
/// fn make_my_great_writer() -> impl std::io::Write {
///     // ...
///     # std::io::stdout()
/// }
///
/// let subscriber = tracing_subscriber::fmt()
///     .with_writer(make_my_great_writer)
///     .finish();
/// # drop(subscriber);
/// ```
///
/// A closure can be used to introduce arbitrary logic into how the writer is
/// created. Consider the (admittedly rather silly) example of sending every 5th
/// event to stderr, and all other events to stdout:
///
/// ```
/// use std::io;
/// use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
///
/// let n = AtomicUsize::new(0);
/// let subscriber = tracing_subscriber::fmt()
///     .with_writer(move || -> Box<dyn io::Write> {
///         if n.fetch_add(1, Relaxed) % 5 == 0 {
///             Box::new(io::stderr())
///         } else {
///             Box::new(io::stdout())
///        }
///     })
///     .finish();
/// # drop(subscriber);
/// ```
///
/// A single instance of a type implementing [`io::Write`] may be used as a
/// `MakeWriter` by wrapping it in an [`Arc`]. For example, we could
/// write to a file like so:
///
/// ```
/// use std::{fs::File, sync::Arc};
///
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
/// let log_file = Arc::new(File::create("my_cool_trace.log")?);
/// let subscriber = tracing_subscriber::fmt()
///     .with_writer(Arc::clone(&log_file))
///     .finish();
/// # drop(subscriber);
/// # Ok(())
/// # }
/// ```
///
/// [`io::Write`]: std::io::Write
/// [`Arc`]: std::sync::Arc
/// [`fmt::Layer`]: crate::fmt::Layer
/// [`fmt::Subscriber`]: crate::fmt::Subscriber
/// [`Event`]: tracing_core::event::Event
/// [`io::stdout`]: std::io::stdout()
/// [`io::stderr`]: std::io::stderr()
/// [`MakeWriter::make_writer_for`]: MakeWriter::make_writer_for
/// [`Metadata`]: tracing_core::Metadata
/// [levels]: tracing_core::Level
/// [targets]: tracing_core::Metadata::target
pub trait MakeWriter<'a> {
    /// The concrete [`io::Write`] implementation returned by [`make_writer`].
    ///
    /// [`io::Write`]: std::io::Write
    /// [`make_writer`]: MakeWriter::make_writer
    type Writer: Write;

    /// Returns an instance of [`Writer`].
    ///
    /// # Implementer notes
    ///
    /// [`fmt::Layer`] or [`fmt::Subscriber`] will call this method each time an event is recorded. Ensure any state
    /// that must be saved across writes is not lost when the [`Writer`] instance is dropped. If
    /// creating a [`io::Write`] instance is expensive, be sure to cache it when implementing
    /// [`MakeWriter`] to improve performance.
    ///
    /// [`Writer`]: MakeWriter::Writer
    /// [`fmt::Layer`]: crate::fmt::Layer
    /// [`fmt::Subscriber`]: crate::fmt::Subscriber
    /// [`io::Write`]: std::io::Write
    fn make_writer(&'a self) -> Self::Writer;

    /// Returns a [`Writer`] for writing data from the span or event described
    /// by the provided [`Metadata`].
    ///
    /// By default, this calls [`self.make_writer()`][make_writer], ignoring
    /// the provided metadata, but implementations can override this to provide
    /// metadata-specific behaviors.
    ///
    /// This method allows `MakeWriter` implementations to implement different
    /// behaviors based on the span or event being written. The `MakeWriter`
    /// type might return different writers based on the provided metadata, or
    /// might write some values to the writer before or after providing it to
    /// the caller.
    ///
    /// For example, we might want to write data from spans and events at the
    /// [`ERROR`] and [`WARN`] levels to `stderr`, and data from spans or events
    /// at lower levels to stdout:
    ///
    /// ```
    /// use std::io::{self, Stdout, Stderr, StdoutLock, StderrLock};
    /// use tracing_subscriber::fmt::writer::MakeWriter;
    /// use tracing_core::{Metadata, Level};
    ///
    /// pub struct MyMakeWriter {
    ///     stdout: Stdout,
    ///     stderr: Stderr,
    /// }
    ///
    /// /// A lock on either stdout or stderr, depending on the verbosity level
    /// /// of the event being written.
    /// pub enum StdioLock<'a> {
    ///     Stdout(StdoutLock<'a>),
    ///     Stderr(StderrLock<'a>),
    /// }
    ///
    /// impl<'a> io::Write for StdioLock<'a> {
    ///     fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    ///         match self {
    ///             StdioLock::Stdout(lock) => lock.write(buf),
    ///             StdioLock::Stderr(lock) => lock.write(buf),
    ///         }
    ///     }
    ///
    ///     fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
    ///         // ...
    ///         # match self {
    ///         #     StdioLock::Stdout(lock) => lock.write_all(buf),
    ///         #     StdioLock::Stderr(lock) => lock.write_all(buf),
    ///         # }
    ///     }
    ///
    ///     fn flush(&mut self) -> io::Result<()> {
    ///         // ...
    ///         # match self {
    ///         #     StdioLock::Stdout(lock) => lock.flush(),
    ///         #     StdioLock::Stderr(lock) => lock.flush(),
    ///         # }
    ///     }
    /// }
    ///
    /// impl<'a> MakeWriter<'a> for MyMakeWriter {
    ///     type Writer = StdioLock<'a>;
    ///
    ///     fn make_writer(&'a self) -> Self::Writer {
    ///         // We must have an implementation of `make_writer` that makes
    ///         // a "default" writer without any configuring metadata. Let's
    ///         // just return stdout in that case.
    ///         StdioLock::Stdout(self.stdout.lock())
    ///     }
    ///
    ///     fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
    ///         // Here's where we can implement our special behavior. We'll
    ///         // check if the metadata's verbosity level is WARN or ERROR,
    ///         // and return stderr in that case.
    ///         if meta.level() <= &Level::WARN {
    ///             return StdioLock::Stderr(self.stderr.lock());
    ///         }
    ///
    ///         // Otherwise, we'll return stdout.
    ///         StdioLock::Stdout(self.stdout.lock())
    ///     }
    /// }
    /// ```
    ///
    /// [`Writer`]: MakeWriter::Writer
    /// [`Metadata`]: tracing_core::Metadata
    /// [make_writer]: MakeWriter::make_writer
    /// [`WARN`]: tracing_core::Level::WARN
    /// [`ERROR`]: tracing_core::Level::ERROR
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        let _: &Metadata<'_> = meta;
        self.make_writer()
    }
}

/// Extension trait adding combinators for working with types implementing
/// [`MakeWriter`].
///
/// This is not intended to be implemented directly for user-defined
/// [`MakeWriter`]s; instead, it should be imported when the desired methods are
/// used.
pub trait MakeWriterExt<'a>: MakeWriter<'a> {
    /// Wraps `self` and returns a [`MakeWriter`] that will only write output
    /// for events at or below the provided verbosity [`Level`]. For instance,
    /// `Level::TRACE` is considered to be more verbose than `Level::INFO`.
    ///
    /// Events whose level is more verbose than `level` will be ignored, and no
    /// output will be written.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing::Level;
    /// use tracing_subscriber::fmt::writer::MakeWriterExt;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// // Construct a writer that outputs events to `stderr` only if the span or
    /// // event's level is <= WARN (WARN and ERROR).
    /// let mk_writer = std::io::stderr.with_max_level(Level::WARN);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    ///
    /// Writing the `ERROR` and `WARN` levels to `stderr`, and everything else
    /// to `stdout`:
    ///
    /// ```
    /// # use tracing::Level;
    /// # use tracing_subscriber::fmt::writer::MakeWriterExt;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// let mk_writer = std::io::stderr
    ///     .with_max_level(Level::WARN)
    ///     .or_else(std::io::stdout);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    ///
    /// Writing the `ERROR` level to `stderr`, the `INFO` and `WARN` levels to
    /// `stdout`, and the `INFO` and `DEBUG` levels to a file:
    ///
    /// ```
    /// # use tracing::Level;
    /// # use tracing_subscriber::fmt::writer::MakeWriterExt;
    /// use std::{sync::Arc, fs::File};
    /// # // don't actually create the file when running the tests.
    /// # fn docs() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// let debug_log = Arc::new(File::create("debug.log")?);
    ///
    /// let mk_writer = std::io::stderr
    ///     .with_max_level(Level::ERROR)
    ///     .or_else(std::io::stdout
    ///         .with_max_level(Level::INFO)
    ///         .and(debug_log.with_max_level(Level::DEBUG))
    ///     );
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    ///
    /// [`Level`]: tracing_core::Level
    /// [`io::Write`]: std::io::Write
    fn with_max_level(self, level: tracing_core::Level) -> WithMaxLevel<Self>
    where
        Self: Sized,
    {
        WithMaxLevel::new(self, level)
    }

    /// Wraps `self` and returns a [`MakeWriter`] that will only write output
    /// for events at or above the provided verbosity [`Level`].
    ///
    /// Events whose level is less verbose than `level` will be ignored, and no
    /// output will be written.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing::Level;
    /// use tracing_subscriber::fmt::writer::MakeWriterExt;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// // Construct a writer that outputs events to `stdout` only if the span or
    /// // event's level is >= DEBUG (DEBUG and TRACE).
    /// let mk_writer = std::io::stdout.with_min_level(Level::DEBUG);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    /// This can be combined with [`MakeWriterExt::with_max_level`] to write
    /// only within a range of levels:
    ///
    /// ```
    /// # use tracing::Level;
    /// # use tracing_subscriber::fmt::writer::MakeWriterExt;
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// // Only write the `DEBUG` and `INFO` levels to stdout.
    /// let mk_writer = std::io::stdout
    ///     .with_max_level(Level::DEBUG)
    ///     .with_min_level(Level::INFO)
    ///     // Write the `WARN` and `ERROR` levels to stderr.
    ///     .and(std::io::stderr.with_min_level(Level::WARN));
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    /// [`Level`]: tracing_core::Level
    /// [`io::Write`]: std::io::Write
    fn with_min_level(self, level: tracing_core::Level) -> WithMinLevel<Self>
    where
        Self: Sized,
    {
        WithMinLevel::new(self, level)
    }

    /// Wraps `self` with a predicate that takes a span or event's [`Metadata`]
    /// and returns a `bool`. The returned [`MakeWriter`]'s
    /// [`MakeWriter::make_writer_for`] method will check the predicate to
    /// determine if  a writer should be produced for a given span or event.
    ///
    /// If the predicate returns `false`, the wrapped [`MakeWriter`]'s
    /// [`make_writer_for`][mwf] will return [`OptionalWriter::none`][own].
    /// Otherwise, it calls the wrapped [`MakeWriter`]'s
    /// [`make_writer_for`][mwf] method, and returns the produced writer.
    ///
    /// This can be used to filter an output based on arbitrary [`Metadata`]
    /// parameters.
    ///
    /// # Examples
    ///
    /// Writing events with a specific target to an HTTP access log, and other
    /// events to stdout:
    ///
    /// ```
    /// use tracing_subscriber::fmt::writer::MakeWriterExt;
    /// use std::{sync::Arc, fs::File};
    /// # // don't actually create the file when running the tests.
    /// # fn docs() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// let access_log = Arc::new(File::create("access.log")?);
    ///
    /// let mk_writer = access_log
    ///     // Only write events with the target "http::access_log" to the
    ///     // access log file.
    ///     .with_filter(|meta| meta.target() == "http::access_log")
    ///     // Write events with all other targets to stdout.
    ///     .or_else(std::io::stdout);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Conditionally enabling or disabling a log file:
    /// ```
    /// use tracing_subscriber::fmt::writer::MakeWriterExt;
    /// use std::{
    ///     sync::{Arc, atomic::{AtomicBool, Ordering}},
    ///     fs::File,
    /// };
    ///
    /// static DEBUG_LOG_ENABLED: AtomicBool = AtomicBool::new(false);
    ///
    /// # // don't actually create the file when running the tests.
    /// # fn docs() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// // Create the debug log file
    /// let debug_file = Arc::new(File::create("debug.log")?)
    ///     // Enable the debug log only if the flag is enabled.
    ///     .with_filter(|_| DEBUG_LOG_ENABLED.load(Ordering::Acquire));
    ///
    /// // Always write to stdout
    /// let mk_writer = std::io::stdout
    ///     // Write to the debug file if it's enabled
    ///     .and(debug_file);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    ///
    /// // ...
    ///
    /// // Later, we can toggle on or off the debug log file.
    /// DEBUG_LOG_ENABLED.store(true, Ordering::Release);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Metadata`]: tracing_core::Metadata
    /// [mwf]: MakeWriter::make_writer_for
    /// [own]: EitherWriter::none
    fn with_filter<F>(self, filter: F) -> WithFilter<Self, F>
    where
        Self: Sized,
        F: Fn(&Metadata<'_>) -> bool,
    {
        WithFilter::new(self, filter)
    }

    /// Combines `self` with another type implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that produces [writers] that write to *both*
    /// outputs.
    ///
    /// If writing to either writer returns an error, the returned writer will
    /// return that error. However, both writers will still be written to before
    /// the error is returned, so it is possible for one writer to fail while
    /// the other is written to successfully.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::fmt::writer::MakeWriterExt;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// // Construct a writer that outputs events to `stdout` *and* `stderr`.
    /// let mk_writer = std::io::stdout.and(std::io::stderr);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    ///
    /// `and` can be used in conjunction with filtering combinators. For
    /// example, if we want to write to a number of outputs depending on the
    /// level of an event, we could write:
    ///
    /// ```
    /// use tracing::Level;
    /// # use tracing_subscriber::fmt::writer::MakeWriterExt;
    /// use std::{sync::Arc, fs::File};
    /// # // don't actually create the file when running the tests.
    /// # fn docs() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// let debug_log = Arc::new(File::create("debug.log")?);
    ///
    /// // Write everything to the debug log.
    /// let mk_writer = debug_log
    ///     // Write the `ERROR` and `WARN` levels to stderr.
    ///     .and(std::io::stderr.with_max_level(Level::WARN))
    ///     // Write `INFO` to `stdout`.
    ///     .and(std::io::stdout
    ///         .with_max_level(Level::INFO)
    ///         .with_min_level(Level::INFO)
    ///     );
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    ///
    /// [writers]: std::io::Write
    fn and<B>(self, other: B) -> Tee<Self, B>
    where
        Self: Sized,
        B: MakeWriter<'a> + Sized,
    {
        Tee::new(self, other)
    }

    /// Combines `self` with another type implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that calls `other`'s [`make_writer`] if `self`'s
    /// `make_writer` returns [`OptionalWriter::none`][own].
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing::Level;
    /// use tracing_subscriber::fmt::writer::MakeWriterExt;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// // Produces a writer that writes to `stderr` if the level is <= WARN,
    /// // or returns `OptionalWriter::none()` otherwise.
    /// let stderr = std::io::stderr.with_max_level(Level::WARN);
    ///
    /// // If the `stderr` `MakeWriter` is disabled by the max level filter,
    /// // write to stdout instead:
    /// let mk_writer = stderr.or_else(std::io::stdout);
    ///
    /// tracing_subscriber::fmt().with_writer(mk_writer).try_init()?;
    /// # Ok(()) }
    /// ```
    ///
    /// [`make_writer`]: MakeWriter::make_writer
    /// [own]: EitherWriter::none
    fn or_else<W, B>(self, other: B) -> OrElse<Self, B>
    where
        Self: MakeWriter<'a, Writer = OptionalWriter<W>> + Sized,
        B: MakeWriter<'a> + Sized,
        W: Write,
    {
        OrElse::new(self, other)
    }
}

/// A writer intended for use in unit tests.
///
/// `TestWriter` is used by [`fmt::Subscriber`] or [`fmt::Layer`] to write
/// formatted output to the process standard streams during tests.
///
/// See [`libtest`'s output capturing][capturing] and
/// [rust-lang/rust#90785](https://github.com/rust-lang/rust/issues/90785)
/// for more details about output capturing.
///
/// Writing to [`io::stdout`] and [`io::stderr`] produces the same results as using
/// [`libtest`'s `--nocapture` option][nocapture] which may make the results look unreadable.
///
/// [`fmt::Subscriber`]: super::Subscriber
/// [`fmt::Layer`]: super::Layer
/// [capturing]: https://doc.rust-lang.org/book/ch11-02-running-tests.html#showing-function-output
/// [nocapture]: https://doc.rust-lang.org/cargo/commands/cargo-test.html
/// [`io::stdout`]: std::io::stdout
/// [`io::stderr`]: std::io::stderr
#[derive(Copy, Clone, Default, Debug)]
pub struct TestWriter {
    /// Whether or not to use `stderr` instead of the default `stdout` as
    /// the underlying stream to write to.
    use_stderr: bool,
}

/// Boxed writer returned by a type-erased writer factory.
type BoxedDynWrite<'a> = Box<dyn Write + 'a>;

/// Type-erased writer factory stored by [`BoxMakeWriter`].
type BoxedMakeWriter = Box<dyn for<'a> MakeWriter<'a, Writer = BoxedDynWrite<'a>> + Send + Sync>;

/// A writer that erases the specific [`io::Write`] and [`MakeWriter`] types being used.
///
/// This is useful in cases where the concrete type of the writer cannot be known
/// until runtime.
///
/// # Examples
///
/// A function that returns a [`Subscriber`] that will write to either stdout or stderr:
///
/// ```rust
/// # use tracing::Subscriber;
/// # use tracing_subscriber::fmt::writer::BoxMakeWriter;
///
/// fn dynamic_writer(use_stderr: bool) -> impl Subscriber {
///     let writer = if use_stderr {
///         BoxMakeWriter::new(std::io::stderr)
///     } else {
///         BoxMakeWriter::new(std::io::stdout)
///     };
///
///     tracing_subscriber::fmt().with_writer(writer).finish()
/// }
/// ```
///
/// [`Subscriber`]: tracing::Subscriber
/// [`io::Write`]: std::io::Write
pub struct BoxMakeWriter {
    /// The erased writer factory.
    inner: BoxedMakeWriter,
    /// The erased writer factory's type name.
    name: &'static str,
}

/// A [writer] that is one of two types implementing [`io::Write`].
///
/// This may be used by [`MakeWriter`] implementations that may conditionally
/// return one of two writers.
///
/// [writer]: std::io::Write
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EitherWriter<A, B> {
    /// The first writer type.
    First(A),
    /// The second writer type.
    Second(B),
}

/// A [writer] which may or may not be enabled.
///
/// This may be used by [`MakeWriter`] implementations that wish to
/// conditionally enable or disable the returned writer based on a span or
/// event's [`Metadata`].
///
/// [writer]: std::io::Write
pub type OptionalWriter<T> = EitherWriter<T, io::Sink>;

/// A [`MakeWriter`] combinator that only returns an enabled [writer] for spans
/// and events with metadata at or below a specified verbosity [`Level`].
///
/// This is returned by the [`MakeWriterExt::with_max_level`] method. See the
/// method documentation for details.
///
/// [writer]: std::io::Write
/// [`Level`]: tracing_core::Level
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithMaxLevel<M> {
    /// The wrapped writer factory.
    make: M,
    /// The least verbose level emitted by this writer.
    level: tracing_core::Level,
}

/// A [`MakeWriter`] combinator that only returns an enabled [writer] for spans
/// and events with metadata at or above a specified verbosity [`Level`].
///
/// This is returned by the [`MakeWriterExt::with_min_level`] method. See the
/// method documentation for details.
///
/// [writer]: std::io::Write
/// [`Level`]: tracing_core::Level
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithMinLevel<M> {
    /// The wrapped writer factory.
    make: M,
    /// The most verbose level emitted by this writer.
    level: tracing_core::Level,
}

/// A [`MakeWriter`] combinator that wraps a [`MakeWriter`] with a metadata predicate.
///
/// For span and event [`Metadata`], [`MakeWriter::make_writer_for`] returns
/// [`OptionalWriter::some`][ows] when the predicate returns `true`, and
/// [`OptionalWriter::none`][own] when the predicate returns `false`.
///
/// This is returned by the [`MakeWriterExt::with_filter`] method. See the
/// method documentation for details.
///
/// [`Metadata`]: tracing_core::Metadata
/// [ows]: EitherWriter::some
/// [own]: EitherWriter::none
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithFilter<M, F> {
    /// The wrapped writer factory.
    make: M,
    /// The metadata predicate that enables the wrapped writer.
    filter: F,
}

/// Combines an optional [`MakeWriter`] with a fallback [`MakeWriter`].
///
/// The second [`MakeWriter`] is used when the first [`MakeWriter`] returns
/// [`OptionalWriter::none`][own].
///
/// This is returned by the [`MakeWriterExt::or_else`] method. See the
/// method documentation for details.
///
/// [own]: EitherWriter::none
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OrElse<A, B> {
    /// The primary writer factory.
    inner: A,
    /// The fallback writer factory.
    or_else: B,
}

/// Combines two types implementing [`MakeWriter`] (or [`std::io::Write`]) to
/// produce a writer that writes to both [`MakeWriter`]'s returned writers.
///
/// This is returned by the [`MakeWriterExt::and`] method. See the method
/// documentation for details.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Tee<A, B> {
    /// The first writer factory.
    first: A,
    /// The second writer factory.
    second: B,
}

/// Implements [`std::io::Write`] for an [`Arc`]<W> where `&W: Write`.
///
/// This is an implementation detail of the [`MakeWriter`] impl for [`Arc`].
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ArcWriter<W>(Arc<W>);

/// A bridge between `fmt::Write` and `io::Write`.
///
/// This is used by the timestamp formatting implementation for the `time`
/// crate and by the JSON formatter. In both cases, this is needed because
/// `tracing-subscriber`'s `FormatEvent`/`FormatTime` traits expect a
/// `fmt::Write` implementation, while `serde_json::Serializer` and `time`'s
/// `format_into` methods expect an `io::Write`.
#[cfg(any(feature = "json", feature = "time"))]
pub(in crate::fmt) struct WriteAdaptor<'a> {
    /// The formatter-backed writer that receives decoded UTF-8.
    fmt_write: &'a mut dyn fmt::Write,
}

impl<'a, F, W> MakeWriter<'a> for F
where
    F: Fn() -> W,
    W: Write,
{
    type Writer = W;

    fn make_writer(&'a self) -> Self::Writer {
        (self)()
    }
}

impl<'a, W> MakeWriter<'a> for Arc<W>
where
    &'a W: Write + 'a,
{
    type Writer = &'a W;
    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}

impl<'a> MakeWriter<'a> for File {
    type Writer = &'a Self;
    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}

// === impl TestWriter ===

impl TestWriter {
    /// Returns a new `TestWriter` with the default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a new `TestWriter` that writes to `stderr` instead of `stdout`.
    #[must_use]
    pub const fn with_stderr() -> Self {
        Self { use_stderr: true }
    }
}

impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.use_stderr {
            io::stderr().write_all(buf)?;
        } else {
            io::stdout().write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TestWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        *self
    }
}

// === impl BoxMakeWriter ===

impl BoxMakeWriter {
    /// Constructs a `BoxMakeWriter` wrapping a type implementing [`MakeWriter`].
    ///
    pub fn new<M>(make_writer: M) -> Self
    where
        M: for<'a> MakeWriter<'a> + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(Boxed(make_writer)),
            name: type_name::<M>(),
        }
    }
}

impl fmt::Debug for BoxMakeWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BoxMakeWriter")
            .field(&format_args!("<{}>", self.name))
            .finish()
    }
}

impl<'a> MakeWriter<'a> for BoxMakeWriter {
    type Writer = Box<dyn Write + 'a>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        self.inner.make_writer()
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        self.inner.make_writer_for(meta)
    }
}

/// Erases the concrete writer type produced by a [`MakeWriter`].
struct Boxed<M>(M);

impl<'a, M> MakeWriter<'a> for Boxed<M>
where
    M: MakeWriter<'a>,
{
    type Writer = Box<dyn Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        let writer = self.0.make_writer();
        Box::new(writer)
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        let writer = self.0.make_writer_for(meta);
        Box::new(writer)
    }
}

// === impl EitherWriter ===

impl<A, B> Write for EitherWriter<A, B>
where
    A: Write,
    B: Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match *self {
            Self::First(ref mut writer) => writer.write(buf),
            Self::Second(ref mut writer) => writer.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match *self {
            Self::First(ref mut writer) => writer.flush(),
            Self::Second(ref mut writer) => writer.flush(),
        }
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        match *self {
            Self::First(ref mut writer) => writer.write_vectored(bufs),
            Self::Second(ref mut writer) => writer.write_vectored(bufs),
        }
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match *self {
            Self::First(ref mut writer) => writer.write_all(buf),
            Self::Second(ref mut writer) => writer.write_all(buf),
        }
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        match *self {
            Self::First(ref mut writer) => writer.write_fmt(fmt),
            Self::Second(ref mut writer) => writer.write_fmt(fmt),
        }
    }
}

impl<T> OptionalWriter<T> {
    /// Returns a [disabled writer].
    ///
    /// Any bytes written to the returned writer are discarded.
    ///
    /// This is equivalent to returning [`Option::None`].
    ///
    /// [disabled writer]: std::io::sink
    #[inline]
    #[must_use]
    pub const fn none() -> Self {
        Self::Second(io::sink())
    }

    /// Returns an enabled writer of type `T`.
    ///
    /// This is equivalent to returning [`Option::Some`].
    #[inline]
    pub const fn some(writer: T) -> Self {
        Self::First(writer)
    }
}

impl<T> From<Option<T>> for OptionalWriter<T> {
    #[inline]
    fn from(opt: Option<T>) -> Self {
        opt.map_or_else(Self::none, Self::some)
    }
}

// === impl WithMaxLevel ===

impl<M> WithMaxLevel<M> {
    /// Wraps the provided [`MakeWriter`] with a maximum [`Level`], so that it
    /// returns [`OptionalWriter::none`] for spans and events whose level is
    /// more verbose than the maximum level.
    ///
    /// See [`MakeWriterExt::with_max_level`] for details.
    ///
    /// [`Level`]: tracing_core::Level
    #[allow(
        clippy::single_call_fn,
        reason = "public writer-adapter constructor is part of the `MakeWriterExt` API"
    )]
    pub const fn new(make: M, level: tracing_core::Level) -> Self {
        Self { make, level }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for WithMaxLevel<M> {
    type Writer = OptionalWriter<M::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        // If we don't know the level, assume it's disabled.
        OptionalWriter::none()
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if meta.level() <= &self.level {
            return OptionalWriter::some(self.make.make_writer_for(meta));
        }
        OptionalWriter::none()
    }
}

// === impl WithMinLevel ===

impl<M> WithMinLevel<M> {
    /// Wraps the provided [`MakeWriter`] with a minimum [`Level`], so that it
    /// returns [`OptionalWriter::none`] for spans and events whose level is
    /// less verbose than the maximum level.
    ///
    /// See [`MakeWriterExt::with_min_level`] for details.
    ///
    /// [`Level`]: tracing_core::Level
    #[allow(
        clippy::single_call_fn,
        reason = "public writer-adapter constructor is part of the `MakeWriterExt` API"
    )]
    pub const fn new(make: M, level: tracing_core::Level) -> Self {
        Self { make, level }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for WithMinLevel<M> {
    type Writer = OptionalWriter<M::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        // If we don't know the level, assume it's disabled.
        OptionalWriter::none()
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if meta.level() >= &self.level {
            return OptionalWriter::some(self.make.make_writer_for(meta));
        }
        OptionalWriter::none()
    }
}

// ==== impl WithFilter ===

impl<M, F> WithFilter<M, F> {
    /// Wraps `make` with the provided `filter`, returning a [`MakeWriter`] that
    /// will call `make.make_writer_for()` when `filter` returns `true` for a
    /// span or event's [`Metadata`], and returns a [`sink`] otherwise.
    ///
    /// See [`MakeWriterExt::with_filter`] for details.
    ///
    /// [`Metadata`]: tracing_core::Metadata
    /// [`sink`]: std::io::sink
    #[allow(
        clippy::single_call_fn,
        reason = "public writer-adapter constructor is part of the `MakeWriterExt` API"
    )]
    pub const fn new(make: M, filter: F) -> Self
    where
        F: Fn(&Metadata<'_>) -> bool,
    {
        Self { make, filter }
    }
}

impl<'a, M, F> MakeWriter<'a> for WithFilter<M, F>
where
    M: MakeWriter<'a>,
    F: Fn(&Metadata<'_>) -> bool,
{
    type Writer = OptionalWriter<M::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        OptionalWriter::some(self.make.make_writer())
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if (self.filter)(meta) {
            OptionalWriter::some(self.make.make_writer_for(meta))
        } else {
            OptionalWriter::none()
        }
    }
}

// === impl Tee ===

impl<A, B> Tee<A, B> {
    /// Combines two types implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that produces [writers] that write to *both*
    /// outputs.
    ///
    /// See the documentation for [`MakeWriterExt::and`] for details.
    ///
    /// [writers]: std::io::Write
    pub const fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
}

impl<'a, A, B> MakeWriter<'a> for Tee<A, B>
where
    A: MakeWriter<'a>,
    B: MakeWriter<'a>,
{
    type Writer = Tee<A::Writer, B::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        Tee::new(self.first.make_writer(), self.second.make_writer())
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        Tee::new(
            self.first.make_writer_for(meta),
            self.second.make_writer_for(meta),
        )
    }
}

/// Calls the same [`Write`] method on both tee writers.
macro_rules! impl_tee {
    ($self_:ident.$f:ident($($arg:ident),*)) => {
        {
            let first_result = $self_.first.$f($($arg),*);
            let second_result = $self_.second.$f($($arg),*);
            (first_result?, second_result?)
        }
    }
}

impl<A, B> Write for Tee<A, B>
where
    A: Write,
    B: Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (first_written, second_written) = impl_tee!(self.write(buf));
        Ok(cmp::max(first_written, second_written))
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        impl_tee!(self.flush());
        Ok(())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        let (first_written, second_written) = impl_tee!(self.write_vectored(bufs));
        Ok(cmp::max(first_written, second_written))
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        impl_tee!(self.write_all(buf));
        Ok(())
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        impl_tee!(self.write_fmt(fmt));
        Ok(())
    }
}

// === impl OrElse ===

impl<A, B> OrElse<A, B> {
    /// Combines
    #[allow(
        clippy::single_call_fn,
        reason = "public writer-adapter constructor is part of the `MakeWriterExt` API"
    )]
    pub const fn new<'a, W>(inner: A, or_else: B) -> Self
    where
        A: MakeWriter<'a, Writer = OptionalWriter<W>>,
        B: MakeWriter<'a>,
        W: Write,
    {
        Self { inner, or_else }
    }
}

impl<'a, A, B, W> MakeWriter<'a> for OrElse<A, B>
where
    A: MakeWriter<'a, Writer = OptionalWriter<W>>,
    B: MakeWriter<'a>,
    W: Write,
{
    type Writer = EitherWriter<W, B::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        match self.inner.make_writer() {
            EitherWriter::First(writer) => EitherWriter::First(writer),
            EitherWriter::Second(_) => EitherWriter::Second(self.or_else.make_writer()),
        }
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        match self.inner.make_writer_for(meta) {
            EitherWriter::First(writer) => EitherWriter::First(writer),
            EitherWriter::Second(_) => EitherWriter::Second(self.or_else.make_writer_for(meta)),
        }
    }
}

// === impl ArcWriter ===

impl<W> Write for ArcWriter<W>
where
    for<'a> &'a W: Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        (&*self.0).write_vectored(bufs)
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        (&*self.0).write_all(buf)
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        (&*self.0).write_fmt(fmt)
    }
}

// === impl WriteAdaptor ===

#[cfg(any(feature = "json", feature = "time"))]
impl<'a> WriteAdaptor<'a> {
    /// Returns an adapter that forwards UTF-8 bytes to a [`fmt::Write`] value.
    pub(in crate::fmt) fn new(fmt_write: &'a mut dyn fmt::Write) -> Self {
        Self { fmt_write }
    }
}
#[cfg(any(feature = "json", feature = "time"))]
impl Write for WriteAdaptor<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = str::from_utf8(buf).map_err(|utf8_error| {
            let _: str::Utf8Error = utf8_error;
            io::Error::from(io::ErrorKind::InvalidData)
        })?;

        self.fmt_write
            .write_str(text)
            .map_err(io_error_from_fmt)?;

        Ok(text.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(any(feature = "json", feature = "time"))]
/// Converts a [`fmt::Error`] from the adapted formatter to an [`io::Error`].
#[allow(
    clippy::single_call_fn,
    reason = "formatter writer adapter centralizes the `fmt::Error` to `io::Error` conversion"
)]
fn io_error_from_fmt(fmt_error: fmt::Error) -> io::Error {
    let _: fmt::Error = fmt_error;
    io::Error::from(io::ErrorKind::Other)
}

#[cfg(any(feature = "json", feature = "time"))]
impl fmt::Debug for WriteAdaptor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("WriteAdaptor { .. }")
    }
}
// === blanket impls ===

impl<'a, M> MakeWriterExt<'a> for M where M: MakeWriter<'a> {}
#[cfg(test)]
mod test {
    use super::*;
    use crate::fmt::format::Format;
    use crate::fmt::test::{MockMakeWriter, MockWriter};
    use crate::fmt::Subscriber;
    use alloc::{string::String, vec::Vec};
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::{format, sync::Arc};
    use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok, ensure_some};
    use tracing::{debug, error, info, trace, warn, Level, subscriber};
    use tracing_core::callsite::Callsite;
    use tracing_core::metadata::Kind;
    use tracing_core::subscriber::Interest;

    /// Shared byte buffer used by writer tests.
    type SharedBuffer = Arc<Mutex<Vec<u8>>>;

    /// Callsite used by writer metadata fixtures.
    struct WriterTestCallsite;

    /// Shared callsite for writer metadata fixtures.
    static WRITER_CALLSITE: WriterTestCallsite = WriterTestCallsite;

    impl Callsite for WriterTestCallsite {
        fn set_interest(&self, _: Interest) {}

        fn metadata(&self) -> &Metadata<'_> {
            static META: Metadata<'static> = tracing_core::metadata! {
                name: "writer_test",
                target: "writer_target",
                level: Level::INFO,
                fields: &[],
                callsite: &WRITER_CALLSITE,
                kind: Kind::EVENT,
            };
            &META
        }
    }

    /// Creates a shared byte buffer paired with a [`MockMakeWriter`] that appends to it.
    fn writer_buffer() -> (SharedBuffer, MockMakeWriter) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = MockMakeWriter::new(Arc::clone(&buf));
        (buf, writer)
    }

    /// Installs a time-free fmt subscriber writing through `make_writer`, returning the default guard.
    fn install_writer<W>(make_writer: W) -> subscriber::DefaultGuard
    where
        W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
    {
        #[cfg(feature = "ansi")]
        let format = Format::default().without_time().with_ansi(false);
        #[cfg(not(feature = "ansi"))]
        let format = Format::default().without_time();
        let subscriber = Subscriber::builder()
            .event_format(format)
            .with_writer(make_writer)
            .with_max_level(Level::TRACE)
            .finish();
        subscriber::set_default(subscriber)
    }

    fn test_writer<T>(make_writer: T, msg: &str, buf: &Mutex<Vec<u8>>) -> Result<(), TestFailure>
    where
        T: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
    {
        let _guard = install_writer(make_writer);
        error!("{}", msg);

        let expected = format!("ERROR {}: {}\n", module_path!(), msg);
        let actual = String::from_utf8_lossy(&buf.lock()).into_owned();
        ensure(
            actual.contains(expected.as_str()),
            "custom writer output contains expected line",
        )
    }

    fn has_lines(buf: &Mutex<Vec<u8>>, msgs: &[(Level, &str)]) -> Result<(), TestFailure> {
        let actual = String::from_utf8_lossy(&buf.lock()).into_owned();
        let mut expected_lines = msgs.iter();
        for actual_line in actual.lines() {
            let line = actual_line.trim();
            let &(level, msg) = ensure_some(
                expected_lines.next(),
                "writer emitted no more lines than expected",
            )?;
            let expected = format!("{} {}: {}", level, module_path!(), msg);
            ensure(line == expected.as_str(), "writer emitted expected line")?;
        }
        ensure(
            expected_lines.next().is_none(),
            "writer emitted all expected lines",
        )
    }

    #[test]
    fn custom_writer_closure() -> Result<(), TestFailure> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer_buf = Arc::clone(&buf);
        let make_writer = move || MockWriter::new(Arc::clone(&writer_buf));
        let msg = "my custom writer closure error";
        test_writer(make_writer, msg, &buf)
    }

    #[test]
    fn custom_writer_struct() -> Result<(), TestFailure> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let make_writer = MockMakeWriter::new(Arc::clone(&buf));
        let msg = "my custom writer struct error";
        test_writer(make_writer, msg, &buf)
    }

    #[test]
    fn combinators_level_filters() -> Result<(), TestFailure> {
        let (info_buf, info) = writer_buffer();
        let (debug_buf, debug) = writer_buffer();
        let (warn_buf, warn) = writer_buffer();
        let (err_buf, err) = writer_buffer();

        let make_writer = info
            .with_max_level(Level::INFO)
            .and(debug.with_max_level(Level::DEBUG))
            .and(warn.with_max_level(Level::WARN))
            .and(err.with_max_level(Level::ERROR));

        let _guard = install_writer(make_writer);

        trace!("trace");
        debug!("debug");
        info!("info");
        warn!("warn");
        error!("error");

        let all_lines = [
            (Level::TRACE, "trace"),
            (Level::DEBUG, "debug"),
            (Level::INFO, "info"),
            (Level::WARN, "warn"),
            (Level::ERROR, "error"),
        ];

        has_lines(&debug_buf, &all_lines[1..])?;

        has_lines(&info_buf, &all_lines[2..])?;

        has_lines(&warn_buf, &all_lines[3..])?;

        has_lines(&err_buf, &all_lines[4..])
    }

    #[test]
    fn combinators_or_else() -> Result<(), TestFailure> {
        let (some_buf, some) = writer_buffer();
        let (or_else_buf, or_else) = writer_buffer();

        let return_some = AtomicBool::new(true);
        let optional_writer = move || {
            if return_some.swap(false, Ordering::Relaxed) {
                OptionalWriter::some(some.make_writer())
            } else {
                OptionalWriter::none()
            }
        };
        let make_writer = optional_writer.or_else(or_else);
        let _guard = install_writer(make_writer);
        info!("hello");
        info!("world");
        info!("goodbye");

        has_lines(&some_buf, &[(Level::INFO, "hello")])?;
        has_lines(
            &or_else_buf,
            &[(Level::INFO, "world"), (Level::INFO, "goodbye")],
        )
    }

    #[test]
    fn combinators_or_else_chain() -> Result<(), TestFailure> {
        let (info_buf, info) = writer_buffer();
        let (debug_buf, debug) = writer_buffer();
        let (warn_buf, warn) = writer_buffer();
        let (err_buf, err) = writer_buffer();

        let make_writer = err.with_max_level(Level::ERROR).or_else(
            warn.with_max_level(Level::WARN).or_else(
                info.with_max_level(Level::INFO)
                    .or_else(debug.with_max_level(Level::DEBUG)),
            ),
        );

        let _guard = install_writer(make_writer);

        trace!("trace");
        debug!("debug");
        info!("info");
        warn!("warn");
        error!("error");

        has_lines(&debug_buf, &[(Level::DEBUG, "debug")])?;

        has_lines(&info_buf, &[(Level::INFO, "info")])?;

        has_lines(&warn_buf, &[(Level::WARN, "warn")])?;

        has_lines(&err_buf, &[(Level::ERROR, "error")])
    }

    #[test]
    fn combinators_and() -> Result<(), TestFailure> {
        let (first_buf, first) = writer_buffer();
        let (second_buf, second) = writer_buffer();

        let lines = &[(Level::INFO, "hello"), (Level::INFO, "world")];

        let make_writer = first.and(second);
        let _guard = install_writer(make_writer);
        info!("hello");
        info!("world");

        has_lines(&first_buf, &lines[..])?;
        has_lines(&second_buf, &lines[..])
    }

    #[test]
    fn optional_writer_forwards_enabled_writes_and_discards_disabled_writes(
    ) -> Result<(), TestFailure> {
        let mut some = OptionalWriter::some(Vec::new());
        ensure_eq(
            &ensure_ok(some.write(b"enabled"), "enabled optional writer writes")?,
            &7_usize,
            "enabled optional writer reports bytes written",
        )?;
        ensure_ok(some.flush(), "enabled optional writer flushes")?;
        match some {
            EitherWriter::First(bytes) => {
                ensure(
                    bytes == b"enabled".to_vec(),
                    "enabled optional writer stores bytes",
                )
            }
            EitherWriter::Second(_) => ensure(false, "enabled optional writer remains enabled"),
        }?;

        let mut none = OptionalWriter::<Vec<u8>>::none();
        ensure_eq(
            &ensure_ok(none.write(b"disabled"), "disabled optional writer accepts writes")?,
            &8_usize,
            "disabled optional writer reports accepted bytes",
        )?;
        ensure_ok(none.flush(), "disabled optional writer flushes")
    }

    #[test]
    fn option_converts_into_optional_writer_polarities() -> Result<(), TestFailure> {
        let some: OptionalWriter<Vec<u8>> = Some(b"present".to_vec()).into();
        let none: OptionalWriter<Vec<u8>> = None.into();

        match some {
            EitherWriter::First(bytes) => {
                ensure(bytes == b"present".to_vec(), "Some converts to enabled writer")
            }
            EitherWriter::Second(_) => ensure(false, "Some does not convert to sink"),
        }?;

        match none {
            EitherWriter::First(_) => ensure(false, "None does not convert to enabled writer"),
            EitherWriter::Second(_) => Ok(()),
        }
    }

    #[test]
    fn level_and_filter_combinators_enable_only_matching_metadata() -> Result<(), TestFailure> {
        static INFO_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_info",
            target: "writer_target",
            level: Level::INFO,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };
        static TRACE_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_trace",
            target: "writer_target",
            level: Level::TRACE,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };
        static ERROR_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_error",
            target: "writer_target",
            level: Level::ERROR,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };
        static OTHER_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_other",
            target: "other_target",
            level: Level::INFO,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };

        let (max_buf, max_writer) = writer_buffer();
        let max = max_writer.with_max_level(Level::INFO);
        ensure(
            matches!(max.make_writer_for(&INFO_META), OptionalWriter::First(_)),
            "max-level writer enables boundary level",
        )?;
        ensure(
            matches!(max.make_writer_for(&TRACE_META), OptionalWriter::Second(_)),
            "max-level writer disables more verbose level",
        )?;
        ensure(
            matches!(max.make_writer(), OptionalWriter::Second(_)),
            "max-level writer disables unclassified writes",
        )?;
        drop(max_buf);

        let (min_buf, min_writer) = writer_buffer();
        let min = min_writer.with_min_level(Level::WARN);
        ensure(
            matches!(min.make_writer_for(&TRACE_META), OptionalWriter::First(_)),
            "min-level writer enables more verbose level",
        )?;
        ensure(
            matches!(min.make_writer_for(&INFO_META), OptionalWriter::First(_)),
            "min-level writer enables levels above the minimum verbosity",
        )?;
        ensure(
            matches!(min.make_writer_for(&ERROR_META), OptionalWriter::Second(_)),
            "min-level writer disables less verbose levels",
        )?;
        ensure(
            matches!(min.make_writer(), OptionalWriter::Second(_)),
            "min-level writer disables unclassified writes",
        )?;
        drop(min_buf);

        let (filter_buf, filter_writer) = writer_buffer();
        let filtered = filter_writer.with_filter(|meta| meta.target() == "writer_target");
        ensure(
            matches!(filtered.make_writer_for(&INFO_META), OptionalWriter::First(_)),
            "filtered writer enables matching metadata",
        )?;
        ensure(
            matches!(filtered.make_writer_for(&OTHER_META), OptionalWriter::Second(_)),
            "filtered writer disables nonmatching metadata",
        )?;
        ensure(
            matches!(filtered.make_writer(), OptionalWriter::First(_)),
            "filtered writer enables unclassified writes",
        )?;
        drop(filter_buf);

        Ok(())
    }

    #[test]
    fn tee_and_or_else_forward_write_methods_to_expected_branches() -> Result<(), TestFailure> {
        static INFO_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_info",
            target: "writer_target",
            level: Level::INFO,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };
        static TRACE_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_trace",
            target: "writer_target",
            level: Level::TRACE,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };

        let mut tee = Tee::new(Vec::new(), Vec::new());
        ensure_eq(
            &ensure_ok(tee.write(b"tee"), "tee writes bytes")?,
            &3_usize,
            "tee reports the larger write count",
        )?;
        ensure_ok(tee.write_all(b"-all"), "tee forwards write_all")?;
        ensure_ok(write!(tee, "-fmt"), "tee forwards write_fmt")?;
        ensure_ok(tee.flush(), "tee forwards flush")?;
        ensure(tee.first == b"tee-all-fmt".to_vec(), "tee writes first branch")?;
        ensure(tee.second == b"tee-all-fmt".to_vec(), "tee writes second branch")?;

        let (primary_buf, primary) = writer_buffer();
        let (fallback_buf, fallback) = writer_buffer();
        let or_else = primary.with_max_level(Level::INFO).or_else(fallback);

        let mut primary_writer = or_else.make_writer_for(&INFO_META);
        ensure_ok(
            primary_writer.write_all(b"primary"),
            "or-else writes primary branch",
        )?;
        let mut fallback_writer = or_else.make_writer_for(&TRACE_META);
        ensure_ok(
            fallback_writer.write_all(b"fallback"),
            "or-else writes fallback branch",
        )?;

        ensure_eq(
            &String::from_utf8_lossy(&primary_buf.lock()).into_owned(),
            &String::from("primary"),
            "or-else uses primary writer when enabled",
        )?;
        ensure_eq(
            &String::from_utf8_lossy(&fallback_buf.lock()).into_owned(),
            &String::from("fallback"),
            "or-else uses fallback writer when primary is disabled",
        )
    }

    #[test]
    fn boxed_writer_forwards_type_erased_writes() -> Result<(), TestFailure> {
        static INFO_META: Metadata<'static> = tracing_core::metadata! {
            name: "writer_info",
            target: "writer_target",
            level: Level::INFO,
            fields: &[],
            callsite: &WRITER_CALLSITE,
            kind: Kind::EVENT,
        };

        let (boxed_buf, boxed_writer) = writer_buffer();
        let boxed = BoxMakeWriter::new(boxed_writer);
        let mut writer = boxed.make_writer();
        ensure_ok(writer.write_all(b"boxed"), "boxed writer forwards write_all")?;
        ensure(
            format!("{boxed:?}").contains("BoxMakeWriter"),
            "boxed writer debug names the type-erased wrapper",
        )?;
        ensure_eq(
            &String::from_utf8_lossy(&boxed_buf.lock()).into_owned(),
            &String::from("boxed"),
            "boxed writer stores forwarded bytes",
        )?;

        let (for_buf, for_writer) = writer_buffer();
        let boxed_for = BoxMakeWriter::new(for_writer);
        let mut writer_for = boxed_for.make_writer_for(&INFO_META);
        ensure_ok(
            writer_for.write_all(b"for"),
            "boxed writer forwards metadata-specific write_all",
        )?;
        ensure_eq(
            &String::from_utf8_lossy(&for_buf.lock()).into_owned(),
            &String::from("for"),
            "boxed writer stores metadata-specific bytes",
        )
    }

    #[test]
    fn tee_make_writer_constructs_default_and_metadata_specific_writer_pairs(
    ) -> Result<(), TestFailure> {
        let (first_buf, first) = writer_buffer();
        let (second_buf, second) = writer_buffer();
        let tee = first.and(second);

        let mut default_writer = tee.make_writer();
        ensure_ok(
            default_writer.write_all(b"default"),
            "tee default writer forwards write_all",
        )?;
        let mut metadata_writer = tee.make_writer_for(WRITER_CALLSITE.metadata());
        ensure_ok(
            metadata_writer.write_all(b"-metadata"),
            "tee metadata writer forwards write_all",
        )?;

        ensure_eq(
            &String::from_utf8_lossy(&first_buf.lock()).into_owned(),
            &String::from("default-metadata"),
            "tee writes default and metadata output to first writer",
        )?;
        ensure_eq(
            &String::from_utf8_lossy(&second_buf.lock()).into_owned(),
            &String::from("default-metadata"),
            "tee writes default and metadata output to second writer",
        )
    }

    #[test]
    fn or_else_default_writer_uses_fallback_when_primary_is_unclassified(
    ) -> Result<(), TestFailure> {
        let (primary_buf, primary) = writer_buffer();
        let (fallback_buf, fallback) = writer_buffer();
        let or_else = primary.with_max_level(Level::INFO).or_else(fallback);

        let mut writer = or_else.make_writer();
        ensure_ok(
            writer.write_all(b"fallback"),
            "or-else default writer uses fallback branch",
        )?;

        ensure(
            primary_buf.lock().is_empty(),
            "disabled primary receives no unclassified bytes",
        )?;
        ensure_eq(
            &String::from_utf8_lossy(&fallback_buf.lock()).into_owned(),
            &String::from("fallback"),
            "fallback receives unclassified bytes",
        )
    }

    #[test]
    fn either_writer_forwards_vectored_all_fmt_and_flush_to_both_variants(
    ) -> Result<(), TestFailure> {
        let mut first = EitherWriter::<Vec<u8>, Vec<u8>>::First(Vec::new());
        ensure_eq(
            &ensure_ok(
                first.write_vectored(&[io::IoSlice::new(b"a"), io::IoSlice::new(b"b")]),
                "first either writer forwards vectored writes",
            )?,
            &2_usize,
            "first either writer reports vectored byte count",
        )?;
        ensure_ok(first.write_all(b"-all"), "first either writer forwards write_all")?;
        ensure_ok(write!(first, "-fmt"), "first either writer forwards write_fmt")?;
        ensure_ok(first.flush(), "first either writer flushes")?;
        match first {
            EitherWriter::First(bytes) => ensure(
                bytes == b"ab-all-fmt".to_vec(),
                "first either writer stores branch A bytes",
            ),
            EitherWriter::Second(_) => ensure(false, "first either writer remains branch A"),
        }?;

        let mut second = EitherWriter::<Vec<u8>, Vec<u8>>::Second(Vec::new());
        ensure_eq(
            &ensure_ok(
                second.write_vectored(&[io::IoSlice::new(b"c"), io::IoSlice::new(b"d")]),
                "second either writer forwards vectored writes",
            )?,
            &2_usize,
            "second either writer reports vectored byte count",
        )?;
        ensure_ok(second.write_all(b"-all"), "second either writer forwards write_all")?;
        ensure_ok(write!(second, "-fmt"), "second either writer forwards write_fmt")?;
        ensure_ok(second.flush(), "second either writer flushes")?;
        match second {
            EitherWriter::First(_) => ensure(false, "second either writer remains branch B"),
            EitherWriter::Second(bytes) => ensure(
                bytes == b"cd-all-fmt".to_vec(),
                "second either writer stores branch B bytes",
            ),
        }
    }

    #[test]
    fn arc_make_writer_borrows_shared_writer_without_losing_state() -> Result<(), TestFailure> {
        #[derive(Default)]
        struct SharedWrite {
            bytes: Mutex<Vec<u8>>,
        }

        impl Write for &SharedWrite {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.bytes.lock().write(buf)
            }

            fn flush(&mut self) -> io::Result<()> {
                self.bytes.lock().flush()
            }

            fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
                self.bytes.lock().write_vectored(bufs)
            }

            fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
                self.bytes.lock().write_all(buf)
            }

            fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
                self.bytes.lock().write_fmt(args)
            }
        }

        let shared = Arc::new(SharedWrite::default());
        let mut writer = shared.make_writer();
        ensure_ok(writer.write_all(b"arc"), "arc writer forwards write_all")?;
        ensure_eq(
            &ensure_ok(
                writer.write_vectored(&[io::IoSlice::new(b"-vec"), io::IoSlice::new(b"-tail")]),
                "arc writer forwards vectored writes",
            )?,
            &9_usize,
            "arc writer reports all vectored bytes",
        )?;
        ensure_ok(write!(writer, "-fmt"), "arc writer forwards write_fmt")?;
        ensure_ok(writer.flush(), "arc writer flushes")?;

        ensure_eq(
            &String::from_utf8_lossy(&shared.bytes.lock()).into_owned(),
            &String::from("arc-vec-tail-fmt"),
            "arc writer preserves shared state across borrowed writer",
        )
    }

    #[cfg(any(feature = "json", feature = "time"))]
    #[test]
    fn write_adaptor_forwards_utf8_and_reports_formatter_errors() -> Result<(), TestFailure> {
        struct FailingFmt;

        impl fmt::Write for FailingFmt {
            fn write_str(&mut self, _s: &str) -> fmt::Result {
                Err(fmt::Error)
            }
        }

        let mut output = String::new();
        {
            let mut adaptor = WriteAdaptor::new(&mut output);
            ensure_eq(
                &ensure_ok(adaptor.write(b"utf8"), "write adaptor forwards utf8")?,
                &4_usize,
                "write adaptor reports UTF-8 byte count",
            )?;
            ensure_ok(adaptor.flush(), "write adaptor flushes")?;
            ensure(
                format!("{adaptor:?}").contains("WriteAdaptor"),
                "write adaptor debug identifies the adapter",
            )?;
        };
        ensure_eq(&output, &String::from("utf8"), "write adaptor appends text")?;

        let mut invalid_adaptor = WriteAdaptor::new(&mut output);
        ensure(
            invalid_adaptor.write(&[0xff]).is_err(),
            "write adaptor rejects invalid UTF-8 bytes",
        )?;

        let mut failing = FailingFmt;
        let mut failing_adaptor = WriteAdaptor::new(&mut failing);
        ensure(
            failing_adaptor.write(b"text").is_err(),
            "write adaptor maps fmt errors to io errors",
        )
    }
}
