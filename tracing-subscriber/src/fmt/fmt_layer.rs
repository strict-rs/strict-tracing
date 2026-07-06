use crate::{
    field::RecordFields,
    fmt::{format, FormatEvent, FormatFields, MakeWriter, TestWriter},
    layer::{self, Context},
    registry::{self, LookupSpan, SpanRef},
};
use alloc::{fmt, format, string::String};
use core::{
    any::{type_name, Any, TypeId},
    marker::PhantomData,
    ops::Deref,
};
use format::{FmtSpan, TimingDisplay};
use std::{
    cell::{RefCell, RefMut},
    env, io, thread_local,
    time::Instant,
};
use tracing_core::{
    field,
    span::{Attributes, Current, Id, Record},
    subscriber::SubscriberResult,
    Event, Metadata, Subscriber,
};

/// A [`Layer`] that logs formatted representations of `tracing` events.
///
/// ## Examples
///
/// Constructing a layer with the default configuration:
///
/// ```rust
/// use tracing_subscriber::{fmt, Registry};
/// use tracing_subscriber::prelude::*;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// let subscriber = Registry::default()
///     .with(fmt::Layer::default());
///
/// tracing::subscriber::set_global_default(subscriber)?;
/// # Ok(()) }
/// ```
///
/// Overriding the layer's behavior:
///
/// ```rust
/// use tracing_subscriber::{fmt, Registry};
/// use tracing_subscriber::prelude::*;
///
/// let fmt_layer = fmt::layer()
///    .with_target(false) // don't include event targets when logging
///    .with_level(false); // don't include event levels when logging
///
/// let subscriber = Registry::default().with(fmt_layer);
/// # let _result = tracing::subscriber::set_global_default(subscriber);
/// ```
///
/// Setting a custom event formatter:
///
/// ```rust
/// use tracing_subscriber::fmt::{self, format, time};
/// use tracing_subscriber::prelude::*;
///
/// let fmt = format().with_timer(time::Uptime::default());
/// let fmt_layer = fmt::layer()
///     .event_format(fmt)
///     .with_target(false);
/// # let subscriber = fmt_layer.with_subscriber(tracing_subscriber::registry::Registry::default());
/// # let _result = tracing::subscriber::set_global_default(subscriber);
/// ```
///
/// [`Layer`]: super::layer::Layer
#[cfg_attr(docsrs, doc(cfg(all(feature = "fmt", feature = "std"))))]
#[derive(Debug)]
pub struct Layer<
    S,
    N = format::DefaultFields,
    E = format::Format<format::Full>,
    W = fn() -> io::Stdout,
> {
    /// Produces writers for formatted events.
    make_writer: W,
    /// Formats fields attached to spans and events.
    fmt_fields: N,
    /// Formats complete events.
    fmt_event: E,
    /// Configures synthesized span lifecycle events.
    fmt_span: format::FmtSpanConfig,
    /// Stores private boolean formatter flags.
    flags: LayerFlags,
    /// Tracks the wrapped subscriber type.
    _inner: PhantomData<fn(S)>,
}

/// Private boolean options for formatter layers.
#[derive(Clone, Copy, Debug)]
struct LayerFlags {
    /// Bitset storing enabled layer options.
    bits: u8,
}

impl LayerFlags {
    /// Default flags: sanitize ANSI values, do not emit ANSI output, and ignore internal errors.
    const DEFAULT: Self = Self {
        bits: Self::ANSI_SANITIZATION,
    };

    /// ANSI output flag bit.
    const ANSI: u8 = 0b001;
    /// ANSI sanitization flag bit.
    const ANSI_SANITIZATION: u8 = 0b010;
    /// Internal formatter error logging flag bit.
    const LOG_INTERNAL_ERRORS: u8 = 0b100;

    /// Returns true when a flag bit is enabled.
    const fn contains(self, bit: u8) -> bool {
        self.bits & bit != 0
    }

    /// Returns whether ANSI output is enabled.
    const fn is_ansi(self) -> bool {
        self.contains(Self::ANSI)
    }

    /// Returns whether ANSI sanitization is enabled.
    const fn ansi_sanitization(self) -> bool {
        self.contains(Self::ANSI_SANITIZATION)
    }

    /// Returns whether internal formatter errors are logged.
    const fn log_internal_errors(self) -> bool {
        self.contains(Self::LOG_INTERNAL_ERRORS)
    }

    /// Returns a copy with ANSI output configured.
    const fn with_ansi(self, ansi: bool) -> Self {
        self.with_flag(Self::ANSI, ansi)
    }

    /// Returns a copy with ANSI sanitization configured.
    const fn with_ansi_sanitization(self, ansi_sanitization: bool) -> Self {
        self.with_flag(Self::ANSI_SANITIZATION, ansi_sanitization)
    }

    /// Returns a copy with internal formatter error logging configured.
    const fn with_log_internal_errors(self, log_internal_errors: bool) -> Self {
        self.with_flag(Self::LOG_INTERNAL_ERRORS, log_internal_errors)
    }

    /// Returns a copy with a single flag configured.
    const fn with_flag(self, bit: u8, enabled: bool) -> Self {
        let bits = if enabled {
            self.bits | bit
        } else {
            self.bits & !bit
        };
        Self { bits }
    }
}

impl<S> Layer<S> {
    /// Returns a new [`Layer`][self::Layer] with the default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a default layer using an explicit `NO_COLOR` environment value.
    #[allow(
        clippy::single_call_fn,
        reason = "`NO_COLOR` handling is injected separately so tests can cover default ANSI policy"
    )]
    fn default_with_no_color(no_color: Option<&str>) -> Self {
        let ansi = default_ansi_enabled(no_color);

        Self {
            fmt_fields: format::DefaultFields::default(),
            fmt_event: format::Format::default(),
            fmt_span: format::FmtSpanConfig::default(),
            make_writer: io::stdout,
            flags: LayerFlags::DEFAULT.with_ansi(ansi),
            _inner: PhantomData,
        }
    }
}

