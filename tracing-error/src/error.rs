//! Error wrappers and extension traits for carrying captured span traces.

use crate::SpanTrace;
use std::error::Error;
use std::fmt::{self, Debug, Display};
use std::marker::PhantomData;

/// A wrapper type for `Error`s that bundles a `SpanTrace` with an inner `Error`
/// type.
///
/// This type is a good match for the error-kind pattern where you have an error
/// type with an inner enum of error variants and you would like to capture a
/// span trace that can be extracted during printing without formatting the span
/// trace as part of your display impl.
///
/// An example of implementing an error type for a library using `TracedError`
/// might look like this
///
/// ```rust,compile_fail
/// #[derive(Debug, thiserror::Error)]
/// enum Kind {
///     // ...
/// }
///
/// #[derive(Debug)]
/// pub struct Error {
///     source: TracedError<Kind>,
///     backtrace: Backtrace,
/// }
///
/// impl std::error::Error for Error {
///     fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
///         self.source.source()
///     }
///
///     fn backtrace(&self) -> Option<&Backtrace> {
///         Some(&self.backtrace)
///     }
/// }
///
/// impl fmt::Display for Error {
///     fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
///         fmt::Display::fmt(&self.source, fmt)
///     }
/// }
///
/// impl<E> From<E> for Error
/// where
///     Kind: From<E>,
/// {
///     fn from(source: E) -> Self {
///         Self {
///             source: Kind::from(source).into(),
///             backtrace: Backtrace::capture(),
///         }
///     }
/// }
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "traced-error")))]
pub struct TracedError<E> {
    /// Erased error storage and the `SpanTrace` captured for it.
    inner: TracedErrorInner,
    /// Marker preserving the public source error type.
    _error: PhantomData<E>,
}

impl<E> From<E> for TracedError<E>
where
    E: Error + Send + Sync + 'static,
{
    fn from(error: E) -> Self {
        Self {
            inner: TracedErrorInner {
                error: Box::new(error),
                span_trace: SpanTrace::capture(),
            },
            _error: PhantomData,
        }
    }
}

/// Erased error storage used as the stable downcast target for span trace
/// extraction.
struct TracedErrorInner {
    /// The wrapped source error.
    error: Box<dyn Error + Send + Sync + 'static>,
    /// The span trace captured when the error was instrumented.
    span_trace: SpanTrace,
}

impl<E> Error for TracedError<E>
where
    E: Error + 'static,
{
    fn source<'a>(&'a self) -> Option<&'a (dyn Error + 'static)> {
        Some(&self.inner)
    }
}

impl<E> Debug for TracedError<E>
where
    E: Error,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.inner.error, f)
    }
}

impl<E> Display for TracedError<E>
where
    E: Error,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.inner.error, f)
    }
}

impl Error for TracedErrorInner {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.error.source()
    }
}

impl Debug for TracedErrorInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("span backtrace:\n")?;
        Debug::fmt(&self.span_trace, f)
    }
}

impl Display for TracedErrorInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("span backtrace:\n")?;
        Display::fmt(&self.span_trace, f)
    }
}

/// Extension trait for instrumenting errors with `SpanTrace`s.
#[cfg_attr(docsrs, doc(cfg(feature = "traced-error")))]
pub trait InstrumentError {
    /// The type of the wrapped error after instrumentation.
    type Instrumented;

    /// Instrument an `Error` by bundling it with a `SpanTrace`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tracing_error::{TracedError, InstrumentError};
    ///
    /// fn wrap_error<E>(e: E) -> TracedError<E>
    /// where
    ///     E: std::error::Error + Send + Sync + 'static
    /// {
    ///     e.in_current_span()
    /// }
    /// ```
    fn in_current_span(self) -> Self::Instrumented;
}

/// Extension trait for instrumenting errors in `Result`s with `SpanTrace`s.
#[cfg_attr(docsrs, doc(cfg(feature = "traced-error")))]
pub trait InstrumentResult<T> {
    /// The type of the wrapped error after instrumentation.
    type Instrumented;

    /// Instrument an `Error` by bundling it with a `SpanTrace`.
    ///
    /// # Errors
    ///
    /// Returns the original `Err` variant after wrapping its error in the current
    /// span.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use std::{io, fs};
    /// use tracing_error::{TracedError, InstrumentResult};
    ///
    /// # fn fallible_fn() -> io::Result<()> { fs::read_dir("......").map(drop) };
    ///
    /// fn do_thing() -> Result<(), TracedError<io::Error>> {
    ///     fallible_fn().in_current_span()
    /// }
    /// ```
    fn in_current_span(self) -> Result<T, Self::Instrumented>;
}

impl<T, E> InstrumentResult<T> for Result<T, E>
where
    E: InstrumentError,
{
    type Instrumented = <E as InstrumentError>::Instrumented;

    fn in_current_span(self) -> Result<T, Self::Instrumented> {
        self.map_err(E::in_current_span)
    }
}

/// A trait for extracting `SpanTrace`s created by `in_current_span()` from
/// `dyn Error` trait objects.
#[cfg_attr(docsrs, doc(cfg(feature = "traced-error")))]
pub trait ExtractSpanTrace {
    /// Attempts to downcast to a `TracedError` and return a reference to its
    /// `SpanTrace`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tracing_error::ExtractSpanTrace;
    /// use std::error::Error;
    ///
    /// fn print_span_trace(e: &(dyn Error + 'static)) {
    ///     let span_trace = e.span_trace();
    ///     if let Some(span_trace) = span_trace {
    ///         println!("{}", span_trace);
    ///     }
    /// }
    /// ```
    fn span_trace(&self) -> Option<&SpanTrace>;
}

impl<E> InstrumentError for E
where
    TracedError<E>: From<E>,
{
    type Instrumented = TracedError<E>;

    fn in_current_span(self) -> Self::Instrumented {
        TracedError::from(self)
    }
}

impl ExtractSpanTrace for dyn Error + 'static {
    fn span_trace(&self) -> Option<&SpanTrace> {
        self.downcast_ref::<TracedErrorInner>()
            .map(|inner| &inner.span_trace)
    }
}
