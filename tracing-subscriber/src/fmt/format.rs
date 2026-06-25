//! Formatters for logging [`tracing`] events.
//!
//! This module provides several formatter implementations, as well as utilities
//! for implementing custom formatters.
//!
//! # Formatters
//! This module provides a number of formatter implementations:
//!
//! * [`Full`]: The default formatter. This emits human-readable,
//!   single-line logs for each event that occurs, with the current span context
//!   displayed before the formatted representation of the event. See
//!   [here](Full#example-output) for sample output.
//!
//! * [`Compact`]: A variant of the default formatter, optimized for
//!   short line lengths. Fields from the current span context are appended to
//!   the fields of the formatted event, and span names are not shown; the
//!   verbosity level is abbreviated to a single character. See
//!   [here](Compact#example-output) for sample output.
//!
//! * [`Pretty`]: Emits excessively pretty, multi-line logs, optimized
//!   for human readability. This is primarily intended to be used in local
//!   development and debugging, or for command-line applications, where
//!   automated analysis and compact storage of logs is less of a priority than
//!   readability and visual appeal. See [here](Pretty#example-output)
//!   for sample output.
//!
//! * [`Json`]: Outputs newline-delimited JSON logs. This is intended
//!   for production use with systems where structured logs are consumed as JSON
//!   by analysis and viewing tools. The JSON output is not optimized for human
//!   readability. See [here](Json#example-output) for sample output.
use super::time::{FormatTime, SystemTime};
use crate::{
    field::{MakeOutput, MakeVisitor, RecordFields, VisitFmt, VisitOutput},
    fmt::fmt_layer::FmtContext,
    fmt::fmt_layer::FormattedFields,
    registry::{LookupSpan, Scope},
};

use std::{
    any,
    error::Error,
    fmt::{self, Debug, Display, Write},
    thread::{self, ThreadId},
};
use tracing_core::{
    field::{Field, Visit},
    span, Event, Level, Subscriber,
};

#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent as _;

#[cfg(feature = "ansi")]
use nu_ansi_term::{Color, Style};

/// ANSI escape sanitization helpers.
mod escape;
use escape::EscapeGuard;

#[cfg(feature = "json")]
/// JSON formatting support.
mod json;
#[cfg(feature = "json")]
#[cfg_attr(docsrs, doc(cfg(feature = "json")))]
pub use json::*;

#[cfg(feature = "ansi")]
/// Pretty ANSI formatting support.
mod pretty;
#[cfg(feature = "ansi")]
#[cfg_attr(docsrs, doc(cfg(feature = "ansi")))]
pub use pretty::*;

/// A type that can format a tracing [`Event`] to a [`Writer`].
///
/// [`FormatEvent`] is primarily used in the context of [`fmt::Subscriber`] or
/// [`fmt::Layer`]. Each time an event is dispatched to [`fmt::Subscriber`] or
/// [`fmt::Layer`], the subscriber or layer
/// forwards it to its associated [`FormatEvent`] to emit a log message.
///
/// This trait is already implemented for function pointers with the same
/// signature as `format_event`.
///
/// # Arguments
///
/// The following arguments are passed to [`FormatEvent::format_event`]:
///
/// * A [`FmtContext`]. This is an extension of the [`layer::Context`] type,
///   which can be used for accessing stored information such as the current
///   span context an event occurred in.
///
///   In addition, [`FmtContext`] exposes access to the [`FormatFields`]
///   implementation that the subscriber was configured to use via the
///   [`FmtContext::field_format`] method. This can be used when the
///   [`FormatEvent`] implementation needs to format the event's fields.
///
///   For convenience, [`FmtContext`] also implements [`FormatFields`],
///   forwarding to the configured [`FormatFields`] type.
///
/// * A [`Writer`] to which the formatted representation of the event is
///   written. This type implements the [`std::fmt::Write`] trait, and therefore
///   can be used with the [`std::write!`] and [`std::writeln!`] macros, as well
///   as calling [`std::fmt::Write`] methods directly.
///
///   The [`Writer`] type also implements additional methods that provide
///   information about how the event should be formatted. The
///   [`Writer::has_ansi_escapes`] method indicates whether [ANSI terminal
///   escape codes] are supported by the underlying I/O writer that the event
///   will be written to. If this returns `true`, the formatter is permitted to
///   use ANSI escape codes to add colors and other text formatting to its
///   output. If it returns `false`, the event will be written to an output that
///   does not support ANSI escape codes (such as a log file), and they should
///   not be emitted.
///
///   Crates like [`nu_ansi_term`] and [`owo-colors`] can be used to add ANSI
///   escape codes to formatted output.
///
/// * The actual [`Event`] to be formatted.
///
/// # Examples
///
/// This example re-implements a simplified version of this crate's [default
/// formatter]:
///
/// ```rust
/// use std::fmt::{self, Write};
/// use tracing_core::{Subscriber, Event};
/// use tracing_subscriber::fmt::{
///     format::{self, FormatEvent, FormatFields},
///     FmtContext,
///     FormattedFields,
/// };
/// use tracing_subscriber::registry::LookupSpan;
///
/// struct MyFormatter;
///
/// impl<S, N> FormatEvent<S, N> for MyFormatter
/// where
///     S: Subscriber + for<'a> LookupSpan<'a>,
///     N: for<'a> FormatFields<'a> + 'static,
/// {
///     fn format_event(
///         &self,
///         ctx: &FmtContext<'_, S, N>,
///         mut writer: format::Writer<'_>,
///         event: &Event<'_>,
///     ) -> fmt::Result {
///         // Format values from the event's's metadata:
///         let metadata = event.metadata();
///         write!(&mut writer, "{} {}: ", metadata.level(), metadata.target())?;
///
///         // Format all the spans in the event's span context.
///         if let Some(scope) = ctx.event_scope() {
///             for span in scope.root_to_leaf() {
///                 write!(writer, "{}", span.name())?;
///
///                 // `FormattedFields` is a formatted representation of the span's
///                 // fields, which is stored in its extensions by the `fmt` layer's
///                 // `new_span` method. The fields will have been formatted
///                 // by the same field formatter that's provided to the event
///                 // formatter in the `FmtContext`.
///                 let extensions = span.extensions();
///                 if let Some(fields) = extensions.get::<FormattedFields<N>>() {
///                     // Skip formatting the fields if the span had no fields.
///                     if !fields.is_empty() {
///                         write!(writer, "{{{}}}", fields)?;
///                     }
///                 }
///                 write!(writer, ": ")?;
///             }
///         }
///
///         // Write fields on the event
///         ctx.field_format().format_fields(writer.by_ref(), event)?;
///
///         writeln!(writer)
///     }
/// }
///
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// let _subscriber = tracing_subscriber::fmt()
///     .event_format(MyFormatter)
///     .try_init()?;
///
/// let _span = tracing::info_span!("my_span", answer = 42).entered();
/// tracing::info!(question = "life, the universe, and everything", "hello world");
/// # Ok(()) }
/// ```
///
/// This formatter will print events like this:
///
/// ```text
/// DEBUG yak_shaving::shaver: some-span{field-on-span=foo}: started shaving yak
/// ```
///
/// [`layer::Context`]: crate::layer::Context
/// [`fmt::Layer`]: super::Layer
/// [`fmt::Subscriber`]: super::Subscriber
/// [`Event`]: tracing::Event
/// [implements `FormatFields`]: super::FmtContext#impl-FormatFields<'writer>
/// [ANSI terminal escape codes]: https://en.wikipedia.org/wiki/ANSI_escape_code
/// [`Writer::has_ansi_escapes`]: Writer::has_ansi_escapes
/// [`nu_ansi_term`]: https://crates.io/crates/nu_ansi_term
/// [`owo-colors`]: https://crates.io/crates/owo-colors
/// [default formatter]: Full
pub trait FormatEvent<S, N>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    /// Write a log message for [`Event`] in [`FmtContext`] to the given [`Writer`].
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if writing the formatted event fails.
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result;
}

impl<S, N> FormatEvent<S, N>
    for fn(ctx: &FmtContext<'_, S, N>, Writer<'_>, &Event<'_>) -> fmt::Result
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        (*self)(ctx, writer, event)
    }
}
/// A type that can format a [set of fields] to a [`Writer`].
///
/// [`FormatFields`] is primarily used in the context of [`FmtSubscriber`]. Each
/// time a span or event with fields is recorded, the subscriber will format
/// those fields with its associated [`FormatFields`] implementation.
///
/// [set of fields]: crate::field::RecordFields
/// [`FmtSubscriber`]: super::Subscriber
pub trait FormatFields<'writer> {
    /// Format the provided `fields` to the provided [`Writer`], returning a result.
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if writing any field fails.
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result;

    /// Record additional field(s) on an existing span.
    ///
    /// By default, this appends a space to the current set of fields if it is
    /// non-empty, and then calls `self.format_fields`. If different behavior is
    /// required, the default implementation of this method can be overridden.
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if appending fields fails.
    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &span::Record<'_>,
    ) -> fmt::Result {
        if !current.fields().is_empty() {
            current.fields_mut().push(' ');
        }
        self.format_fields(current.as_writer(), fields)
    }
}

/// Returns the default configuration for an event formatter.
///
/// Methods on the returned event formatter can be used for further
/// configuration. For example:
///
/// ```rust
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// let format = tracing_subscriber::fmt::format()
///     .without_time()         // Don't include timestamps
///     .with_target(false)     // Don't include event targets.
///     .with_level(false)      // Don't include event levels.
///     .compact();             // Use a more compact, abbreviated format.
///
/// // Use the configured formatter when building a new subscriber.
/// tracing_subscriber::fmt()
///     .event_format(format)
///     .try_init()?;
/// # Ok(()) }
/// ```
#[must_use]
#[allow(
    clippy::single_call_fn,
    reason = "public factory is part of the documented formatting API"
)]
pub fn format() -> Format {
    Format::default()
}

/// Returns the default configuration for a JSON event formatter.
#[cfg(feature = "json")]
#[cfg_attr(docsrs, doc(cfg(feature = "json")))]
#[must_use]
#[allow(
    clippy::single_call_fn,
    reason = "public factory is part of the documented JSON formatting API"
)]
pub fn json() -> Format<Json> {
    format().json()
}