// This needs to be a seperate impl block because they place different bounds on the type parameters.
impl<S, N, E, W> Layer<S, N, E, W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    /// Sets the [event formatter][`FormatEvent`] that the layer being built will
    /// use to format events.
    ///
    /// The event formatter may be any type implementing the [`FormatEvent`]
    /// trait, which is implemented for all functions taking a [`FmtContext`], a
    /// [`Writer`], and an [`Event`].
    ///
    /// # Examples
    ///
    /// Setting a type implementing [`FormatEvent`] as the formatter:
    /// ```rust
    /// use tracing_subscriber::fmt::{self, format};
    ///
    /// let layer = fmt::layer()
    ///     .event_format(format().compact());
    /// # // this is necessary for type inference.
    /// # use tracing_subscriber::Layer as _;
    /// # let _ = layer.with_subscriber(tracing_subscriber::registry::Registry::default());
    /// ```
    /// [`FormatEvent`]: format::FormatEvent
    /// [`Event`]: tracing::Event
    /// [`Writer`]: format::Writer
    pub fn event_format<E2>(self, fmt_event: E2) -> Layer<S, N, E2, W>
    where
        E2: FormatEvent<S, N> + 'static,
    {
        Layer {
            fmt_fields: self.fmt_fields,
            fmt_event,
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }

    /// Updates the event formatter by applying a function to the existing event formatter.
    ///
    /// This sets the event formatter that the layer being built will use to record fields.
    ///
    /// # Examples
    ///
    /// Updating an event formatter:
    ///
    /// ```rust
    /// let layer = tracing_subscriber::fmt::layer()
    ///     .map_event_format(|e| e.compact());
    /// # // this is necessary for type inference.
    /// # use tracing_subscriber::Layer as _;
    /// # let _ = layer.with_subscriber(tracing_subscriber::registry::Registry::default());
    /// ```
    pub fn map_event_format<E2>(self, f: impl FnOnce(E) -> E2) -> Layer<S, N, E2, W>
    where
        E2: FormatEvent<S, N> + 'static,
    {
        Layer {
            fmt_fields: self.fmt_fields,
            fmt_event: f(self.fmt_event),
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }
}

// This needs to be a seperate impl block because they place different bounds on the type parameters.
impl<S, N, E, W> Layer<S, N, E, W> {
    /// Sets the [`MakeWriter`] that the layer being built will use to write events.
    ///
    /// # Examples
    ///
    /// Using `stderr` rather than `stdout`:
    ///
    /// ```rust
    /// use std::io;
    /// use tracing_subscriber::fmt;
    ///
    /// let layer = fmt::layer()
    ///     .with_writer(io::stderr);
    /// # // this is necessary for type inference.
    /// # use tracing_subscriber::Layer as _;
    /// # let _ = layer.with_subscriber(tracing_subscriber::registry::Registry::default());
    /// ```
    pub fn with_writer<W2>(self, make_writer: W2) -> Layer<S, N, E, W2>
    where
        W2: for<'writer> MakeWriter<'writer> + 'static,
    {
        Layer {
            fmt_fields: self.fmt_fields,
            fmt_event: self.fmt_event,
            fmt_span: self.fmt_span,
            flags: self.flags,
            make_writer,
            _inner: self._inner,
        }
    }

    /// Borrows the [writer] for this [`Layer`].
    ///
    /// [writer]: MakeWriter
    pub const fn writer(&self) -> &W {
        &self.make_writer
    }

    /// Mutably borrows the [writer] for this [`Layer`].
    ///
    /// This method is primarily expected to be used with the
    /// [`reload::Handle::modify`](crate::reload::Handle::modify) method.
    ///
    /// # Examples
    ///
    /// ```
    /// # use tracing::info;
    /// # use tracing_subscriber::{fmt,reload,Registry,prelude::*};
    /// # fn non_blocking<T: std::io::Write>(writer: T) -> (fn() -> std::io::Stdout) {
    /// #   std::io::stdout
    /// # }
    /// # fn main() {
    /// let layer = fmt::layer().with_writer(non_blocking(std::io::stderr()));
    /// let (layer, reload_handle) = reload::Layer::new(layer);
    /// #
    /// # // specifying the Registry type is required
    /// # let _: &reload::Handle<fmt::Layer<Registry, _, _, _>, Registry> = &reload_handle;
    /// #
    /// info!("This will be logged to stderr");
    /// reload_handle.modify(|layer| *layer.writer_mut() = non_blocking(std::io::stdout()));
    /// info!("This will be logged to stdout");
    /// # }
    /// ```
    ///
    /// [writer]: MakeWriter
    pub const fn writer_mut(&mut self) -> &mut W {
        &mut self.make_writer
    }

    /// Sets whether this layer should use ANSI terminal formatting
    /// escape codes (such as colors).
    ///
    /// This method is primarily expected to be used with the
    /// [`reload::Handle::modify`](crate::reload::Handle::modify) method when changing
    /// the writer.
    #[cfg(feature = "ansi")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ansi")))]
    pub const fn set_ansi(&mut self, ansi: bool) {
        self.flags = self.flags.with_ansi(ansi);
    }

    /// Modifies how synthesized events are emitted at points in the [span
    /// lifecycle][lifecycle].
    ///
    /// See [`Self::with_span_events`] for documentation on the [`FmtSpan`]
    ///
    /// This method is primarily expected to be used with the
    /// [`reload::Handle::modify`](crate::reload::Handle::modify) method
    ///
    /// Note that using this method modifies the span configuration instantly and does not take into
    /// account any current spans. If the previous configuration was set to capture
    /// `FmtSpan::ALL`, for example, using this method to change to `FmtSpan::NONE` will cause an
    /// exit event for currently entered events not to be formatted
    ///
    /// [lifecycle]: mod@tracing::span#the-span-lifecycle
    pub const fn set_span_events(&mut self, kind: FmtSpan) {
        self.fmt_span = format::FmtSpanConfig {
            kind,
            fmt_timing: self.fmt_span.fmt_timing,
        }
    }

    /// Configures the layer to support [`libtest`'s output capturing][capturing] when used in
    /// unit tests.
    ///
    /// See [`TestWriter`] for additional details.
    ///
    /// # Examples
    ///
    /// Using [`TestWriter`] to let `cargo test` capture test output:
    ///
    /// ```rust
    /// use std::io;
    /// use tracing_subscriber::fmt;
    ///
    /// let layer = fmt::layer()
    ///     .with_test_writer();
    /// # // this is necessary for type inference.
    /// # use tracing_subscriber::Layer as _;
    /// # let _ = layer.with_subscriber(tracing_subscriber::registry::Registry::default());
    /// ```
    /// [capturing]:
    /// https://doc.rust-lang.org/book/ch11-02-running-tests.html#showing-function-output
    /// [`TestWriter`]: super::writer::TestWriter
    pub fn with_test_writer(self) -> Layer<S, N, E, TestWriter> {
        Layer {
            fmt_fields: self.fmt_fields,
            fmt_event: self.fmt_event,
            fmt_span: self.fmt_span,
            flags: self.flags,
            make_writer: TestWriter::default(),
            _inner: self._inner,
        }
    }

    /// Sets whether or not the formatter emits ANSI terminal escape codes
    /// for colors and other text formatting.
    ///
    /// When the "ansi" crate feature flag is enabled, ANSI colors are enabled
    /// by default unless the [`NO_COLOR`] environment variable is set to
    /// a non-empty value.  If the [`NO_COLOR`] environment variable is set to
    /// any non-empty value, then ANSI colors will be suppressed by default.
    /// The [`with_ansi`] and [`set_ansi`] methods can be used to forcibly
    /// enable ANSI colors, overriding any [`NO_COLOR`] environment variable.
    ///
    /// [`NO_COLOR`]: https://no-color.org/
    ///
    /// This method itself is still available without the feature flag. This
    /// is to allow ANSI escape codes to be explicitly *disabled* without
    /// having to opt-in to the dependencies required to emit ANSI formatting.
    /// This way, code which constructs a formatter that should never emit
    /// ANSI escape codes can ensure that they are not used, regardless of
    /// whether or not other crates in the dependency graph enable the "ansi"
    /// feature flag.
    ///
    /// [`with_ansi`]: Layer::with_ansi
    /// [`set_ansi`]: Layer::set_ansi
    #[cfg(feature = "ansi")]
    #[must_use]
    pub fn with_ansi(self, ansi: bool) -> Self {
        Self {
            flags: self.flags.with_ansi(ansi),
            ..self
        }
    }

    /// Sets whether or not the formatter emits ANSI terminal escape codes
    /// for colors and other text formatting.
    ///
    /// ANSI output is unavailable because this crate was built without the
    /// "ansi" feature. Calling this method is therefore a no-op; the returned
    /// layer leaves ANSI disabled even when passed `true`.
    #[cfg(not(feature = "ansi"))]
    #[must_use]
    pub fn with_ansi(self, _requested_ansi: bool) -> Self {
        Self {
            flags: self.flags.with_ansi(false),
            ..self
        }
    }

    /// Sets whether ANSI control character sanitization is enabled.
    ///
    /// This defaults to `true` as a protective measure against terminal
    /// injection attacks. If this is set to `false`, ANSI sanitization is
    /// disabled and trusted ANSI control sequences in logged values are passed
    /// through unchanged.
    #[must_use]
    pub fn with_ansi_sanitization(self, ansi_sanitization: bool) -> Self {
        Self {
            flags: self.flags.with_ansi_sanitization(ansi_sanitization),
            ..self
        }
    }

    /// Sets whether to write errors from [`FormatEvent`] to the writer.
    /// Defaults to true.
    ///
    /// By default, `fmt::Layer` will write any `FormatEvent`-internal errors to
    /// the writer. These errors are unlikely and will only occur if there is a
    /// bug in the `FormatEvent` implementation or its dependencies.
    ///
    /// If writing to the writer fails, the error message is printed to stderr
    /// as a fallback.
    ///
    /// [`FormatEvent`]: crate::fmt::FormatEvent
    #[must_use]
    pub fn log_internal_errors(self, log_internal_errors: bool) -> Self {
        Self {
            flags: self.flags.with_log_internal_errors(log_internal_errors),
            ..self
        }
    }

    /// Updates the [`MakeWriter`] by applying a function to the existing [`MakeWriter`].
    ///
    /// This sets the [`MakeWriter`] that the layer being built will use to write events.
    ///
    /// # Examples
    ///
    /// Redirect output to stderr if level is <= WARN:
    ///
    /// ```rust
    /// use tracing::Level;
    /// use tracing_subscriber::fmt::{self, writer::MakeWriterExt};
    ///
    /// let stderr = std::io::stderr.with_max_level(Level::WARN);
    /// let layer = fmt::layer()
    ///     .map_writer(move |w| stderr.or_else(w));
    /// # // this is necessary for type inference.
    /// # use tracing_subscriber::Layer as _;
    /// # let _ = layer.with_subscriber(tracing_subscriber::registry::Registry::default());
    /// ```
    pub fn map_writer<W2>(self, f: impl FnOnce(W) -> W2) -> Layer<S, N, E, W2>
    where
        W2: for<'writer> MakeWriter<'writer> + 'static,
    {
        Layer {
            fmt_fields: self.fmt_fields,
            fmt_event: self.fmt_event,
            fmt_span: self.fmt_span,
            flags: self.flags,
            make_writer: f(self.make_writer),
            _inner: self._inner,
        }
    }

    /// Sets the field formatter that the layer being built will use to record
    /// fields.
    pub fn fmt_fields<N2>(self, fmt_fields: N2) -> Layer<S, N2, E, W>
    where
        N2: for<'writer> FormatFields<'writer> + 'static,
    {
        Layer {
            fmt_event: self.fmt_event,
            fmt_fields,
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }

    /// Updates the field formatter by applying a function to the existing field formatter.
    ///
    /// This sets the field formatter that the layer being built will use to record fields.
    ///
    /// # Examples
    ///
    /// Updating a field formatter:
    ///
    /// ```rust
    /// use tracing_subscriber::field::MakeExt;
    /// let layer = tracing_subscriber::fmt::layer()
    ///     .map_fmt_fields(|f| f.debug_alt());
    /// # // this is necessary for type inference.
    /// # use tracing_subscriber::Layer as _;
    /// # let _ = layer.with_subscriber(tracing_subscriber::registry::Registry::default());
    /// ```
    pub fn map_fmt_fields<N2>(self, f: impl FnOnce(N) -> N2) -> Layer<S, N2, E, W>
    where
        N2: for<'writer> FormatFields<'writer> + 'static,
    {
        Layer {
            fmt_event: self.fmt_event,
            fmt_fields: f(self.fmt_fields),
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }
}

impl<S, N, L, T, W> Layer<S, N, format::Format<L, T>, W>
where
    N: for<'writer> FormatFields<'writer> + 'static,
{
    /// Use the given [`timer`] for span and event timestamps.
    ///
    /// See the [`time` module] for the provided timer implementations.
    ///
    /// Note that using the `"time`"" feature flag enables the
    /// additional time formatters [`UtcTime`] and [`LocalTime`], which use the
    /// [`time` crate] to provide more sophisticated timestamp formatting
    /// options.
    ///
    /// [`timer`]: super::time::FormatTime
    /// [`time` module]: mod@super::time
    /// [`UtcTime`]: super::time::UtcTime
    /// [`LocalTime`]: super::time::LocalTime
    /// [`time` crate]: https://docs.rs/time/0.3
    pub fn with_timer<T2>(self, timer: T2) -> Layer<S, N, format::Format<L, T2>, W> {
        Layer {
            fmt_event: self.fmt_event.with_timer(timer),
            fmt_fields: self.fmt_fields,
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }

    /// Do not emit timestamps with spans and event.
    pub fn without_time(self) -> Layer<S, N, format::Format<L, ()>, W> {
        Layer {
            fmt_event: self.fmt_event.without_time(),
            fmt_fields: self.fmt_fields,
            fmt_span: self.fmt_span.without_time(),
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }

    /// Configures how synthesized events are emitted at points in the [span
    /// lifecycle][lifecycle].
    ///
    /// The following options are available:
    ///
    /// - `FmtSpan::NONE`: No events will be synthesized when spans are
    ///   created, entered, exited, or closed. Data from spans will still be
    ///   included as the context for formatted events. This is the default.
    /// - `FmtSpan::NEW`: An event will be synthesized when spans are created.
    /// - `FmtSpan::ENTER`: An event will be synthesized when spans are entered.
    /// - `FmtSpan::EXIT`: An event will be synthesized when spans are exited.
    /// - `FmtSpan::CLOSE`: An event will be synthesized when a span closes. If
    ///   [timestamps are enabled][time] for this formatter, the generated
    ///   event will contain fields with the span's _busy time_ (the total
    ///   time for which it was entered) and _idle time_ (the total time that
    ///   the span existed but was not entered).
    /// - `FmtSpan::ACTIVE`: Events will be synthesized when spans are entered
    ///   or exited.
    /// - `FmtSpan::FULL`: Events will be synthesized whenever a span is
    ///   created, entered, exited, or closed. If timestamps are enabled, the
    ///   close event will contain the span's busy and idle time, as
    ///   described above.
    ///
    /// The options can be enabled in any combination. For instance, the following
    /// will synthesize events whenever spans are created and closed:
    ///
    /// ```rust
    /// use tracing_subscriber::fmt;
    /// use tracing_subscriber::fmt::format::FmtSpan;
    ///
    /// let subscriber = fmt()
    ///     .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
    ///     .finish();
    /// ```
    ///
    /// Note that the generated events will only be part of the log output by
    /// this formatter; they will not be recorded by other `Subscriber`s or by
    /// `Layer`s added to this subscriber.
    ///
    /// [lifecycle]: https://docs.rs/tracing/latest/tracing/span/index.html#the-span-lifecycle
    /// [time]: Layer::without_time()
    #[must_use]
    pub fn with_span_events(self, kind: FmtSpan) -> Self {
        Self {
            fmt_span: self.fmt_span.with_kind(kind),
            ..self
        }
    }

    /// Sets whether or not an event's target is displayed.
    #[must_use]
    pub fn with_target(self, display_target: bool) -> Self {
        Self {
            fmt_event: self.fmt_event.with_target(display_target),
            ..self
        }
    }
    /// Sets whether or not an event's [source code file path][file] is
    /// displayed.
    ///
    /// [file]: tracing_core::Metadata::file
    #[must_use]
    pub fn with_file(self, display_filename: bool) -> Self {
        Self {
            fmt_event: self.fmt_event.with_file(display_filename),
            ..self
        }
    }

    /// Sets whether or not an event's [source code line number][line] is
    /// displayed.
    ///
    /// [line]: tracing_core::Metadata::line
    #[must_use]
    pub fn with_line_number(
        self,
        display_line_number: bool,
    ) -> Self {
        Self {
            fmt_event: self.fmt_event.with_line_number(display_line_number),
            ..self
        }
    }

    /// Sets whether or not an event's level is displayed.
    #[must_use]
    pub fn with_level(self, display_level: bool) -> Self {
        Self {
            fmt_event: self.fmt_event.with_level(display_level),
            ..self
        }
    }

    /// Sets whether or not the [thread ID] of the current thread is displayed
    /// when formatting events.
    ///
    /// [thread ID]: std::thread::ThreadId
    #[must_use]
    pub fn with_thread_ids(self, display_thread_ids: bool) -> Self {
        Self {
            fmt_event: self.fmt_event.with_thread_ids(display_thread_ids),
            ..self
        }
    }

    /// Sets whether or not the [name] of the current thread is displayed
    /// when formatting events.
    ///
    /// [name]: std::thread#naming-threads
    #[must_use]
    pub fn with_thread_names(
        self,
        display_thread_names: bool,
    ) -> Self {
        Self {
            fmt_event: self.fmt_event.with_thread_names(display_thread_names),
            ..self
        }
    }

    /// Sets the layer being built to use a [less verbose formatter][super::format::Compact].
    pub fn compact(self) -> Layer<S, N, format::Format<format::Compact, T>, W>
    where
        N: for<'writer> FormatFields<'writer> + 'static,
    {
        Layer {
            fmt_event: self.fmt_event.compact(),
            fmt_fields: self.fmt_fields,
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }

    /// Sets the layer being built to use an [excessively pretty, human-readable formatter][format::Pretty].
    #[cfg(feature = "ansi")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ansi")))]
    pub fn pretty(self) -> Layer<S, format::Pretty, format::Format<format::Pretty, T>, W> {
        Layer {
            fmt_event: self.fmt_event.pretty(),
            fmt_fields: format::Pretty::default(),
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            flags: self.flags,
            _inner: self._inner,
        }
    }

    /// Sets the layer being built to use a [JSON formatter][super::format::Json].
    ///
    /// The full format includes fields from all entered spans.
    ///
    /// # Example Output
    ///
    /// ```ignore,json
    /// {"timestamp":"Feb 20 11:28:15.096","level":"INFO","target":"mycrate","fields":{"message":"some message", "key": "value"}}
    /// ```
    ///
    /// # Options
    ///
    /// - [`Layer::flatten_event`] can be used to enable flattening event fields into the root
    ///   object.
    ///
    /// [`Layer::flatten_event`]: Layer::flatten_event()
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn json(self) -> Layer<S, format::JsonFields, format::Format<format::Json, T>, W> {
        Layer {
            fmt_event: self.fmt_event.json(),
            fmt_fields: format::JsonFields::new(),
            fmt_span: self.fmt_span,
            make_writer: self.make_writer,
            // always disable ANSI escapes in JSON mode!
            flags: self.flags.with_ansi(false),
            _inner: self._inner,
        }
    }
}

#[cfg(feature = "json")]
#[cfg_attr(docsrs, doc(cfg(feature = "json")))]
impl<S, T, W> Layer<S, format::JsonFields, format::Format<format::Json, T>, W> {
    /// Sets the JSON layer being built to flatten event metadata.
    ///
    /// See [`format::Json`][super::format::Json]
    #[must_use]
    pub fn flatten_event(
        self,
        flatten_event: bool,
    ) -> Self {
        Self {
            fmt_event: self.fmt_event.flatten_event(flatten_event),
            fmt_fields: format::JsonFields::new(),
            ..self
        }
    }

    /// Sets whether or not the formatter will include the current span in
    /// formatted events.
    ///
    /// See [`format::Json`][super::format::Json]
    #[must_use]
    pub fn with_current_span(
        self,
        display_current_span: bool,
    ) -> Self {
        Self {
            fmt_event: self.fmt_event.with_current_span(display_current_span),
            fmt_fields: format::JsonFields::new(),
            ..self
        }
    }

    /// Sets whether or not the formatter will include a list (from root to leaf)
    /// of all currently entered spans in formatted events.
    ///
    /// See [`format::Json`][super::format::Json]
    #[must_use]
    pub fn with_span_list(
        self,
        display_span_list: bool,
    ) -> Self {
        Self {
            fmt_event: self.fmt_event.with_span_list(display_span_list),
            fmt_fields: format::JsonFields::new(),
            ..self
        }
    }
}

impl<S> Default for Layer<S> {
    fn default() -> Self {
        let no_color = env::var("NO_COLOR").ok();
        Self::default_with_no_color(no_color.as_deref())
    }
}

/// Returns whether ANSI escapes are enabled for the provided `NO_COLOR` value.
#[allow(
    clippy::single_call_fn,
    reason = "`NO_COLOR` policy is kept as a named predicate shared by defaults and tests"
)]
fn default_ansi_enabled(no_color: Option<&str>) -> bool {
    // Only enable ANSI when the feature is enabled, and the NO_COLOR
    // environment variable is unset or empty.
    cfg!(feature = "ansi") && no_color.is_none_or(str::is_empty)
}

/// Returns the saturating nanoseconds between two [`Instant`] values.
#[allow(
    clippy::single_call_fn,
    reason = "span timing accounting centralizes saturating `Instant` conversion"
)]
fn nanos_between(start: Instant, end: Instant) -> u64 {
    let nanos = end.duration_since(start).as_nanos();
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

/// Writes a best-effort internal formatter error to stderr.
fn write_internal_error(args: fmt::Arguments<'_>) {
    let mut stderr = io::stderr();
    let _result: io::Result<()> = io::Write::write_fmt(&mut stderr, args);
}

/// Logs a writer failure produced while reporting a formatting event.
fn log_internal_write_error(enabled: bool, result: io::Result<()>, context: &str) {
    if !enabled {
        return;
    }
    if let Err(error) = result {
        write_internal_error(format_args!(
            "[tracing-subscriber] {context} Error: {error}\n"
        ));
    }
}

impl<S, N, E, W> Layer<S, N, E, W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
    E: FormatEvent<S, N> + 'static,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    #[inline]
    /// Builds the formatting context for an event.
    const fn make_ctx<'a>(&'a self, ctx: Context<'a, S>, event: &'a Event<'a>) -> FmtContext<'a, S, N> {
        FmtContext {
            ctx,
            fmt_fields: &self.fmt_fields,
            event,
        }
    }
}

/// A formatted representation of a span's fields stored in its [extensions].
///
/// Because `FormattedFields` is generic over the type of the formatter that
/// produced it, multiple versions of a span's formatted fields can be stored in
/// the [`Extensions`][extensions] type-map. This means that when multiple
/// formatters are in use, each can store its own formatted representation
/// without conflicting.
///
/// [extensions]: crate::registry::Extensions
pub struct FormattedFields<E: ?Sized> {
    /// The rendered field string.
    fields: String,
    /// Associates this field string with its field formatter type.
    _format_fields: PhantomData<fn(E)>,
    /// Whether ANSI escapes were enabled when the fields were formatted.
    was_ansi: bool,
    /// Whether ANSI escapes were sanitized when the fields were formatted.
    was_ansi_sanitized: bool,
}

/// A mutable event-formatting buffer borrowed from thread-local storage or allocated as a fallback.
enum EventBuffer<'a> {
    /// A successfully borrowed thread-local buffer.
    ThreadLocal(RefMut<'a, String>),
    /// An owned fallback buffer used during recursive formatter entry.
    Fallback(String),
}

impl EventBuffer<'_> {
    /// Returns mutable access to the underlying string buffer.
    fn as_mut_string(&mut self) -> &mut String {
        match *self {
            Self::ThreadLocal(ref mut buffer) => buffer,
            Self::Fallback(ref mut buffer) => buffer,
        }
    }
}

impl<E: ?Sized> Default for FormattedFields<E> {
    fn default() -> Self {
        Self {
            _format_fields: PhantomData,
            was_ansi: Default::default(),
            was_ansi_sanitized: true,
            fields: String::default(),
        }
    }
}

impl<E: ?Sized> FormattedFields<E> {
    /// Returns a new `FormattedFields`.
    #[must_use]
    pub fn new(fields: String) -> Self {
        Self {
            fields,
            was_ansi: false,
            was_ansi_sanitized: true,
            _format_fields: PhantomData,
        }
    }

    /// Returns a new [`format::Writer`] for writing to this `FormattedFields`.
    ///
    /// The returned [`format::Writer`] can be used with the
    /// [`FormatFields::format_fields`] method.
    pub fn as_writer(&mut self) -> format::Writer<'_> {
        format::Writer::new(&mut self.fields)
            .with_ansi(self.was_ansi)
            .with_ansi_sanitization(self.was_ansi_sanitized)
    }

    /// Returns the formatted field string.
    #[must_use]
    pub fn fields(&self) -> &str {
        &self.fields
    }

    /// Returns mutable access to the formatted field string for formatters.
    pub(in crate::fmt) const fn fields_mut(&mut self) -> &mut String {
        &mut self.fields
    }
}