/// Returns a [`FormatFields`] implementation that formats fields using the
/// provided function or closure.
///
pub const fn debug_fn<F>(f: F) -> FieldFn<F>
where
    F: Fn(&mut Writer<'_>, &Field, &dyn Debug) -> fmt::Result + Clone,
{
    FieldFn(f)
}

/// A writer to which formatted representations of spans and events are written.
///
/// This type is provided as input to the [`FormatEvent::format_event`] and
/// [`FormatFields::format_fields`] methods, which will write formatted
/// representations of [`Event`]s and [fields] to the [`Writer`].
///
/// This type implements the [`std::fmt::Write`] trait, allowing it to be used
/// with any function that takes an instance of [`std::fmt::Write`].
/// Additionally, it can be used with the standard library's [`std::write!`] and
/// [`std::writeln!`] macros.
///
/// Additionally, a [`Writer`] may expose additional [`tracing`]-specific
/// information to the formatter implementation.
///
/// [fields]: tracing_core::field
pub struct Writer<'writer> {
    /// The underlying formatter receiving output.
    inner: &'writer mut dyn Write,
    // TODO(eliza): add ANSI support
    /// Whether ANSI escape sequences may be emitted.
    is_ansi: bool,
    /// Whether ANSI escape sequences in values should be sanitized.
    ansi_sanitization: bool,
}

/// A [`FormatFields`] implementation that formats fields by calling a function
/// or closure.
///
#[derive(Debug, Clone)]
pub struct FieldFn<F>(F);
/// The [visitor] produced by [`FieldFn`]'s [`MakeVisitor`] implementation.
///
/// [visitor]: super::super::field::Visit
/// [`MakeVisitor`]: super::super::field::MakeVisitor
pub struct FieldFnVisitor<'a, F> {
    /// The callback used to format each field.
    f: F,
    /// The writer receiving formatted fields.
    writer: Writer<'a>,
    /// The accumulated formatter result.
    result: fmt::Result,
}
/// Marker for [`Format`] that indicates that the compact log format should be used.
///
/// The compact format includes fields from all currently entered spans, after
/// the event's fields. Span fields are ordered (but not grouped) by
/// span, and span names are not shown. A more compact representation of the
/// event's [`Level`] is used, and additional information—such as the event's
/// target—is disabled by default.
///
/// # Example Output
///
/// <pre><font color="#4E9A06"><b>:;</b></font> <font color="#4E9A06">cargo</font> run --example fmt-compact
/// <font color="#4E9A06"><b>    Finished</b></font> dev [unoptimized + debuginfo] target(s) in 0.08s
/// <font color="#4E9A06"><b>     Running</b></font> `target/debug/examples/fmt-compact`
/// <font color="#AAAAAA">2022-02-17T19:51:05.809287Z </font><font color="#4E9A06"> INFO</font> <b>fmt_compact</b><font color="#AAAAAA">: preparing to shave yaks </font><i>number_of_yaks</i><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809367Z </font><font color="#4E9A06"> INFO</font> <b>shaving_yaks</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: shaving yaks </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809414Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>:<b>shave</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: hello! I&apos;m gonna shave a yak </font><i>excitement</i><font color="#AAAAAA">=&quot;yay!&quot; </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3 </font><font color="#AAAAAA"><i>yak</i></font><font color="#AAAAAA">=1</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809443Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>:<b>shave</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: yak shaved successfully </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3 </font><font color="#AAAAAA"><i>yak</i></font><font color="#AAAAAA">=1</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809477Z </font><font color="#3465A4">DEBUG</font> <b>shaving_yaks</b>: <b>yak_events</b><font color="#AAAAAA">: </font><i>yak</i><font color="#AAAAAA">=1 </font><i>shaved</i><font color="#AAAAAA">=true </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809500Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: </font><i>yaks_shaved</i><font color="#AAAAAA">=1 </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809531Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>:<b>shave</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: hello! I&apos;m gonna shave a yak </font><i>excitement</i><font color="#AAAAAA">=&quot;yay!&quot; </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3 </font><font color="#AAAAAA"><i>yak</i></font><font color="#AAAAAA">=2</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809554Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>:<b>shave</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: yak shaved successfully </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3 </font><font color="#AAAAAA"><i>yak</i></font><font color="#AAAAAA">=2</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809581Z </font><font color="#3465A4">DEBUG</font> <b>shaving_yaks</b>: <b>yak_events</b><font color="#AAAAAA">: </font><i>yak</i><font color="#AAAAAA">=2 </font><i>shaved</i><font color="#AAAAAA">=true </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809606Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: </font><i>yaks_shaved</i><font color="#AAAAAA">=2 </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809635Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>:<b>shave</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: hello! I&apos;m gonna shave a yak </font><i>excitement</i><font color="#AAAAAA">=&quot;yay!&quot; </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3 </font><font color="#AAAAAA"><i>yak</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809664Z </font><font color="#C4A000"> WARN</font> <b>shaving_yaks</b>:<b>shave</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: could not locate yak </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3 </font><font color="#AAAAAA"><i>yak</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809693Z </font><font color="#3465A4">DEBUG</font> <b>shaving_yaks</b>: <b>yak_events</b><font color="#AAAAAA">: </font><i>yak</i><font color="#AAAAAA">=3 </font><i>shaved</i><font color="#AAAAAA">=false </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809717Z </font><font color="#CC0000">ERROR</font> <b>shaving_yaks</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: failed to shave yak </font><i>yak</i><font color="#AAAAAA">=3 </font><i>error</i><font color="#AAAAAA">=missing yak </font><i>error.sources</i><font color="#AAAAAA">=[out of space, out of cash] </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809743Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks</b>: <b>fmt_compact::yak_shave</b><font color="#AAAAAA">: </font><i>yaks_shaved</i><font color="#AAAAAA">=2 </font><font color="#AAAAAA"><i>yaks</i></font><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-17T19:51:05.809768Z </font><font color="#4E9A06"> INFO</font> <b>fmt_compact</b><font color="#AAAAAA">: yak shaving completed </font><i>all_yaks_shaved</i><font color="#AAAAAA">=false</font>
///
/// </pre>
#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub struct Compact;

/// Marker for [`Format`] that indicates that the default log format should be used.
///
/// This formatter shows the span context before printing event data. Spans are
/// displayed including their names and fields.
///
/// # Example Output
///
/// <pre><font color="#4E9A06"><b>:;</b></font> <font color="#4E9A06">cargo</font> run --example fmt
/// <font color="#4E9A06"><b>    Finished</b></font> dev [unoptimized + debuginfo] target(s) in 0.08s
/// <font color="#4E9A06"><b>     Running</b></font> `target/debug/examples/fmt`
/// <font color="#AAAAAA">2022-02-15T18:40:14.289898Z </font><font color="#4E9A06"> INFO</font> fmt: preparing to shave yaks <i>number_of_yaks</i><font color="#AAAAAA">=3</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.289974Z </font><font color="#4E9A06"> INFO</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: shaving yaks</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290011Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">:</font><b>shave{</b><i>yak</i><font color="#AAAAAA">=1</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: hello! I&apos;m gonna shave a yak </font><i>excitement</i><font color="#AAAAAA">=&quot;yay!&quot;</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290038Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">:</font><b>shave{</b><i>yak</i><font color="#AAAAAA">=1</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: yak shaved successfully</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290070Z </font><font color="#3465A4">DEBUG</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: yak_events: </font><i>yak</i><font color="#AAAAAA">=1 </font><i>shaved</i><font color="#AAAAAA">=true</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290089Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: </font><i>yaks_shaved</i><font color="#AAAAAA">=1</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290114Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">:</font><b>shave{</b><i>yak</i><font color="#AAAAAA">=2</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: hello! I&apos;m gonna shave a yak </font><i>excitement</i><font color="#AAAAAA">=&quot;yay!&quot;</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290134Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">:</font><b>shave{</b><i>yak</i><font color="#AAAAAA">=2</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: yak shaved successfully</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290157Z </font><font color="#3465A4">DEBUG</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: yak_events: </font><i>yak</i><font color="#AAAAAA">=2 </font><i>shaved</i><font color="#AAAAAA">=true</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290174Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: </font><i>yaks_shaved</i><font color="#AAAAAA">=2</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290198Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">:</font><b>shave{</b><i>yak</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: hello! I&apos;m gonna shave a yak </font><i>excitement</i><font color="#AAAAAA">=&quot;yay!&quot;</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290222Z </font><font color="#C4A000"> WARN</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">:</font><b>shave{</b><i>yak</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: could not locate yak</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290247Z </font><font color="#3465A4">DEBUG</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: yak_events: </font><i>yak</i><font color="#AAAAAA">=3 </font><i>shaved</i><font color="#AAAAAA">=false</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290268Z </font><font color="#CC0000">ERROR</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: failed to shave yak </font><i>yak</i><font color="#AAAAAA">=3 </font><i>error</i><font color="#AAAAAA">=missing yak </font><i>error.sources</i><font color="#AAAAAA">=[out of space, out of cash]</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290287Z </font><font color="#75507B">TRACE</font> <b>shaving_yaks{</b><i>yaks</i><font color="#AAAAAA">=3</font><b>}</b><font color="#AAAAAA">: fmt::yak_shave: </font><i>yaks_shaved</i><font color="#AAAAAA">=2</font>
/// <font color="#AAAAAA">2022-02-15T18:40:14.290309Z </font><font color="#4E9A06"> INFO</font> fmt: yak shaving completed. <i>all_yaks_shaved</i><font color="#AAAAAA">=false</font>
/// </pre>
#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub struct Full;

/// A pre-configured event formatter.
///
/// You will usually want to use this as the [`FormatEvent`] for a [`FmtSubscriber`].
///
/// The default logging format, [`Full`] includes all fields in each event and its containing
/// spans. The [`Compact`] logging format is intended to produce shorter log
/// lines; it displays each event's fields, along with fields from the current
/// span context, but other information is abbreviated. The [`Pretty`] logging
/// format is an extra-verbose, multi-line human-readable logging format
/// intended for use in development.
///
/// [`FmtSubscriber`]: super::Subscriber
#[derive(Debug, Clone)]
pub struct Format<F = Full, T = SystemTime> {
    /// The concrete event format strategy.
    kind: F,
    /// Timestamp source used by the format strategy.
    pub(crate) timer: T,
    /// Deprecated per-format ANSI override, preserved for compatibility.
    pub(crate) ansi: Option<bool>,
    /// Display options shared by the concrete format strategies.
    pub(crate) display: FormatDisplay,
}

/// Display options shared by event formatters.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct FormatDisplay {
    /// Bitset of enabled display options.
    bits: u8,
}

// === impl Writer ===

impl<'writer> Writer<'writer> {
    // TODO(eliza): consider making this a public API?
    // We may not want to do that if we choose to expose specialized
    // constructors instead (e.g. `from_string` that stores whether the string
    // is empty...?)
    //(@kaifastromai) I suppose having dedicated constructors may have certain benefits
    // but I am not privy to the larger direction of tracing/subscriber.
    /// Create a new [`Writer`] from any type that implements [`fmt::Write`].
    ///
    /// The returned `Writer` value may be passed as an argument to methods
    /// such as [`Format::format_event`]. Since constructing a [`Writer`]
    /// mutably borrows the underlying [`fmt::Write`] instance, that value may
    /// be accessed again once the [`Writer`] is dropped. For example, if the
    /// value implementing [`fmt::Write`] is a [`String`], it will contain
    /// the formatted output of [`Format::format_event`], which may then be
    /// used for other purposes.
    ///
    /// [`String`]: alloc::string::String
    #[must_use]
    pub fn new(writer: &'writer mut impl Write) -> Self {
        let inner: &mut dyn Write = writer;
        Self {
            inner,
            is_ansi: false,
            ansi_sanitization: true,
        }
    }

    // TODO(eliza): consider making this a public API?
    /// Returns a copy of this writer with ANSI escape support set.
    #[cfg(feature = "ansi")]
    pub(crate) const fn with_ansi(self, is_ansi: bool) -> Self {
        Self { is_ansi, ..self }
    }

    // TODO(eliza): consider making this a public API?
    /// Returns this writer unchanged when ANSI escape support is unavailable.
    #[cfg(not(feature = "ansi"))]
    pub(crate) const fn with_ansi(self, _requested_ansi: bool) -> Self {
        self
    }

    /// Returns a copy of this writer with ANSI sanitization set.
    pub(crate) const fn with_ansi_sanitization(self, ansi_sanitization: bool) -> Self {
        Self {
            ansi_sanitization,
            ..self
        }
    }

    /// Return a new [`Writer`] that mutably borrows [`self`].
    ///
    /// This can be used to temporarily borrow a [`Writer`] to pass a new [`Writer`]
    /// to a function that takes a [`Writer`] by value, allowing the original writer
    /// to still be used once that function returns.
    pub fn by_ref(&mut self) -> Writer<'_> {
        let is_ansi = self.is_ansi;
        let ansi_sanitization = self.ansi_sanitization;
        let inner: &mut dyn Write = self;
        Writer {
            inner,
            is_ansi,
            ansi_sanitization,
        }
    }

    /// Returns `true` if [ANSI escape codes] may be used to add colors
    /// and other formatting when writing to this `Writer`.
    ///
    /// If this returns `false`, formatters should not emit ANSI escape codes.
    ///
    /// [ANSI escape codes]: https://en.wikipedia.org/wiki/ANSI_escape_code
    #[must_use]
    pub const fn has_ansi_escapes(&self) -> bool {
        self.is_ansi
    }

    /// Returns `true` if ANSI escape codes should be sanitized.
    #[must_use]
    pub const fn sanitizes_ansi_escapes(&self) -> bool {
        self.ansi_sanitization
    }

    /// Returns the bold style when ANSI output is enabled.
    #[cfg(feature = "ansi")]
    pub(in crate::fmt::format) fn bold(&self) -> Style {
        if self.is_ansi {
            return Style::new().bold();
        }

        Style::new()
    }

    /// Returns the dimmed style when ANSI output is enabled.
    #[cfg(feature = "ansi")]
    pub(in crate::fmt::format) fn dimmed(&self) -> Style {
        if self.is_ansi {
            return Style::new().dimmed();
        }

        Style::new()
    }

    /// Returns the italic style when ANSI output is enabled.
    #[cfg(feature = "ansi")]
    pub(in crate::fmt::format) fn italic(&self) -> Style {
        if self.is_ansi {
            return Style::new().italic();
        }

        Style::new()
    }
}

impl Write for Writer<'_> {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.write_str(s)
    }

    #[inline]
    fn write_char(&mut self, c: char) -> fmt::Result {
        self.inner.write_char(c)
    }

    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        self.inner.write_fmt(args)
    }
}

impl Debug for Writer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Writer")
            .field("writer", &format_args!("<&mut dyn fmt::Write>"))
            .field("is_ansi", &self.is_ansi)
            .field("ansi_sanitization", &self.ansi_sanitization)
            .finish()
    }
}

// === impl Format ===

impl Default for Format<Full, SystemTime> {
    fn default() -> Self {
        Self {
            kind: Full,
            timer: SystemTime,
            ansi: None,
            display: FormatDisplay::default(),
        }
    }
}

impl Default for FormatDisplay {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl FormatDisplay {
    /// Timestamp display flag.
    const TIMESTAMP: u8 = 1 << 0;
    /// Target display flag.
    const TARGET: u8 = 1 << 1;
    /// Level display flag.
    const LEVEL: u8 = 1 << 2;
    /// Thread ID display flag.
    const THREAD_ID: u8 = 1 << 3;
    /// Thread name display flag.
    const THREAD_NAME: u8 = 1 << 4;
    /// Source filename display flag.
    const FILENAME: u8 = 1 << 5;
    /// Source line number display flag.
    const LINE_NUMBER: u8 = 1 << 6;
    /// Default display options.
    const DEFAULT: Self = Self {
        bits: Self::TIMESTAMP | Self::TARGET | Self::LEVEL,
    };

    /// Returns whether `flag` is enabled.
    const fn contains(self, flag: u8) -> bool {
        self.bits & flag == flag
    }

    /// Returns a copy with `flag` set to `enabled`.
    const fn with_flag(mut self, flag: u8, enabled: bool) -> Self {
        if enabled {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
        self
    }

    /// Returns whether timestamps are shown.
    pub(crate) const fn timestamp(self) -> bool {
        self.contains(Self::TIMESTAMP)
    }

    /// Returns whether event targets are shown.
    pub(crate) const fn target(self) -> bool {
        self.contains(Self::TARGET)
    }

    /// Returns whether event levels are shown.
    pub(crate) const fn level(self) -> bool {
        self.contains(Self::LEVEL)
    }

    /// Returns whether thread IDs are shown.
    pub(crate) const fn thread_id(self) -> bool {
        self.contains(Self::THREAD_ID)
    }

    /// Returns whether thread names are shown.
    pub(crate) const fn thread_name(self) -> bool {
        self.contains(Self::THREAD_NAME)
    }

    /// Returns whether source file paths are shown.
    pub(crate) const fn filename(self) -> bool {
        self.contains(Self::FILENAME)
    }

    /// Returns whether source line numbers are shown.
    pub(crate) const fn line_number(self) -> bool {
        self.contains(Self::LINE_NUMBER)
    }

    /// Returns a copy with timestamp display set to `enabled`.
    pub(crate) const fn with_timestamp(self, enabled: bool) -> Self {
        self.with_flag(Self::TIMESTAMP, enabled)
    }

    /// Returns a copy with target display set to `enabled`.
    pub(crate) const fn with_target(self, enabled: bool) -> Self {
        self.with_flag(Self::TARGET, enabled)
    }

    /// Returns a copy with level display set to `enabled`.
    pub(crate) const fn with_level(self, enabled: bool) -> Self {
        self.with_flag(Self::LEVEL, enabled)
    }

    /// Returns a copy with thread ID display set to `enabled`.
    pub(crate) const fn with_thread_id(self, enabled: bool) -> Self {
        self.with_flag(Self::THREAD_ID, enabled)
    }

    /// Returns a copy with thread name display set to `enabled`.
    pub(crate) const fn with_thread_name(self, enabled: bool) -> Self {
        self.with_flag(Self::THREAD_NAME, enabled)
    }

    /// Returns a copy with source filename display set to `enabled`.
    pub(crate) const fn with_filename(self, enabled: bool) -> Self {
        self.with_flag(Self::FILENAME, enabled)
    }

    /// Returns a copy with source line-number display set to `enabled`.
    pub(crate) const fn with_line_number(self, enabled: bool) -> Self {
        self.with_flag(Self::LINE_NUMBER, enabled)
    }
}

impl<F, T> Format<F, T> {
    /// Use a less verbose output format.
    ///
    /// See [`Compact`].
    pub fn compact(self) -> Format<Compact, T> {
        Format {
            kind: Compact,
            timer: self.timer,
            ansi: self.ansi,
            display: self.display,
        }
    }

    /// Use an excessively pretty, human-readable output format.
    ///
    /// See [`Pretty`].
    ///
    /// Note that this requires the `"ansi"` feature to be enabled.
    ///
    /// # Options
    ///
    /// [`Format::with_ansi`] can be used to disable ANSI terminal escape codes (which enable
    /// formatting such as colors, bold, italic, etc) in event formatting. However, a field
    /// formatter must be manually provided to avoid ANSI in the formatting of parent spans, like
    /// so:
    ///
    /// ```
    /// # use tracing_subscriber::fmt::format;
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// tracing_subscriber::fmt()
    ///    .pretty()
    ///    .with_ansi(false)
    ///    .fmt_fields(format::PrettyFields::new().with_ansi(false))
    ///    // ... other settings ...
    ///    .try_init()?;
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "ansi")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ansi")))]
    pub fn pretty(self) -> Format<Pretty, T> {
        let display = self.display.with_filename(true).with_line_number(true);

        Format {
            kind: Pretty::default(),
            timer: self.timer,
            ansi: self.ansi,
            display,
        }
    }

    /// Use the full JSON format.
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
    /// - [`Format::flatten_event`] can be used to enable flattening event fields into the root
    ///   object.
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub fn json(self) -> Format<Json, T> {
        Format {
            kind: Json::default(),
            timer: self.timer,
            ansi: self.ansi,
            display: self.display,
        }
    }