impl<E: ?Sized> fmt::Debug for FormattedFields<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FormattedFields")
            .field("fields", &self.fields)
            .field("formatter", &format_args!("{}", type_name::<E>()))
            .field("was_ansi", &self.was_ansi)
            .field("was_ansi_sanitized", &self.was_ansi_sanitized)
            .finish()
    }
}

impl<E: ?Sized> fmt::Display for FormattedFields<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.fields, f)
    }
}

impl<E: ?Sized> Deref for FormattedFields<E> {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

// === impl FmtLayer ===

/// Builds a synthetic event from a span and passes it to a local block.
macro_rules! with_event_from_span {
    ($id:ident, $span:ident, $($field:literal = $value:expr),*, |$event:ident| $code:block) => {
        let meta = $span.metadata();
        let cs = meta.callsite();
        let fs = field::FieldSet::new(&[$($field),*], cs);
        let values = [$(
            {
                let field_value: &dyn field::Value = &$value;
                ::core::option::Option::Some(field_value)
            },
        )*];
        let value_set = fs.value_set_all(&values);
        let $event = Event::new_child_of($id, meta, &value_set);
        $code
    };
}

impl<S, N, E, W> layer::Layer<S> for Layer<S, N, E, W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'writer> FormatFields<'writer> + 'static,
    E: FormatEvent<S, N> + 'static,
    W: for<'writer> MakeWriter<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: Id, ctx: Context<'_, S>) -> SubscriberResult {
        let Some(span) = ctx.span(id) else {
            return Ok(());
        };
        let mut extensions = span.extensions_mut();

        if extensions.get_mut::<FormattedFields<N>>().is_none() {
            let mut fields = FormattedFields::<N>::new(String::new());
            fields.was_ansi = self.flags.is_ansi();
            fields.was_ansi_sanitized = self.flags.ansi_sanitization();
            if self
                .fmt_fields
                .format_fields(fields.as_writer(), attrs)
                .is_ok()
            {
                let _previous = extensions.insert(fields);
            } else {
                write_internal_error(format_args!(
                    "[tracing-subscriber] Unable to format span fields, ignoring fields for span `{}`\n",
                    attrs.metadata().name()
                ));
            }
        }

        if self.fmt_span.fmt_timing
            && self.fmt_span.trace_close()
            && extensions.get_mut::<Timings>().is_none()
        {
            let _previous = extensions.insert(Timings::new());
        }

        if self.fmt_span.trace_new() {
            with_event_from_span!(id, span, "message" = "new", |event| {
                drop(extensions);
                drop(span);
                self.on_event(&event, ctx)?;
            });
        }

        Ok(())
    }

    fn on_record(&self, id: Id, values: &Record<'_>, ctx: Context<'_, S>) -> SubscriberResult {
        let Some(span) = ctx.span(id) else {
            return Ok(());
        };

        {
            let mut extensions = span.extensions_mut();
            if let Some(fields) = extensions.get_mut::<FormattedFields<N>>() {
                let _result: fmt::Result = self.fmt_fields.add_fields(fields, values);
                return Ok(());
            }
        }

        let mut fields = FormattedFields::<N>::new(String::new());
        fields.was_ansi = self.flags.is_ansi();
        fields.was_ansi_sanitized = self.flags.ansi_sanitization();
        if self
            .fmt_fields
            .format_fields(fields.as_writer(), values)
            .is_ok()
        {
            let mut extensions = span.extensions_mut();
            let _previous = extensions.insert(fields);
        }

        Ok(())
    }

    fn on_enter(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult {
        if !(self.fmt_span.trace_enter() || self.fmt_span.trace_close() && self.fmt_span.fmt_timing)
        {
            return Ok(());
        }
        let Some(span) = ctx.span(id) else {
            return Ok(());
        };
        let mut extensions = span.extensions_mut();
        if let Some(timings) = extensions.get_mut::<Timings>() {
            if timings.entered_count == 0 {
                let now = Instant::now();
                timings.idle = timings.idle.saturating_add(nanos_between(timings.last, now));
                timings.last = now;
            }
            timings.entered_count = timings.entered_count.saturating_add(1);
        }

        if self.fmt_span.trace_enter() {
            with_event_from_span!(id, span, "message" = "enter", |event| {
                drop(extensions);
                drop(span);
                self.on_event(&event, ctx)?;
            });
        }

        Ok(())
    }

    fn on_exit(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult {
        if !(self.fmt_span.trace_exit() || self.fmt_span.trace_close() && self.fmt_span.fmt_timing)
        {
            return Ok(());
        }
        let Some(span) = ctx.span(id) else {
            return Ok(());
        };
        let mut extensions = span.extensions_mut();
        if let Some(timings) = extensions.get_mut::<Timings>() {
            timings.entered_count = timings.entered_count.saturating_sub(1);
            if timings.entered_count == 0 {
                let now = Instant::now();
                timings.busy = timings.busy.saturating_add(nanos_between(timings.last, now));
                timings.last = now;
            }
        }

        if self.fmt_span.trace_exit() {
            with_event_from_span!(id, span, "message" = "exit", |event| {
                drop(extensions);
                drop(span);
                self.on_event(&event, ctx)?;
            });
        }

        Ok(())
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) -> SubscriberResult {
        if !self.fmt_span.trace_close() {
            return Ok(());
        }
        let Some(span) = ctx.span(id) else {
            return Ok(());
        };
        let extensions = span.extensions();
        if let Some(timing) = extensions.get::<Timings>() {
            let Timings {
                busy,
                mut idle,
                last,
                entered_count,
            } = *timing;
            if entered_count == 0 {
                idle = idle.saturating_add(nanos_between(last, Instant::now()));
            }

            let t_idle = field::display(TimingDisplay(idle));
            let t_busy = field::display(TimingDisplay(busy));

            with_event_from_span!(
                id,
                span,
                "message" = "close",
                "time.busy" = t_busy,
                "time.idle" = t_idle,
                |event| {
                    drop(extensions);
                    drop(span);
                    self.on_event(&event, ctx)?;
                }
            );
        } else {
            with_event_from_span!(id, span, "message" = "close", |event| {
                drop(extensions);
                drop(span);
                self.on_event(&event, ctx)?;
            });
        }

        Ok(())
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
        thread_local! {
            static BUF: RefCell<String> = const { RefCell::new(String::new()) };
        }

        BUF.with(|thread_local_cell| {
            let mut event_buffer = thread_local_cell
                .try_borrow_mut()
                .map_or_else(
                    |_| EventBuffer::Fallback(String::new()),
                    EventBuffer::ThreadLocal,
                );
            let buf = event_buffer.as_mut_string();

            let format_context = self.make_ctx(ctx, event);
            match self
                .fmt_event
                .format_event(
                        &format_context,
                        format::Writer::new(buf)
                        .with_ansi(self.flags.is_ansi())
                        .with_ansi_sanitization(self.flags.ansi_sanitization()),
                    event,
                ) {
                Ok(()) => {
                    let mut writer = self.make_writer.make_writer_for(event.metadata());
                    let res = io::Write::write_all(&mut writer, buf.as_bytes());
                    log_internal_write_error(
                        self.flags.log_internal_errors(),
                        res,
                        "Unable to write an event to the Writer for this Subscriber!",
                    );
                }
                Err(_) if self.flags.log_internal_errors() => {
                    let err_msg = format!(
                        "Unable to format the following event. Name: {}\n",
                        event.metadata().name()
                    );
                    let mut writer = self.make_writer.make_writer_for(event.metadata());
                    let res = io::Write::write_all(&mut writer, err_msg.as_bytes());
                    log_internal_write_error(
                        true,
                        res,
                        "Unable to write an \"event formatting error\" to the Writer for this Subscriber!",
                    );
                }
                Err(_) => {}
            }

            buf.clear();
        });

        Ok(())
    }

    fn downcast_ref_by_id(&self, id: TypeId) -> Option<&dyn Any> {
        // This impl allows downcasting a `fmt` layer to any of
        // its components (event formatter, field formatter, and `MakeWriter`)
        // as well as to the layer's type itself. The potential use-cases for
        // this *may* be somewhat niche, though...
        match () {
            () if id == TypeId::of::<Self>() => Some(self),
            () if id == TypeId::of::<E>() => Some(&self.fmt_event),
            () if id == TypeId::of::<N>() => Some(&self.fmt_fields),
            () if id == TypeId::of::<W>() => Some(&self.make_writer),
            () => None,
        }
    }
}

/// Provides the current span context to a formatter.
pub struct FmtContext<'a, S, N> {
    /// The layer context for looking up spans.
    pub(crate) ctx: Context<'a, S>,
    /// The field formatter configured for this layer.
    pub(crate) fmt_fields: &'a N,
    /// The event currently being formatted.
    pub(crate) event: &'a Event<'a>,
}