    /// Use the given [`timer`] for log message timestamps.
    ///
    /// See [`time` module] for the provided timer implementations.
    ///
    /// Note that using the `"time"` feature flag enables the
    /// additional time formatters [`UtcTime`] and [`LocalTime`], which use the
    /// [`time` crate] to provide more sophisticated timestamp formatting
    /// options.
    ///
    /// [`timer`]: super::time::FormatTime
    /// [`time` module]: mod@super::time
    /// [`UtcTime`]: super::time::UtcTime
    /// [`LocalTime`]: super::time::LocalTime
    /// [`time` crate]: https://docs.rs/time/0.3
    pub fn with_timer<T2>(self, timer: T2) -> Format<F, T2> {
        Format {
            kind: self.kind,
            timer,
            ansi: self.ansi,
            display: self.display,
        }
    }

    /// Do not emit timestamps with log messages.
    pub fn without_time(self) -> Format<F, ()> {
        let display = self.display.with_timestamp(false);

        Format {
            kind: self.kind,
            timer: (),
            ansi: self.ansi,
            display,
        }
    }

    /// Enable ANSI terminal colors for formatted output.
    #[must_use]
    pub fn with_ansi(self, ansi: bool) -> Self {
        Self {
            ansi: Some(ansi),
            ..self
        }
    }

    /// Sets whether or not an event's target is displayed.
    #[must_use]
    pub fn with_target(self, display_target: bool) -> Self {
        let display = self.display.with_target(display_target);
        Self { display, ..self }
    }

    /// Sets whether or not an event's level is displayed.
    #[must_use]
    pub fn with_level(self, display_level: bool) -> Self {
        let display = self.display.with_level(display_level);
        Self { display, ..self }
    }

    /// Sets whether or not the [thread ID] of the current thread is displayed
    /// when formatting events.
    ///
    /// [thread ID]: std::thread::ThreadId
    #[must_use]
    pub fn with_thread_ids(self, display_thread_id: bool) -> Self {
        let display = self.display.with_thread_id(display_thread_id);
        Self { display, ..self }
    }

    /// Sets whether or not the [name] of the current thread is displayed
    /// when formatting events.
    ///
    /// [name]: std::thread#naming-threads
    #[must_use]
    pub fn with_thread_names(self, display_thread_name: bool) -> Self {
        let display = self.display.with_thread_name(display_thread_name);
        Self { display, ..self }
    }

    /// Sets whether or not an event's [source code file path][file] is
    /// displayed.
    ///
    /// [file]: tracing_core::Metadata::file
    #[must_use]
    pub fn with_file(self, display_filename: bool) -> Self {
        let display = self.display.with_filename(display_filename);
        Self { display, ..self }
    }

    /// Sets whether or not an event's [source code line number][line] is
    /// displayed.
    ///
    /// [line]: tracing_core::Metadata::line
    #[must_use]
    pub fn with_line_number(self, display_line_number: bool) -> Self {
        let display = self.display.with_line_number(display_line_number);
        Self { display, ..self }
    }

    /// Sets whether or not the source code location from which an event
    /// originated is displayed.
    ///
    /// This is equivalent to calling [`Format::with_file`] and
    /// [`Format::with_line_number`] with the same value.
    #[must_use]
    pub fn with_source_location(self, display_location: bool) -> Self {
        self.with_line_number(display_location)
            .with_file(display_location)
    }

    #[inline]
    /// Formats the timestamp prefix if timestamp display is enabled.
    fn format_timestamp(&self, writer: &mut Writer<'_>) -> fmt::Result
    where
        T: FormatTime,
    {
        // If timestamps are disabled, do nothing.
        if !self.display.timestamp() {
            return Ok(());
        }

        // If ANSI color codes are enabled, format the timestamp with ANSI
        // colors.
        #[cfg(feature = "ansi")]
        {
            if writer.has_ansi_escapes() {
                let style = Style::new().dimmed();
                write!(writer, "{}", style.prefix())?;

                // If getting the timestamp failed, don't bail --- only bail on
                // formatting errors.
                if self.timer.format_time(writer).is_err() {
                    writer.write_str("<unknown time>")?;
                }

                write!(writer, "{} ", style.suffix())?;
                return Ok(());
            }
        }

        // Otherwise, just format the timestamp without ANSI formatting.
        // If getting the timestamp failed, don't bail --- only bail on
        // formatting errors.
        if self.timer.format_time(writer).is_err() {
            writer.write_str("<unknown time>")?;
        }
        writer.write_char(' ')
    }
}

#[cfg(feature = "json")]
#[cfg_attr(docsrs, doc(cfg(feature = "json")))]
impl<T> Format<Json, T> {
    /// Use the full JSON format with the event's event fields flattened.
    ///
    /// # Example Output
    ///
    /// ```ignore,json
    /// {"timestamp":"Feb 20 11:28:15.096","level":"INFO","target":"mycrate", "message":"some message", "key": "value"}
    /// ```
    /// See [`Json`].
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    #[must_use]
    pub const fn flatten_event(mut self, flatten_event: bool) -> Self {
        self.kind.flatten_event(flatten_event);
        self
    }

    /// Sets whether or not the formatter will include the current span in
    /// formatted events.
    ///
    /// See [`format::Json`][Json]
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    #[must_use]
    pub const fn with_current_span(mut self, display_current_span: bool) -> Self {
        self.kind.with_current_span(display_current_span);
        self
    }

    /// Sets whether or not the formatter will include a list (from root to
    /// leaf) of all currently entered spans in formatted events.
    ///
    /// See [`format::Json`][Json]
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    #[must_use]
    pub const fn with_span_list(mut self, display_span_list: bool) -> Self {
        self.kind.with_span_list(display_span_list);
        self
    }
}

/// Displays a value using its [`Debug`] implementation.
pub(super) struct DebugValue<'a>(
    /// The value to format with [`Debug`].
    pub(super) &'a dyn Debug,
);

impl Display for DebugValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(self.0, f)
    }
}

/// Displays a thread identifier with the standard [`Debug`] representation.
pub(super) struct FmtThreadId(ThreadId);

impl FmtThreadId {
    /// Returns a display adapter for `thread_id`.
    pub(super) const fn new(thread_id: ThreadId) -> Self {
        Self(thread_id)
    }
}

impl Display for FmtThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

/// Writes the configured thread-name and thread-id prefix.
fn write_thread_context(display: FormatDisplay, writer: &mut Writer<'_>) -> fmt::Result {
    if display.thread_name() {
        let current_thread = thread::current();
        match current_thread.name() {
            Some(name) => {
                write!(writer, "{} ", FmtThreadName::new(name))?;
            }
            // fall back to thread id when name is absent and ids are not enabled
            None if !display.thread_id() => {
                write!(writer, "{} ", FmtThreadId::new(current_thread.id()))?;
            }
            _ => {}
        }
    }

    if display.thread_id() {
        write!(writer, "{} ", FmtThreadId::new(thread::current().id()))?;
    }

    Ok(())
}

impl<S, N, T> FormatEvent<S, N> for Format<Full, T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        #[cfg(feature = "tracing-log")]
        let normalized = event.normalized_metadata();
        #[cfg(feature = "tracing-log")]
        let normalized_meta = normalized.as_ref().map(|meta| meta.as_metadata());
        #[cfg(feature = "tracing-log")]
        let meta = normalized_meta.as_ref().unwrap_or_else(|| event.metadata());
        #[cfg(not(feature = "tracing-log"))]
        let meta = event.metadata();

        // if the `Format` struct *also* has an ANSI color configuration,
        // override the writer...the API for configuring ANSI color codes on the
        // `Format` struct is deprecated, but we still need to honor those
        // configurations.
        if let Some(ansi) = self.ansi {
            writer = writer.with_ansi(ansi);
        }

        self.format_timestamp(&mut writer)?;

        if self.display.level() {
            let fmt_level = {
                #[cfg(feature = "ansi")]
                {
                    FmtLevel::new(meta.level(), writer.has_ansi_escapes())
                }
                #[cfg(not(feature = "ansi"))]
                {
                    FmtLevel::new(meta.level())
                }
            };
            write!(writer, "{fmt_level} ")?;
        }

        write_thread_context(self.display, &mut writer)?;

        let dimmed = {
            #[cfg(feature = "ansi")]
            {
                writer.dimmed()
            }
            #[cfg(not(feature = "ansi"))]
            {
                Style::new()
            }
        };

        if let Some(scope) = ctx.event_scope() {
            let bold = {
                #[cfg(feature = "ansi")]
                {
                    writer.bold()
                }
                #[cfg(not(feature = "ansi"))]
                {
                    Style::new()
                }
            };

            let mut seen = false;

            for span in scope.root_to_leaf() {
                write!(writer, "{}", bold.paint(span.metadata().name()))?;
                seen = true;

                {
                    let ext = span.extensions();
                    if let Some(fields) = ext.get::<FormattedFields<N>>()
                        && !fields.is_empty()
                    {
                        write!(writer, "{}{}{}", bold.paint("{"), fields, bold.paint("}"))?;
                    }
                }
                write!(writer, "{}", dimmed.paint(":"))?;
            }

            if seen {
                writer.write_char(' ')?;
            }
        }

        if self.display.target() {
            write!(
                writer,
                "{}{} ",
                dimmed.paint(meta.target()),
                dimmed.paint(":")
            )?;
        }

        let maybe_line_number = if self.display.line_number() {
            meta.line()
        } else {
            None
        };

        if self.display.filename()
            && let Some(filename) = meta.file()
        {
            write!(
                writer,
                "{}{}{}",
                dimmed.paint(filename),
                dimmed.paint(":"),
                if maybe_line_number.is_some() { "" } else { " " }
            )?;
        }

        if let Some(event_line_number) = maybe_line_number {
            write!(
                writer,
                "{}{}:{} ",
                dimmed.prefix(),
                event_line_number,
                dimmed.suffix()
            )?;
        }

        ctx.format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

impl<S, N, T> FormatEvent<S, N> for Format<Compact, T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        #[cfg(feature = "tracing-log")]
        let normalized = event.normalized_metadata();
        #[cfg(feature = "tracing-log")]
        let normalized_meta = normalized.as_ref().map(|meta| meta.as_metadata());
        #[cfg(feature = "tracing-log")]
        let meta = normalized_meta.as_ref().unwrap_or_else(|| event.metadata());
        #[cfg(not(feature = "tracing-log"))]
        let meta = event.metadata();

        // if the `Format` struct *also* has an ANSI color configuration,
        // override the writer...the API for configuring ANSI color codes on the
        // `Format` struct is deprecated, but we still need to honor those
        // configurations.
        if let Some(ansi) = self.ansi {
            writer = writer.with_ansi(ansi);
        }

        self.format_timestamp(&mut writer)?;

        if self.display.level() {
            let fmt_level = {
                #[cfg(feature = "ansi")]
                {
                    FmtLevel::new(meta.level(), writer.has_ansi_escapes())
                }
                #[cfg(not(feature = "ansi"))]
                {
                    FmtLevel::new(meta.level())
                }
            };
            write!(writer, "{fmt_level} ")?;
        }

        write_thread_context(self.display, &mut writer)?;

        let fmt_ctx = {
            #[cfg(feature = "ansi")]
            {
                FmtCtx::new(ctx, event.parent(), writer.has_ansi_escapes())
            }
            #[cfg(not(feature = "ansi"))]
            {
                FmtCtx::new(ctx, event.parent())
            }
        };
        write!(writer, "{fmt_ctx}")?;

        let dimmed = {
            #[cfg(feature = "ansi")]
            {
                writer.dimmed()
            }
            #[cfg(not(feature = "ansi"))]
            {
                Style::new()
            }
        };

        let mut needs_space = self.display.target();
        if needs_space {
            write!(
                writer,
                "{}{}",
                dimmed.paint(meta.target()),
                dimmed.paint(":")
            )?;
        }

        if self.display.filename()
            && let Some(filename) = meta.file()
        {
            if self.display.target() {
                writer.write_char(' ')?;
            }
            write!(writer, "{}{}", dimmed.paint(filename), dimmed.paint(":"))?;
            needs_space = true;
        }

        if self.display.line_number()
            && let Some(line_number) = meta.line()
        {
            write!(
                writer,
                "{}{}{}{}",
                dimmed.prefix(),
                line_number,
                dimmed.suffix(),
                dimmed.paint(":")
            )?;
            needs_space = true;
        }

        if needs_space {
            writer.write_char(' ')?;
        }

        ctx.format_fields(writer.by_ref(), event)?;

        for span in ctx
            .event_scope()
            .into_iter()
            .flat_map(Scope::root_to_leaf)
        {
            let exts = span.extensions();
            if let Some(fields) = exts.get::<FormattedFields<N>>()
                && !fields.is_empty()
            {
                write!(writer, " {}", dimmed.paint(fields.fields()))?;
            }
        }
        writeln!(writer)
    }
}

// === impl FormatFields ===
impl<'writer, M> FormatFields<'writer> for M
where
    M: MakeOutput<Writer<'writer>, fmt::Result>,
    M::Visitor: VisitFmt + VisitOutput<fmt::Result>,
{
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut visitor = self.make_visitor(writer);
        fields.record(&mut visitor);
        visitor.finish()
    }
}

/// The default [`FormatFields`] implementation.
///
#[derive(Copy, Clone, Debug)]
pub struct DefaultFields {
    // reserve the ability to add fields to this without causing a breaking
    // change in the future.
    /// Prevents external construction and reserves room for future fields.
    _private: (),
}

/// The [visitor] produced by [`DefaultFields`]'s [`MakeVisitor`] implementation.
///
/// [visitor]: super::super::field::Visit
/// [`MakeVisitor`]: super::super::field::MakeVisitor
#[derive(Debug)]
pub struct DefaultVisitor<'a> {
    /// The writer receiving formatted fields.
    writer: Writer<'a>,
    /// Whether no fields have been written yet.
    is_empty: bool,
    /// The first formatting error encountered while visiting fields.
    result: fmt::Result,
}

impl DefaultFields {
    /// Returns a new default [`FormatFields`] implementation.
    #[must_use]
    #[allow(
        clippy::single_call_fn,
        reason = "public constructor is part of the documented default field formatter API"
    )]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for DefaultFields {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> MakeVisitor<Writer<'a>> for DefaultFields {
    type Visitor = DefaultVisitor<'a>;

    #[inline]
    fn make_visitor(&self, target: Writer<'a>) -> Self::Visitor {
        DefaultVisitor::new(target, true)
    }
}

// === impl DefaultVisitor ===

impl<'a> DefaultVisitor<'a> {
    /// Returns a new default visitor that formats to the provided `writer`.
    ///
    /// # Arguments
    /// - `writer`: the writer to format to.
    /// - `is_empty`: whether or not any fields have been previously written to
    ///   that writer.
    #[must_use]
    #[allow(
        clippy::single_call_fn,
        reason = "public visitor constructor is part of the default field formatting API"
    )]
    pub const fn new(writer: Writer<'a>, is_empty: bool) -> Self {
        Self {
            writer,
            is_empty,
            result: Ok(()),
        }
    }

    /// Writes field padding when the visitor has already emitted a field.
    fn maybe_pad(&mut self) {
        if self.is_empty {
            self.is_empty = false;
        } else {
            self.result = write!(self.writer, " ");
        }
    }
}

impl Visit for DefaultVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if self.result.is_err() {
            return;
        }

        if field.name() == "message" {
            self.record_debug(field, &format_args!("{value}"));
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        let sanitize = self.writer.sanitizes_ansi_escapes();
        if let Some(source) = value.source() {
            let italic = {
                #[cfg(feature = "ansi")]
                {
                    self.writer.italic()
                }
                #[cfg(not(feature = "ansi"))]
                {
                    Style::new()
                }
            };
            let dimmed = {
                #[cfg(feature = "ansi")]
                {
                    self.writer.dimmed()
                }
                #[cfg(not(feature = "ansi"))]
                {
                    Style::new()
                }
            };
            self.record_debug(
                field,
                &format_args!(
                    "{} {}{}{}{}",
                    EscapeGuard::new(format_args!("{value}"), sanitize),
                    italic.paint(field.name()),
                    italic.paint(".sources"),
                    dimmed.paint("="),
                    ErrorSourceList::new(source, sanitize)
                ),
            );
        } else {
            self.record_debug(field, &EscapeGuard::new(format_args!("{value}"), sanitize));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if self.result.is_err() {
            return;
        }

        let name = field.name();
        // Skip fields that are actually log metadata that have already been handled
        #[cfg(feature = "tracing-log")]
        if name.starts_with("log.") {
            self.result = Ok(());
            return;
        }

        // emit separating spaces if needed
        self.maybe_pad();

        self.result = if name == "message" {
            // Escape ANSI characters to prevent malicious patterns (e.g., terminal injection attacks)
            write!(
                self.writer,
                "{}",
                EscapeGuard::new(DebugValue(value), self.writer.sanitizes_ansi_escapes())
            )
        } else {
            let italic = {
                #[cfg(feature = "ansi")]
                {
                    self.writer.italic()
                }
                #[cfg(not(feature = "ansi"))]
                {
                    Style::new()
                }
            };
            let dimmed = {
                #[cfg(feature = "ansi")]
                {
                    self.writer.dimmed()
                }
                #[cfg(not(feature = "ansi"))]
                {
                    Style::new()
                }
            };

            if let Some(raw_name) = name.strip_prefix("r#") {
                write!(
                    self.writer,
                    "{}{}{}",
                    italic.paint(raw_name),
                    dimmed.paint("="),
                    DebugValue(value)
                )
            } else {
                write!(
                    self.writer,
                    "{}{}{}",
                    italic.paint(name),
                    dimmed.paint("="),
                    DebugValue(value)
                )
            }
        };
    }
}

impl VisitOutput<fmt::Result> for DefaultVisitor<'_> {
    fn finish(self) -> fmt::Result {
        self.result
    }
}

impl VisitFmt for DefaultVisitor<'_> {
    fn writer(&mut self) -> &mut dyn Write {
        &mut self.writer
    }
}

/// Renders an error into a list of sources, *including* the error.
struct ErrorSourceList<'a> {
    /// The first error to include in the rendered source chain.
    error: &'a (dyn Error + 'static),
    /// Whether ANSI escape sequences are sanitized while rendering errors.
    ansi_sanitization: bool,
}

impl<'a> ErrorSourceList<'a> {
    /// Returns a display adapter for `error` and its source chain.
    #[allow(
        clippy::single_call_fn,
        reason = "shared error-source display adapter centralizes construction for compact and pretty formatters"
    )]
    fn new(error: &'a (dyn Error + 'static), ansi_sanitization: bool) -> Self {
        Self {
            error,
            ansi_sanitization,
        }
    }
}

impl Display for ErrorSourceList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        let mut curr = Some(self.error);
        while let Some(curr_err) = curr {
            let _list = list.entry(&EscapeGuard::new(
                format_args!("{curr_err}"),
                self.ansi_sanitization,
            ));
            curr = curr_err.source();
        }
        list.finish()
    }
}

/// Formats compact span context before event fields.
struct FmtCtx<'a, S, N> {
    /// The formatting context used to look up spans and their fields.
    ctx: &'a FmtContext<'a, S, N>,
    /// The explicit parent span for the event, when one was recorded.
    span: Option<&'a span::Id>,
    /// Whether ANSI styling is enabled for this writer.
    #[cfg(feature = "ansi")]
    ansi: bool,
}

impl<'a, S, N> FmtCtx<'a, S, N>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    #[cfg(feature = "ansi")]
    /// Returns a span-context formatter.
    #[allow(
        clippy::single_call_fn,
        reason = "span-context display adapter keeps ANSI-specific construction localized"
    )]
    const fn new(
        ctx: &'a FmtContext<'_, S, N>,
        span: Option<&'a span::Id>,
        ansi: bool,
    ) -> Self {
        Self { ctx, span, ansi }
    }

    /// Returns a span-context formatter.
    #[cfg(not(feature = "ansi"))]
    #[allow(
        clippy::single_call_fn,
        reason = "span-context display adapter keeps construction localized without ANSI fields"
    )]
    const fn new(ctx: &'a FmtContext<'_, S, N>, span: Option<&'a span::Id>) -> Self {
        Self { ctx, span }
    }

    /// Returns bold styling when ANSI support is enabled.
    #[cfg(feature = "ansi")]
    fn bold(&self) -> Style {
        if self.ansi {
            return Style::new().bold();
        }

        Style::new()
    }
}