impl<S, N> fmt::Debug for FmtContext<'_, S, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FmtContext").finish()
    }
}

impl<'writer, S, N> FormatFields<'writer> for FmtContext<'_, S, N>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: FormatFields<'writer> + 'static,
{
    fn format_fields<R: RecordFields>(
        &self,
        writer: format::Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        self.fmt_fields.format_fields(writer, fields)
    }
}

impl<S, N> FmtContext<'_, S, N>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    /// Visits every span in the current context with a closure.
    ///
    /// The provided closure will be called first with the current span,
    /// and then with that span's parent, and then that span's parent,
    /// and so on until a root span is reached.
    ///
    /// # Errors
    ///
    /// Returns the first error produced by the visitor closure.
    pub fn visit_spans<E, F>(&self, mut f: F) -> Result<(), E>
    where
        F: FnMut(&SpanRef<'_, S>) -> Result<(), E>,
    {
        // visit all the current spans
        if let Some(scope) = self.event_scope() {
            for span in scope.root_to_leaf() {
                f(&span)?;
            }
        }
        Ok(())
    }

    /// Returns metadata for the span with the given `id`, if it exists.
    ///
    /// If this returns `None`, then no span exists for that ID (either it has
    /// closed or the ID is invalid).
    #[inline]
    #[must_use]
    pub fn metadata(&self, id: Id) -> Option<&'static Metadata<'static>>
    where
        S: for<'lookup> LookupSpan<'lookup>,
    {
        self.ctx.metadata(id)
    }

    /// Returns [stored data] for the span with the given `id`, if it exists.
    ///
    /// If this returns `None`, then no span exists for that ID (either it has
    /// closed or the ID is invalid).
    ///
    /// [stored data]: crate::registry::SpanRef
    #[inline]
    #[must_use]
    pub fn span(&self, id: Id) -> Option<SpanRef<'_, S>>
    where
        S: for<'lookup> LookupSpan<'lookup>,
    {
        self.ctx.span(id)
    }

    /// Returns `true` if an active span exists for the given `Id`.
    #[inline]
    #[must_use]
    pub fn exists(&self, id: Id) -> bool
    where
        S: for<'lookup> LookupSpan<'lookup>,
    {
        self.ctx.exists(id)
    }

    /// Returns [stored data] for the span that the wrapped subscriber considers
    /// to be the current.
    ///
    /// If this returns `None`, then we are not currently within a span.
    ///
    /// [stored data]: crate::registry::SpanRef
    #[inline]
    #[must_use]
    pub fn lookup_current(&self) -> Option<SpanRef<'_, S>>
    where
        S: for<'lookup> LookupSpan<'lookup>,
    {
        self.ctx.lookup_current()
    }

    /// Returns the current span for this formatter.
    #[must_use]
    pub fn current_span(&self) -> Current {
        self.ctx.current_span()
    }

    /// Returns [stored data] for the parent span of the event currently being
    /// formatted.
    ///
    /// If the event has a contextual parent, this will return the current span. If
    /// the event has an explicit parent span, this will return that span. If
    /// the event does not have a parent span, this will return `None`.
    ///
    /// [stored data]: SpanRef
    #[must_use]
    pub fn parent_span(&self) -> Option<SpanRef<'_, S>> {
        self.ctx.event_span(self.event)
    }

    /// Returns an iterator over the [stored data] for all the spans in the
    /// current context, starting with the specified span and ending with the
    /// root of the trace tree and ending with the current span.
    ///
    /// This is equivalent to the [`Context::span_scope`] method.
    ///
    /// <div class="information">
    ///     <div class="tooltip ignore" style="">ⓘ<span class="tooltiptext">Note</span></div>
    /// </div>
    /// <div class="example-wrap" style="display:inline-block">
    /// <pre class="ignore" style="white-space:normal;font:inherit;">
    /// <strong>Note</strong>: Compared to <a href="#method.scope"><code>scope</code></a> this
    /// returns the spans in reverse order (from leaf to root). Use
    /// <a href="../registry/struct.Scope.html#method.root_to_leaf"><code>Scope::root_to_leaf</code></a>
    /// in case root-to-leaf ordering is desired.
    /// </pre></div>
    ///
    /// <div class="example-wrap" style="display:inline-block">
    /// <pre class="ignore" style="white-space:normal;font:inherit;">
    /// <strong>Note</strong>: This requires the wrapped subscriber to implement the
    /// <a href="../registry/trait.LookupSpan.html"><code>LookupSpan</code></a> trait.
    /// See the documentation on <a href="./struct.Context.html"><code>Context</code>'s
    /// declaration</a> for details.
    /// </pre></div>
    ///
    /// [stored data]: crate::registry::SpanRef
    #[must_use]
    pub fn span_scope(&self, id: Id) -> Option<registry::Scope<'_, S>>
    where
        S: for<'lookup> LookupSpan<'lookup>,
    {
        self.ctx.span_scope(id)
    }

    /// Returns an iterator over the [stored data] for all the spans in the
    /// event's span context, starting with its parent span and ending with the
    /// root of the trace tree.
    ///
    /// This is equivalent to calling the [`Context::event_scope`] method and
    /// passing the event currently being formatted.
    ///
    /// <div class="example-wrap" style="display:inline-block">
    /// <pre class="ignore" style="white-space:normal;font:inherit;">
    /// <strong>Note</strong>: Compared to <a href="#method.scope"><code>scope</code></a> this
    /// returns the spans in reverse order (from leaf to root). Use
    /// <a href="../registry/struct.Scope.html#method.root_to_leaf"><code>Scope::root_to_leaf</code></a>
    /// in case root-to-leaf ordering is desired.
    /// </pre></div>
    ///
    /// <div class="example-wrap" style="display:inline-block">
    /// <pre class="ignore" style="white-space:normal;font:inherit;">
    /// <strong>Note</strong>: This requires the wrapped subscriber to implement the
    /// <a href="../registry/trait.LookupSpan.html"><code>LookupSpan</code></a> trait.
    /// See the documentation on <a href="./struct.Context.html"><code>Context</code>'s
    /// declaration</a> for details.
    /// </pre></div>
    ///
    /// [stored data]: crate::registry::SpanRef
    #[must_use]
    pub fn event_scope(&self) -> Option<registry::Scope<'_, S>>
    where
        S: for<'lookup> LookupSpan<'lookup>,
    {
        self.ctx.event_scope(self.event)
    }

    /// Returns the [field formatter] configured by the subscriber invoking
    /// `format_event`.
    ///
    /// The event formatter may use the returned field formatter to format the
    /// fields of any events it records.
    ///
    /// [field formatter]: FormatFields
    #[must_use]
    pub const fn field_format(&self) -> &N {
        self.fmt_fields
    }
}

/// Tracks span busy and idle time for synthesized close events.
struct Timings {
    /// Accumulated idle time in nanoseconds.
    idle: u64,
    /// Accumulated busy time in nanoseconds.
    busy: u64,
    /// The last timing transition.
    last: Instant,
    /// Number of currently active enters.
    entered_count: u64,
}

impl Timings {
    /// Returns a new timing accumulator.
    #[allow(
        clippy::single_call_fn,
        reason = "span timing state has a named initializer with the current `Instant`"
    )]
    fn new() -> Self {
        Self {
            idle: 0,
            busy: 0,
            last: Instant::now(),
            entered_count: 0,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::fmt::{
        self,
        format::{self, test::MockTime, Format},
        layer::Layer as _,
        test::{MockMakeWriter, MockWriter},
        time,
    };
    use crate::{Registry, registry::LookupSpan, reload};
    use core::fmt::{Formatter, Result as FmtResult, Write as _};
    use format::FmtSpan;
    use regex::Regex;
    use std::fmt::{Debug as StdDebug, Error as FmtError};
    use strict_test_support::{TestFailure, ensure, ensure_contains, ensure_eq, ensure_ok};
    use tracing::field::Empty;
    use tracing::subscriber::with_default;
    use tracing_core::dispatcher::Dispatch;

    #[test]
    fn impls() {
        let uptime_format = Format::default().with_timer(time::Uptime::default());
        let uptime_layer = Layer::default().event_format(uptime_format);
        let uptime_subscriber = uptime_layer.with_subscriber(Registry::default());
        let _uptime_dispatch = Dispatch::new(uptime_subscriber);

        let default_format = Format::default();
        let default_layer = Layer::default().event_format(default_format);
        let default_subscriber = default_layer.with_subscriber(Registry::default());
        let _default_dispatch = Dispatch::new(default_subscriber);

        let compact_format = Format::default().compact();
        let compact_layer = Layer::default().event_format(compact_format);
        let compact_subscriber = compact_layer.with_subscriber(Registry::default());
        let _compact_dispatch = Dispatch::new(compact_subscriber);
    }

    #[test]
    fn fmt_layer_downcasts() -> Result<(), TestFailure> {
        let default_format = Format::default();
        let fmt_layer = Layer::default().event_format(default_format);
        let subscriber = fmt_layer.with_subscriber(Registry::default());

        let dispatch = Dispatch::new(subscriber);
        ensure(
            dispatch.downcast_ref::<Layer<Registry>>().is_some(),
            "dispatch downcasts to fmt layer",
        )
    }

    #[test]
    fn fmt_layer_downcasts_to_parts() -> Result<(), TestFailure> {
        let default_format = Format::default();
        let fmt_layer = Layer::default().event_format(default_format);
        let subscriber = fmt_layer.with_subscriber(Registry::default());
        let dispatch = Dispatch::new(subscriber);
        ensure(
            dispatch.downcast_ref::<format::DefaultFields>().is_some(),
            "dispatch downcasts to default fields",
        )?;
        ensure(
            dispatch.downcast_ref::<Format>().is_some(),
            "dispatch downcasts to default format",
        )
    }

    #[test]
    fn is_lookup_span() {
        fn assert_lookup_span<T: for<'a> LookupSpan<'a>>(_: T) {}
        let fmt = Layer::default();
        let subscriber = fmt.with_subscriber(Registry::default());
        assert_lookup_span(subscriber);
    }

    fn sanitize_timings(input: &str) -> Result<String, TestFailure> {
        let timing_pattern = ensure_ok(
            Regex::new("time\\.(idle|busy)=([0-9.]+)[m\u{b5}n]s"),
            "timing sanitizer regex compiles",
        )?;
        Ok(timing_pattern.replace_all(input, "timing").into_owned())
    }

    #[test]
    fn format_error_print_to_stderr() -> Result<(), TestFailure> {
        struct AlwaysError;

        impl StdDebug for AlwaysError {
            fn fmt(&self, _formatter: &mut Formatter<'_>) -> FmtResult {
                Err(FmtError)
            }
        }

        let make_writer = MockMakeWriter::default();
        let subscriber = fmt::Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .finish();

        with_default(subscriber, || {
            tracing::info!(?AlwaysError);
        });
        let actual = sanitize_timings(&make_writer.get_string())?;

        // Only assert the start because the line number and callsite may change.
        let expected = concat!(
            "Unable to format the following event. Name: event ",
            file!(),
            ":"
        );
        ensure(
            actual.as_str().starts_with(expected),
            "format errors are written to stderr",
        )
    }

    #[test]
    fn format_error_ignore_if_log_internal_errors_is_false() -> Result<(), TestFailure> {
        struct AlwaysError;

        impl StdDebug for AlwaysError {
            fn fmt(&self, _formatter: &mut Formatter<'_>) -> FmtResult {
                Err(FmtError)
            }
        }

        let make_writer = MockMakeWriter::default();
        let subscriber = fmt::Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .log_internal_errors(false)
            .finish();

        with_default(subscriber, || {
            tracing::info!(?AlwaysError);
        });
        let actual = sanitize_timings(&make_writer.get_string())?;
        ensure_eq(
            &actual.as_str(),
            &"",
            "format errors are ignored when internal logging is disabled",
        )
    }

    #[test]
    fn synthesize_span_none() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = fmt::Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            // check that FmtSpan::NONE is the default
            .finish();

        with_default(subscriber, || {
            let span1 = tracing::info_span!("span1", x = 42);
            let _entered = span1.enter();
        });
        let actual = sanitize_timings(&make_writer.get_string())?;
        ensure_eq(
            &actual.as_str(),
            &"",
            "span events default to not synthesizing output",
        )
    }

    /// Installs an fmt subscriber emitting `span_events` and compares synthesized output to `expected`.
    fn ensure_synthesized_span_events(
        span_events: FmtSpan,
        expected: &'static str,
        message: &'static str,
    ) -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = fmt::Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .with_span_events(span_events)
            .finish();

        with_default(subscriber, || {
            let span1 = tracing::info_span!("span1", x = 42);
            let _entered = span1.enter();
        });
        let actual = sanitize_timings(&make_writer.get_string())?;
        ensure_eq(&actual.as_str(), &expected, message)
    }

    #[test]
    fn synthesize_span_active() -> Result<(), TestFailure> {
        let expected = "fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: enter\n\
                        fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: exit\n";
        ensure_synthesized_span_events(FmtSpan::ACTIVE, expected, "active span events are synthesized")
    }

    #[test]
    fn synthesize_span_close() -> Result<(), TestFailure> {
        let expected =
            "fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: close timing timing\n";
        ensure_synthesized_span_events(FmtSpan::CLOSE, expected, "close span events include timings")
    }

    #[test]
    fn synthesize_span_close_no_timing() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = fmt::Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .without_time()
            .with_span_events(FmtSpan::CLOSE)
            .finish();

        with_default(subscriber, || {
            let span1 = tracing::info_span!("span1", x = 42);
            let _entered = span1.enter();
        });
        let actual = sanitize_timings(&make_writer.get_string())?;
        let expected = "span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: close\n";
        ensure_eq(
            &actual.as_str(),
            &expected,
            "close span events omit timings when time is disabled",
        )
    }

    #[test]
    fn synthesize_span_full() -> Result<(), TestFailure> {
        let expected = "fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: new\n\
                        fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: enter\n\
                        fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: exit\n\
                        fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: close timing timing\n";
        ensure_synthesized_span_events(FmtSpan::FULL, expected, "full span events are synthesized")
    }

    #[test]
    fn make_writer_based_on_meta() -> Result<(), TestFailure> {
        struct MakeByTarget {
            make_writer1: MockMakeWriter,
            make_writer2: MockMakeWriter,
        }

        impl<'a> MakeWriter<'a> for MakeByTarget {
            type Writer = MockWriter;

            fn make_writer(&'a self) -> Self::Writer {
                self.make_writer1.make_writer()
            }

            fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
                match meta.target() {
                    "writer2" => self.make_writer2.make_writer(),
                    _ => self.make_writer(),
                }
            }
        }

        let make_writer1 = MockMakeWriter::default();
        let make_writer2 = MockMakeWriter::default();

        let make_writer = MakeByTarget {
            make_writer1: make_writer1.clone(),
            make_writer2: make_writer2.clone(),
        };

        let subscriber = fmt::Subscriber::builder()
            .with_writer(make_writer)
            .with_level(false)
            .with_target(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .with_span_events(FmtSpan::CLOSE)
            .finish();

        with_default(subscriber, || {
            let span1 = tracing::info_span!("writer1_span", x = 42);
            let _entered_writer1 = span1.enter();
            tracing::info!(target: "writer2", "hello writer2!");
            let span2 = tracing::info_span!(target: "writer2", "writer2_span");
            let _entered_writer2 = span2.enter();
            tracing::warn!(target: "writer1", "hello writer1!");
        });

        let writer1_output = sanitize_timings(&make_writer1.get_string())?;
        let expected_writer1 = "fake time writer1_span{x=42}:writer2_span: hello writer1!\n\
                                fake time writer1_span{x=42}: close timing timing\n";
        ensure_eq(
            &writer1_output.as_str(),
            &expected_writer1,
            "default writer receives non-targeted output",
        )?;

        let writer2_output = sanitize_timings(&make_writer2.get_string())?;
        let expected_writer2 = "fake time writer1_span{x=42}: hello writer2!\n\
                                fake time writer1_span{x=42}:writer2_span: close timing timing\n";
        ensure_eq(
            &writer2_output.as_str(),
            &expected_writer2,
            "metadata writer receives targeted output",
        )
    }

    #[test]
    fn recorded_span_fields_are_appended_to_formatted_context() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = Layer::<Registry>::new()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .with_subscriber(Registry::default());

        with_default(subscriber, || {
            let span = tracing::info_span!("span1", first = 1, second = Empty);
            let _recorded_span = span.record("second", 2);
            let _entered = span.enter();
            tracing::info!("hello");
        });

        let expected =
            "fake time span1{first=1 second=2}: tracing_subscriber::fmt::fmt_layer::test: hello\n";
        ensure_eq(
            &make_writer.get_string().as_str(),
            &expected,
            "recorded span fields are appended with a separator",
        )
    }

    #[test]
    fn layer_writer_mutation_and_sanitization_settings_affect_output() -> Result<(), TestFailure> {
        let original_writer = MockMakeWriter::default();
        let replacement_writer = MockMakeWriter::default();
        let mut layer = Layer::<Registry>::new()
            .with_writer(original_writer.clone())
            .without_time()
            .with_level(false)
            .with_target(false)
            .with_file(false)
            .with_line_number(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_ansi(false)
            .with_ansi_sanitization(false)
            .log_internal_errors(false);

        ensure(
            !layer.flags.ansi_sanitization(),
            "builder disables ANSI sanitization in the layer configuration",
        )?;
        *layer.writer_mut() = replacement_writer.clone();

        let subscriber = layer.with_subscriber(Registry::default());
        with_default(subscriber, || {
            tracing::info!("\u{1b}[31mred\u{1b}[0m");
        });

        ensure_eq(
            &original_writer.get_string().as_str(),
            &"",
            "mutating the writer removes the original output destination",
        )?;
        let output = replacement_writer.get_string();
        ensure_contains(
            &output,
            "\u{1b}[31mred\u{1b}[0m",
            "disabled sanitization preserves trusted ANSI values",
        )
    }

    #[test]
    fn mapped_layer_components_transform_writer_fields_and_events() -> Result<(), TestFailure> {
        let original_writer = MockMakeWriter::default();
        let replacement_writer = MockMakeWriter::default();
        let field_formatter = format::debug_fn(
            |writer: &mut format::Writer<'_>, field: &field::Field, value: &dyn StdDebug| {
                write!(writer, "{}::{};", field.name(), format::DebugValue(value))
            },
        );

        let layer = Layer::<Registry>::new()
            .with_writer(original_writer.clone())
            .map_writer(|_writer| replacement_writer.clone())
            .map_event_format(|formatter| {
                formatter
                    .without_time()
                    .with_level(false)
                    .with_target(false)
                    .compact()
            })
            .fmt_fields(field_formatter)
            .map_fmt_fields(|formatter| formatter)
            .with_ansi(false);

        let subscriber = layer.with_subscriber(Registry::default());
        with_default(subscriber, || {
            tracing::info!(answer = 42, "mapped");
        });

        ensure_eq(
            &original_writer.get_string().as_str(),
            &"",
            "mapped writer bypasses the original destination",
        )?;
        let output = replacement_writer.get_string();
        ensure_contains(
            &output,
            "message::mapped;",
            "mapped field formatter controls message output",
        )?;
        ensure_contains(
            &output,
            "answer::42;",
            "mapped field formatter controls numeric output",
        )
    }

    // Because we need to modify an environment variable for these test cases,
    // we do them all in a single test.
    #[cfg(feature = "ansi")]
    #[test]
    fn layer_no_color() -> Result<(), TestFailure> {
        let cases = [
            (Some("0"), false),   // any non-empty value disables ansi
            (Some("off"), false), // any non-empty value disables ansi
            (Some("1"), false),
            (Some(""), true), // empty value does not disable ansi
            (None, true),
        ];

        for (var, expected_ansi) in cases {
            ensure_eq(
                &default_ansi_enabled(var),
                &expected_ansi,
                "default ansi state follows NO_COLOR",
            )?;

            let no_color_layer: Layer<()> = Layer::default_with_no_color(var);
            ensure_eq(
                &no_color_layer.flags.is_ansi(),
                &expected_ansi,
                "layer ansi state follows NO_COLOR",
            )?;

            // with_ansi should override any `NO_COLOR` value
            let overridden_layer: Layer<()> = Layer::default_with_no_color(var).with_ansi(true);
            ensure(overridden_layer.flags.is_ansi(), "with_ansi overrides NO_COLOR")?;

            // set_ansi should override any `NO_COLOR` value
            let mut mutable_layer: Layer<()> = Layer::default_with_no_color(var);
            mutable_layer.set_ansi(true);
            ensure(mutable_layer.flags.is_ansi(), "set_ansi overrides NO_COLOR")?;
        }
        Ok(())
    }

    // Validates that span event configuration can be modified with a reload handle
    #[test]
    fn modify_span_events() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();

        let inner_layer = Layer::default()
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .with_span_events(FmtSpan::ACTIVE);

        let (reloadable_layer, reload_handle) = reload::Layer::new(inner_layer);
        let reload = reloadable_layer.with_subscriber(Registry::default());

        with_default(reload, || -> Result<(), TestFailure> {
            {
                let span1 = tracing::info_span!("span1", x = 42);
                let _entered = span1.enter();
            }

            ensure_ok(
                reload_handle.modify(|layer| layer.set_span_events(FmtSpan::NONE)),
                "span event reload disables events",
            )?;

            // this span should not be logged at all!
            {
                let span2 = tracing::info_span!("span2", x = 100);
                let _entered = span2.enter();
            }

            {
                let span3 = tracing::info_span!("span3", x = 42);
                let _entered = span3.enter();

                // The span config was modified after span3 was already entered.
                // We should only see an exit
                ensure_ok(
                    reload_handle.modify(|layer| layer.set_span_events(FmtSpan::ACTIVE)),
                    "span event reload enables events",
                )?;
            };
            Ok(())
        })?;
        let actual = sanitize_timings(&make_writer.get_string())?;
        let expected = "fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: enter\n\
                        fake time span1{x=42}: tracing_subscriber::fmt::fmt_layer::test: exit\n\
                        fake time span3{x=42}: tracing_subscriber::fmt::fmt_layer::test: exit\n";
        ensure_eq(
            &actual.as_str(),
            &expected,
            "reloadable span events affect later spans",
        )
    }
}