impl<S, N> Display for FmtCtx<'_, S, N>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bold = {
            #[cfg(feature = "ansi")]
            {
                self.bold()
            }
            #[cfg(not(feature = "ansi"))]
            {
                Style::new()
            }
        };
        let mut seen = false;

        let current_span = self
            .span
            .copied()
            .and_then(|id| self.ctx.ctx.span(id))
            .or_else(|| self.ctx.ctx.lookup_current());

        let scope = current_span
            .into_iter()
            .flat_map(|span_ref| span_ref.scope().root_to_leaf());

        for span_ref in scope {
            seen = true;
            write!(f, "{}:", bold.paint(span_ref.metadata().name()))?;
        }

        if seen {
            f.write_char(' ')?;
        }
        Ok(())
    }
}

#[cfg(not(feature = "ansi"))]
/// No-op styling adapter used when ANSI support is disabled.
struct Style {
    /// The no-op prefix emitted before styled values.
    prefix: &'static str,
    /// The no-op suffix emitted after styled values.
    suffix: &'static str,
}

#[cfg(not(feature = "ansi"))]
impl Style {
    /// Returns a no-op style.
    const fn new() -> Self {
        Self {
            prefix: "",
            suffix: "",
        }
    }

    /// Returns `display` without adding style codes.
    const fn paint<D>(&self, display: D) -> StyledDisplay<D> {
        StyledDisplay {
            prefix: self.prefix,
            display,
            suffix: self.suffix,
        }
    }

    /// Returns the no-op style prefix.
    const fn prefix(&self) -> &'static str {
        self.prefix
    }

    /// Returns the no-op style suffix.
    const fn suffix(&self) -> &'static str {
        self.suffix
    }
}

#[cfg(not(feature = "ansi"))]
/// Display adapter for no-op styled values.
struct StyledDisplay<D> {
    /// The prefix emitted before the value.
    prefix: &'static str,
    /// The value being displayed.
    display: D,
    /// The suffix emitted after the value.
    suffix: &'static str,
}

#[cfg(not(feature = "ansi"))]
impl<D> Display for StyledDisplay<D>
where
    D: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.prefix)?;
        Display::fmt(&self.display, f)?;
        f.write_str(self.suffix)
    }
}

/// Displays thread names with stable alignment.
struct FmtThreadName<'a> {
    /// The thread name to render.
    name: &'a str,
}

impl<'a> FmtThreadName<'a> {
    /// Returns a display adapter for `name`.
    #[allow(
        clippy::single_call_fn,
        reason = "thread-name formatting uses a named display adapter for stable alignment"
    )]
    const fn new(name: &'a str) -> Self {
        Self { name }
    }
}

impl Display for FmtThreadName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::sync::atomic::{
            AtomicUsize,
            Ordering::{AcqRel, Acquire, Relaxed},
        };

        // Track the longest thread name length we've seen so far in an atomic,
        // so that it can be updated by any thread.
        static MAX_LEN: AtomicUsize = AtomicUsize::new(0);
        let len = self.name.len();
        // Snapshot the current max thread name length.
        let mut max_len = MAX_LEN.load(Relaxed);

        while len > max_len {
            // Try to set a new max length, if it is still the value we took a
            // snapshot of.
            match MAX_LEN.compare_exchange(max_len, len, AcqRel, Acquire) {
                // We successfully set the new max value
                Ok(_) => break,
                // Another thread set a new max value since we last observed
                // it! It's possible that the new length is actually longer than
                // ours, so we'll loop again and check whether our length is
                // still the longest. If not, we'll just use the newer value.
                Err(actual) => max_len = actual,
            }
        }

        // pad thread name using `max_len`
        write!(f, "{:>width$}", self.name, width = max_len)
    }
}

/// Displays event levels with optional ANSI styling.
struct FmtLevel<'a> {
    /// The level to render.
    level: &'a Level,
    /// Whether ANSI styling is enabled.
    #[cfg(feature = "ansi")]
    ansi: bool,
}

impl<'a> FmtLevel<'a> {
    #[cfg(feature = "ansi")]
    /// Returns a level formatter with explicit ANSI support.
    pub(super) const fn new(level: &'a Level, ansi: bool) -> Self {
        Self { level, ansi }
    }

    /// Returns a level formatter.
    #[cfg(not(feature = "ansi"))]
    pub(super) const fn new(level: &'a Level) -> Self {
        Self { level }
    }
}

/// Trace-level display text.
const TRACE_STR: &str = "TRACE";
/// Debug-level display text.
const DEBUG_STR: &str = "DEBUG";
/// Info-level display text.
const INFO_STR: &str = " INFO";
/// Warn-level display text.
const WARN_STR: &str = " WARN";
/// Error-level display text.
const ERROR_STR: &str = "ERROR";

#[cfg(not(feature = "ansi"))]
impl Display for FmtLevel<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self.level {
            Level::TRACE => f.pad(TRACE_STR),
            Level::DEBUG => f.pad(DEBUG_STR),
            Level::INFO => f.pad(INFO_STR),
            Level::WARN => f.pad(WARN_STR),
            Level::ERROR => f.pad(ERROR_STR),
        }
    }
}

#[cfg(feature = "ansi")]
impl Display for FmtLevel<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ansi {
            match *self.level {
                Level::TRACE => Display::fmt(&Color::Purple.paint(TRACE_STR), f),
                Level::DEBUG => Display::fmt(&Color::Blue.paint(DEBUG_STR), f),
                Level::INFO => Display::fmt(&Color::Green.paint(INFO_STR), f),
                Level::WARN => Display::fmt(&Color::Yellow.paint(WARN_STR), f),
                Level::ERROR => Display::fmt(&Color::Red.paint(ERROR_STR), f),
            }
        } else {
            match *self.level {
                Level::TRACE => f.pad(TRACE_STR),
                Level::DEBUG => f.pad(DEBUG_STR),
                Level::INFO => f.pad(INFO_STR),
                Level::WARN => f.pad(WARN_STR),
                Level::ERROR => f.pad(ERROR_STR),
            }
        }
    }
}

// === impl FieldFn ===

impl<'a, F> MakeVisitor<Writer<'a>> for FieldFn<F>
where
    F: Fn(&mut Writer<'a>, &Field, &dyn Debug) -> fmt::Result + Clone,
{
    type Visitor = FieldFnVisitor<'a, F>;

    fn make_visitor(&self, writer: Writer<'a>) -> Self::Visitor {
        FieldFnVisitor {
            writer,
            f: self.0.clone(),
            result: Ok(()),
        }
    }
}

impl<'a, F> Visit for FieldFnVisitor<'a, F>
where
    F: Fn(&mut Writer<'a>, &Field, &dyn Debug) -> fmt::Result,
{
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if self.result.is_ok() {
            self.result = (self.f)(&mut self.writer, field, value);
        }
    }
}

impl<'a, F> VisitOutput<fmt::Result> for FieldFnVisitor<'a, F>
where
    F: Fn(&mut Writer<'a>, &Field, &dyn Debug) -> fmt::Result,
{
    fn finish(self) -> fmt::Result {
        self.result
    }
}

impl<'a, F> VisitFmt for FieldFnVisitor<'a, F>
where
    F: Fn(&mut Writer<'a>, &Field, &dyn Debug) -> fmt::Result,
{
    fn writer(&mut self) -> &mut dyn Write {
        &mut self.writer
    }
}

impl<F> Debug for FieldFnVisitor<'_, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldFnVisitor")
            .field("f", &format_args!("{}", any::type_name::<F>()))
            .field("writer", &self.writer)
            .field("result", &self.result)
            .finish()
    }
}

// === printing synthetic Span events ===

/// Configures what points in the span lifecycle are logged as events.
///
/// See also [`with_span_events`].
///
/// [`with_span_events`]: super::SubscriberBuilder::with_span_events
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct FmtSpan(u8);

impl FmtSpan {
    /// one event when span is created
    pub const NEW: Self = Self(1 << 0);
    /// one event per enter of a span
    pub const ENTER: Self = Self(1 << 1);
    /// one event per exit of a span
    pub const EXIT: Self = Self(1 << 2);
    /// one event when the span is dropped
    pub const CLOSE: Self = Self(1 << 3);

    /// spans are ignored (this is the default)
    pub const NONE: Self = Self(0);
    /// one event per enter/exit of a span
    pub const ACTIVE: Self = Self(Self::ENTER.0 | Self::EXIT.0);
    /// events at all points (new, enter, exit, drop)
    pub const FULL: Self =
        Self(Self::NEW.0 | Self::ENTER.0 | Self::EXIT.0 | Self::CLOSE.0);

    /// Check whether or not a certain flag is set for this [`FmtSpan`]
    fn contains(self, other: Self) -> bool {
        self & other == other
    }
}

/// Implements binary bit operators for [`FmtSpan`].
macro_rules! impl_fmt_span_bit_op {
    ($trait:ident, $func:ident, $op:tt) => {
        impl std::ops::$trait for FmtSpan {
            type Output = FmtSpan;

            fn $func(self, rhs: Self) -> Self::Output {
                FmtSpan(self.0 $op rhs.0)
            }
        }
    };
}

/// Implements assignment bit operators for [`FmtSpan`].
macro_rules! impl_fmt_span_bit_assign_op {
    ($trait:ident, $func:ident, $op:tt) => {
        impl std::ops::$trait for FmtSpan {
            fn $func(&mut self, rhs: Self) {
                *self = FmtSpan(self.0 $op rhs.0)
            }
        }
    };
}

impl_fmt_span_bit_op!(BitAnd, bitand, &);
impl_fmt_span_bit_op!(BitOr, bitor, |);
impl_fmt_span_bit_op!(BitXor, bitxor, ^);

impl_fmt_span_bit_assign_op!(BitAndAssign, bitand_assign, &);
impl_fmt_span_bit_assign_op!(BitOrAssign, bitor_assign, |);
impl_fmt_span_bit_assign_op!(BitXorAssign, bitxor_assign, ^);

impl Debug for FmtSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut wrote_flag = false;
        let mut write_flags = |flag, flag_str| -> fmt::Result {
            if self.contains(flag) {
                if wrote_flag {
                    f.write_str(" | ")?;
                }

                f.write_str(flag_str)?;
                wrote_flag = true;
            }

            Ok(())
        };

        if Self::NONE | *self == Self::NONE {
            f.write_str("FmtSpan::NONE")?;
        } else {
            write_flags(Self::NEW, "FmtSpan::NEW")?;
            write_flags(Self::ENTER, "FmtSpan::ENTER")?;
            write_flags(Self::EXIT, "FmtSpan::EXIT")?;
            write_flags(Self::CLOSE, "FmtSpan::CLOSE")?;
        }

        Ok(())
    }
}

/// Runtime configuration for synthetic span lifecycle events.
pub(super) struct FmtSpanConfig {
    /// The span lifecycle points emitted as events.
    pub(super) kind: FmtSpan,
    /// Whether close events include timing fields.
    pub(super) fmt_timing: bool,
}

impl FmtSpanConfig {
    /// Returns this configuration with timing fields disabled.
    pub(super) const fn without_time(self) -> Self {
        Self {
            kind: self.kind,
            fmt_timing: false,
        }
    }
    /// Returns this configuration with a different span event kind.
    pub(super) const fn with_kind(self, kind: FmtSpan) -> Self {
        Self {
            kind,
            fmt_timing: self.fmt_timing,
        }
    }
    /// Returns whether new-span events are enabled.
    pub(super) fn trace_new(&self) -> bool {
        self.kind.contains(FmtSpan::NEW)
    }
    /// Returns whether span-enter events are enabled.
    pub(super) fn trace_enter(&self) -> bool {
        self.kind.contains(FmtSpan::ENTER)
    }
    /// Returns whether span-exit events are enabled.
    pub(super) fn trace_exit(&self) -> bool {
        self.kind.contains(FmtSpan::EXIT)
    }
    /// Returns whether span-close events are enabled.
    pub(super) fn trace_close(&self) -> bool {
        self.kind.contains(FmtSpan::CLOSE)
    }
}

impl Debug for FmtSpanConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Default for FmtSpanConfig {
    fn default() -> Self {
        Self {
            kind: FmtSpan::NONE,
            fmt_timing: true,
        }
    }
}

/// Displays a span duration in compact human-readable units.
pub(super) struct TimingDisplay(pub(super) u64);

impl Display for TimingDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let nanos = u128::from(self.0);
        for (unit, nanos_per_unit) in [
            ("ns", 1_u128),
            ("\u{b5}s", 1_000),
            ("ms", 1_000_000),
            ("s", 1_000_000_000),
        ] {
            if nanos
                < nanos_per_unit
                    .checked_mul(10)
                    .ok_or(fmt::Error)?
            {
                return write_fixed_duration(f, nanos, nanos_per_unit, 2, unit);
            }
            if nanos
                < nanos_per_unit
                    .checked_mul(100)
                    .ok_or(fmt::Error)?
            {
                return write_fixed_duration(f, nanos, nanos_per_unit, 1, unit);
            }
            if nanos
                < nanos_per_unit
                    .checked_mul(1_000)
                    .ok_or(fmt::Error)?
            {
                return write_fixed_duration(f, nanos, nanos_per_unit, 0, unit);
            }
        }
        write_fixed_duration(f, nanos, 1_000_000_000, 0, "s")
    }
}

/// Writes a rounded fixed-point duration.
fn write_fixed_duration(
    f: &mut fmt::Formatter<'_>,
    nanos: u128,
    nanos_per_unit: u128,
    precision: u32,
    unit: &str,
) -> fmt::Result {
    let scale = match precision {
        0 => 1,
        1 => 10,
        2 => 100,
        _ => return Err(fmt::Error),
    };
    let half_unit = nanos_per_unit.checked_div(2).ok_or(fmt::Error)?;
    let scaled = nanos
        .checked_mul(scale)
        .and_then(|value| value.checked_add(half_unit))
        .and_then(|value| value.checked_div(nanos_per_unit))
        .ok_or(fmt::Error)?;

    if precision == 0 {
        return write!(f, "{scaled}{unit}");
    }

    let whole = scaled.checked_div(scale).ok_or(fmt::Error)?;
    let fraction = scaled.checked_rem(scale).ok_or(fmt::Error)?;
    let width = match precision {
        1 => 1,
        2 => 2,
        _ => 0,
    };
    write!(f, "{whole}.{fraction:0width$}{unit}")
}

#[cfg(test)]
pub(super) mod test {
    use crate::fmt::{Subscriber, SubscriberBuilder, test::MockMakeWriter, time::FormatTime};
    use alloc::{
        borrow::ToOwned as _,
        format,
        string::{String, ToString as _},
    };
    use core::fmt::Result as FmtResult;
    use tracing::{
        self,
        dispatcher::{set_default, Dispatch},
        subscriber::with_default,
    };

    use super::*;

    use regex::Regex;
    use std::path::Path;
    use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok, ensure_some};

    pub(in crate::fmt) struct MockTime;
    impl FormatTime for MockTime {
        fn format_time(&self, writer: &mut Writer<'_>) -> FmtResult {
            write!(writer, "fake time")
        }
    }

    #[test]
    fn disable_everything() -> Result<(), TestFailure> {
        // This test reproduces https://github.com/tokio-rs/tracing/issues/1354
        let make_writer = MockMakeWriter::default();
        let subscriber = {
            let builder = Subscriber::builder()
                .with_writer(make_writer.clone())
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false);
            #[cfg(feature = "ansi")]
            {
                builder.with_ansi(false)
            }
            #[cfg(not(feature = "ansi"))]
            {
                builder
            }
        };
        assert_info_hello(subscriber, &make_writer, "hello\n")
    }

    fn test_ansi<T>(
        is_ansi: bool,
        expected: &str,
        builder: SubscriberBuilder<DefaultFields, Format<T>>,
    ) -> Result<(), TestFailure>
    where
        Format<T, MockTime>: FormatEvent<crate::Registry, DefaultFields>,
        T: Send + Sync + 'static,
    {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder
            .with_writer(make_writer.clone())
            .with_ansi(is_ansi)
            .with_timer(MockTime);
        run_test(subscriber, &make_writer, expected)
    }

    #[cfg(not(feature = "ansi"))]
    fn test_without_ansi<T>(
        expected: &str,
        builder: SubscriberBuilder<DefaultFields, Format<T>>,
    ) -> Result<(), TestFailure>
    where
        Format<T, MockTime>: FormatEvent<crate::Registry, DefaultFields>,
        T: Send + Sync,
    {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder.with_writer(make_writer.clone()).with_timer(MockTime);
        run_test(subscriber, &make_writer, expected)
    }

    fn test_without_level<T>(
        expected: &str,
        builder: SubscriberBuilder<DefaultFields, Format<T>>,
    ) -> Result<(), TestFailure>
    where
        Format<T, MockTime>: FormatEvent<crate::Registry, DefaultFields>,
        T: Send + Sync + 'static,
    {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime);
        run_test(subscriber, &make_writer, expected)
    }

    #[test]
    fn with_line_number_and_file_name() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_file(true)
            .with_line_number(true)
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime);

        let current_path = current_path()?.replace('\\', "\\\\");
        let expected = ensure_ok(
            Regex::new(&format!(
                "^fake time tracing_subscriber::fmt::format::test: {current_path}:[0-9]+: hello\n$",
            )),
            "line and file regex compiles",
        )?;
        let _default = set_default(&subscriber.into());
        tracing::info!("hello");
        let result = make_writer.get_string();
        ensure(
            expected.is_match(&result),
            "line and file output matches expected shape",
        )
    }

    #[test]
    fn with_line_number() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_line_number(true)
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime);

        let expected = ensure_ok(
            Regex::new("^fake time tracing_subscriber::fmt::format::test: [0-9]+: hello\n$")
                ,
            "line regex compiles",
        )?;
        let _default = set_default(&subscriber.into());
        tracing::info!("hello");
        let result = make_writer.get_string();
        ensure(expected.is_match(&result), "line output matches expected shape")
    }

    #[test]
    fn with_filename() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_file(true)
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime);
        let expected = &format!(
            "fake time tracing_subscriber::fmt::format::test: {}: hello\n",
            current_path()?,
        );
        assert_info_hello(subscriber, &make_writer, expected)
    }

    #[test]
    fn with_thread_ids() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = Subscriber::builder()
            .with_writer(make_writer.clone())
            .with_thread_ids(true)
            .with_ansi(false)
            .with_timer(MockTime);
        let expected =
            "fake time  INFO ThreadId(NUMERIC) tracing_subscriber::fmt::format::test: hello\n";

        assert_info_hello_ignore_numeric(subscriber, &make_writer, expected)
    }

    #[test]
    fn pretty_default() -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = Subscriber::builder()
            .pretty()
            .with_writer(make_writer.clone())
            .with_ansi(false)
            .with_timer(MockTime);
        let expected = format!(
            "  fake time  INFO tracing_subscriber::fmt::format::test: hello\n    at {}:NUMERIC\n\n",
            file!()
        );

        assert_info_hello_ignore_numeric(subscriber, &make_writer, &expected)
    }

    fn assert_info_hello(
        subscriber: impl Into<Dispatch>,
        buf: &MockMakeWriter,
        expected: &str,
    ) -> Result<(), TestFailure> {
        let _default = set_default(&subscriber.into());
        tracing::info!("hello");
        let result = buf.get_string();

        ensure_eq(&result.as_str(), &expected, "formatted event output matches")
    }

    // When numeric characters are used they often form a non-deterministic value as they usually represent things like a thread id or line number.
    // This assert method should be used when non-deterministic numeric characters are present.
    fn assert_info_hello_ignore_numeric(
        subscriber: impl Into<Dispatch>,
        buf: &MockMakeWriter,
        expected: &str,
    ) -> Result<(), TestFailure> {
        let _default = set_default(&subscriber.into());
        tracing::info!("hello");

        let regex = ensure_ok(Regex::new("[0-9]+"), "numeric regex compiles")?;
        let result = buf.get_string();
        let result_cleaned = regex.replace_all(&result, "NUMERIC");

        ensure_eq(
            &result_cleaned.as_ref(),
            &expected,
            "formatted event output matches after normalizing numbers",
        )
    }

    fn test_overridden_parents<T>(
        expected: &str,
        builder: SubscriberBuilder<DefaultFields, Format<T>>,
    ) -> Result<(), TestFailure>
    where
        Format<T, MockTime>: FormatEvent<crate::Registry, DefaultFields>,
        T: Send + Sync + 'static,
    {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .finish();

        with_default(subscriber, || {
            let span1 = tracing::info_span!("span1", span = 1);
            let span2 = tracing::info_span!(parent: &span1, "span2", span = 2);
            tracing::info!(parent: &span2, "hello");
        });
        let result = make_writer.get_string();
        ensure_eq(
            &result.as_str(),
            &expected,
            "overridden parent output matches",
        )
    }

    fn test_overridden_parents_in_scope<T>(
        expected1: &str,
        expected2: &str,
        builder: SubscriberBuilder<DefaultFields, Format<T>>,
    ) -> Result<(), TestFailure>
    where
        Format<T, MockTime>: FormatEvent<crate::Registry, DefaultFields>,
        T: Send + Sync + 'static,
    {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder
            .with_writer(make_writer.clone())
            .with_level(false)
            .with_ansi(false)
            .with_timer(MockTime)
            .finish();

        with_default(subscriber, || -> Result<(), TestFailure> {
            let span1 = tracing::info_span!("span1", span = 1);
            let span2 = tracing::info_span!(parent: &span1, "span2", span = 2);
            let span3 = tracing::info_span!("span3", span = 3);
            let _entered_span3 = span3.enter();

            tracing::info!("hello");
            let scoped_output = make_writer.get_string();
            ensure_eq(
                &scoped_output.as_str(),
                &expected1,
                "scoped parent output matches",
            )?;

            tracing::info!(parent: &span2, "hello");
            let overridden_output = make_writer.get_string();
            ensure_eq(
                &overridden_output.as_str(),
                &expected2,
                "overridden parent output in scope matches",
            )
        })
    }

    fn run_test(
        subscriber: impl Into<Dispatch>,
        buf: &MockMakeWriter,
        expected: &str,
    ) -> Result<(), TestFailure> {
        let _default = set_default(&subscriber.into());
        tracing::info!("hello");
        let result = buf.get_string();
        ensure_eq(&result.as_str(), &expected, "formatted event output matches")
    }

    mod default {
        use super::*;

        #[test]
        fn with_thread_ids() -> Result<(), TestFailure> {
            let make_writer = MockMakeWriter::default();
            let subscriber = Subscriber::builder()
                .with_writer(make_writer.clone())
                .with_thread_ids(true)
                .with_ansi(false)
                .with_timer(MockTime);
            let expected =
                "fake time  INFO ThreadId(NUMERIC) tracing_subscriber::fmt::format::test: hello\n";

            assert_info_hello_ignore_numeric(subscriber, &make_writer, expected)
        }

        #[cfg(feature = "ansi")]
        #[test]
        fn with_ansi_true() -> Result<(), TestFailure> {
            let expected = "\u{1b}[2mfake time\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mtracing_subscriber::fmt::format::test\u{1b}[0m\u{1b}[2m:\u{1b}[0m hello\n";
            test_ansi(true, expected, Subscriber::builder())
        }

        #[cfg(feature = "ansi")]
        #[test]
        fn with_ansi_false() -> Result<(), TestFailure> {
            let expected = "fake time  INFO tracing_subscriber::fmt::format::test: hello\n";
            test_ansi(false, expected, Subscriber::builder())
        }

        #[cfg(not(feature = "ansi"))]
        #[test]
        fn without_ansi() -> Result<(), TestFailure> {
            let expected = "fake time  INFO tracing_subscriber::fmt::format::test: hello\n";
            test_without_ansi(expected, Subscriber::builder())
        }

        #[test]
        fn without_level() -> Result<(), TestFailure> {
            let expected = "fake time tracing_subscriber::fmt::format::test: hello\n";
            test_without_level(expected, Subscriber::builder())
        }

        #[test]
        fn overridden_parents() -> Result<(), TestFailure> {
            let expected = "fake time span1{span=1}:span2{span=2}: tracing_subscriber::fmt::format::test: hello\n";
            test_overridden_parents(expected, Subscriber::builder())
        }

        #[test]
        fn overridden_parents_in_scope() -> Result<(), TestFailure> {
            test_overridden_parents_in_scope(
                "fake time span3{span=3}: tracing_subscriber::fmt::format::test: hello\n",
                "fake time span1{span=1}:span2{span=2}: tracing_subscriber::fmt::format::test: hello\n",
                Subscriber::builder(),
            )
        }
    }

    mod compact {
        use super::*;

        #[cfg(feature = "ansi")]
        #[test]
        fn with_ansi_true() -> Result<(), TestFailure> {
            let expected = "\u{1b}[2mfake time\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mtracing_subscriber::fmt::format::test\u{1b}[0m\u{1b}[2m:\u{1b}[0m hello\n";
            test_ansi(true, expected, Subscriber::builder().compact())
        }

        #[cfg(feature = "ansi")]
        #[test]
        fn with_ansi_false() -> Result<(), TestFailure> {
            let expected = "fake time  INFO tracing_subscriber::fmt::format::test: hello\n";
            test_ansi(false, expected, Subscriber::builder().compact())
        }

        #[cfg(not(feature = "ansi"))]
        #[test]
        fn without_ansi() -> Result<(), TestFailure> {
            let expected = "fake time  INFO tracing_subscriber::fmt::format::test: hello\n";
            test_without_ansi(expected, Subscriber::builder().compact())
        }

        #[test]
        fn without_level() -> Result<(), TestFailure> {
            let expected = "fake time tracing_subscriber::fmt::format::test: hello\n";
            test_without_level(expected, Subscriber::builder().compact())
        }

        #[test]
        fn overridden_parents() -> Result<(), TestFailure> {
            let expected = "fake time span1:span2: tracing_subscriber::fmt::format::test: hello span=1 span=2\n";
            test_overridden_parents(expected, Subscriber::builder().compact())
        }

        #[test]
        fn overridden_parents_in_scope() -> Result<(), TestFailure> {
            test_overridden_parents_in_scope(
                "fake time span3: tracing_subscriber::fmt::format::test: hello span=3\n",
                "fake time span1:span2: tracing_subscriber::fmt::format::test: hello span=1 span=2\n",
                Subscriber::builder().compact(),
            )
        }
    }

    mod pretty {
        use super::*;

        #[test]
        fn pretty_default() -> Result<(), TestFailure> {
            let make_writer = MockMakeWriter::default();
            let subscriber = Subscriber::builder()
                .pretty()
                .with_writer(make_writer.clone())
                .with_ansi(false)
                .with_timer(MockTime);
            let expected = format!(
                "  fake time  INFO tracing_subscriber::fmt::format::test: hello\n    at {}:NUMERIC\n\n",
                file!()
            );

            assert_info_hello_ignore_numeric(subscriber, &make_writer, &expected)
        }
    }

    #[test]
    fn format_nanos() -> Result<(), TestFailure> {
        fn format_timing(nanos: u64) -> String {
            TimingDisplay(nanos).to_string()
        }

        let cases = [
            (1, "1.00ns"),
            (12, "12.0ns"),
            (123, "123ns"),
            (1_234, "1.23\u{b5}s"),
            (12_345, "12.3\u{b5}s"),
            (123_456, "123\u{b5}s"),
            (1_234_567, "1.23ms"),
            (12_345_678, "12.3ms"),
            (123_456_789, "123ms"),
            (1_234_567_890, "1.23s"),
            (12_345_678_901, "12.3s"),
            (123_456_789_012, "123s"),
            (1_234_567_890_123, "1235s"),
        ];
        for (nanos, expected) in cases {
            let actual = format_timing(nanos);
            ensure_eq(
                &actual.as_str(),
                &expected,
                "nanosecond timing display matches",
            )?;
        }
        Ok(())
    }

    #[test]
    fn fmt_span_combinations() -> Result<(), TestFailure> {
        let none_flags = FmtSpan::NONE;
        ensure(
            !none_flags.contains(FmtSpan::NEW),
            "FmtSpan::NONE excludes new events",
        )?;
        ensure(
            !none_flags.contains(FmtSpan::ENTER),
            "FmtSpan::NONE excludes enter events",
        )?;
        ensure(
            !none_flags.contains(FmtSpan::EXIT),
            "FmtSpan::NONE excludes exit events",
        )?;
        ensure(
            !none_flags.contains(FmtSpan::CLOSE),
            "FmtSpan::NONE excludes close events",
        )?;

        let active_flags = FmtSpan::ACTIVE;
        ensure(
            !active_flags.contains(FmtSpan::NEW),
            "FmtSpan::ACTIVE excludes new events",
        )?;
        ensure(
            active_flags.contains(FmtSpan::ENTER),
            "FmtSpan::ACTIVE includes enter events",
        )?;
        ensure(
            active_flags.contains(FmtSpan::EXIT),
            "FmtSpan::ACTIVE includes exit events",
        )?;
        ensure(
            !active_flags.contains(FmtSpan::CLOSE),
            "FmtSpan::ACTIVE excludes close events",
        )?;

        let full_flags = FmtSpan::FULL;
        ensure(
            full_flags.contains(FmtSpan::NEW),
            "FmtSpan::FULL includes new events",
        )?;
        ensure(
            full_flags.contains(FmtSpan::ENTER),
            "FmtSpan::FULL includes enter events",
        )?;
        ensure(
            full_flags.contains(FmtSpan::EXIT),
            "FmtSpan::FULL includes exit events",
        )?;
        ensure(
            full_flags.contains(FmtSpan::CLOSE),
            "FmtSpan::FULL includes close events",
        )?;

        let new_close_flags = FmtSpan::NEW | FmtSpan::CLOSE;
        ensure(
            new_close_flags.contains(FmtSpan::NEW),
            "new-close combination includes new events",
        )?;
        ensure(
            !new_close_flags.contains(FmtSpan::ENTER),
            "new-close combination excludes enter events",
        )?;
        ensure(
            !new_close_flags.contains(FmtSpan::EXIT),
            "new-close combination excludes exit events",
        )?;
        ensure(
            new_close_flags.contains(FmtSpan::CLOSE),
            "new-close combination includes close events",
        )
    }

    /// Returns the test's module path.
    fn current_path() -> Result<String, TestFailure> {
        let owned_path = Path::new("tracing-subscriber")
            .join("src")
            .join("fmt")
            .join("format.rs");
        let module_path = ensure_some(
            owned_path.to_str(),
            "test module path does not contain invalid unicode",
        )?;
        Ok(module_path.to_owned())
    }
}
