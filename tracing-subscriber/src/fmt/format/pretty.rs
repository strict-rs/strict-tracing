//! Pretty human-readable event and field formatters.

use super::{
    span, ErrorSourceList, EscapeGuard, Format, FormatEvent, FormatFields,
    FormatTime, MakeVisitor, RecordFields, Writer,
};
use crate::{
    field::{VisitFmt, VisitOutput},
    fmt::fmt_layer::{FmtContext, FormattedFields},
    registry::{LookupSpan, SpanRef},
};

use std::{
    error::Error,
    fmt::{self, Debug, Write},
    thread,
};
use tracing_core::{
    field::{Field, Visit},
    Event, Level, Metadata, Subscriber,
};

#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent as _;

use nu_ansi_term::{Color, Style};

use super::{DebugValue, FmtThreadId};

/// An excessively pretty, human-readable event formatter.
///
/// Unlike the [`Full`], [`Compact`], and [`Json`] formatters, this is a
/// multi-line output format. Each individual event may output multiple lines of
/// text.
///
/// # Example Output
///
/// <pre><font color="#4E9A06"><b>:;</b></font> <font color="#4E9A06">cargo</font> run --example fmt-pretty
/// <font color="#4E9A06"><b>    Finished</b></font> dev [unoptimized + debuginfo] target(s) in 0.08s
/// <font color="#4E9A06"><b>     Running</b></font> `target/debug/examples/fmt-pretty`
///   2022-02-15T18:44:24.535324Z <font color="#4E9A06"> INFO</font> <font color="#4E9A06"><b>fmt_pretty</b></font><font color="#4E9A06">: preparing to shave yaks, </font><font color="#4E9A06"><b>number_of_yaks</b></font><font color="#4E9A06">: 3</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt-pretty.rs:16 <font color="#AAAAAA"><i>on</i></font> main
///
///   2022-02-15T18:44:24.535403Z <font color="#4E9A06"> INFO</font> <font color="#4E9A06"><b>fmt_pretty::yak_shave</b></font><font color="#4E9A06">: shaving yaks</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:41 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535442Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: hello! I&apos;m gonna shave a yak, </font><font color="#75507B"><b>excitement</b></font><font color="#75507B">: &quot;yay!&quot;</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:16 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shave</b> <font color="#AAAAAA"><i>with</i></font> <b>yak</b>: 1
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535469Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: yak shaved successfully</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:25 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shave</b> <font color="#AAAAAA"><i>with</i></font> <b>yak</b>: 1
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535502Z <font color="#3465A4">DEBUG</font> <font color="#3465A4"><b>yak_events</b></font><font color="#3465A4">: </font><font color="#3465A4"><b>yak</b></font><font color="#3465A4">: 1, </font><font color="#3465A4"><b>shaved</b></font><font color="#3465A4">: true</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:46 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535524Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: </font><font color="#75507B"><b>yaks_shaved</b></font><font color="#75507B">: 1</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:55 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535551Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: hello! I&apos;m gonna shave a yak, </font><font color="#75507B"><b>excitement</b></font><font color="#75507B">: &quot;yay!&quot;</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:16 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shave</b> <font color="#AAAAAA"><i>with</i></font> <b>yak</b>: 2
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535573Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: yak shaved successfully</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:25 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shave</b> <font color="#AAAAAA"><i>with</i></font> <b>yak</b>: 2
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535600Z <font color="#3465A4">DEBUG</font> <font color="#3465A4"><b>yak_events</b></font><font color="#3465A4">: </font><font color="#3465A4"><b>yak</b></font><font color="#3465A4">: 2, </font><font color="#3465A4"><b>shaved</b></font><font color="#3465A4">: true</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:46 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535618Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: </font><font color="#75507B"><b>yaks_shaved</b></font><font color="#75507B">: 2</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:55 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535644Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: hello! I&apos;m gonna shave a yak, </font><font color="#75507B"><b>excitement</b></font><font color="#75507B">: &quot;yay!&quot;</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:16 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shave</b> <font color="#AAAAAA"><i>with</i></font> <b>yak</b>: 3
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535670Z <font color="#C4A000"> WARN</font> <font color="#C4A000"><b>fmt_pretty::yak_shave</b></font><font color="#C4A000">: could not locate yak</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:18 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shave</b> <font color="#AAAAAA"><i>with</i></font> <b>yak</b>: 3
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535698Z <font color="#3465A4">DEBUG</font> <font color="#3465A4"><b>yak_events</b></font><font color="#3465A4">: </font><font color="#3465A4"><b>yak</b></font><font color="#3465A4">: 3, </font><font color="#3465A4"><b>shaved</b></font><font color="#3465A4">: false</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:46 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535720Z <font color="#CC0000">ERROR</font> <font color="#CC0000"><b>fmt_pretty::yak_shave</b></font><font color="#CC0000">: failed to shave yak, </font><font color="#CC0000"><b>yak</b></font><font color="#CC0000">: 3, </font><font color="#CC0000"><b>error</b></font><font color="#CC0000">: missing yak, </font><font color="#CC0000"><b>error.sources</b></font><font color="#CC0000">: [out of space, out of cash]</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:51 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535742Z <font color="#75507B">TRACE</font> <font color="#75507B"><b>fmt_pretty::yak_shave</b></font><font color="#75507B">: </font><font color="#75507B"><b>yaks_shaved</b></font><font color="#75507B">: 2</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt/yak_shave.rs:55 <font color="#AAAAAA"><i>on</i></font> main
///     <font color="#AAAAAA"><i>in</i></font> fmt_pretty::yak_shave::<b>shaving_yaks</b> <font color="#AAAAAA"><i>with</i></font> <b>yaks</b>: 3
///
///   2022-02-15T18:44:24.535765Z <font color="#4E9A06"> INFO</font> <font color="#4E9A06"><b>fmt_pretty</b></font><font color="#4E9A06">: yak shaving completed, </font><font color="#4E9A06"><b>all_yaks_shaved</b></font><font color="#4E9A06">: false</font>
///     <font color="#AAAAAA"><i>at</i></font> examples/examples/fmt-pretty.rs:19 <font color="#AAAAAA"><i>on</i></font> main
/// </pre>
///
/// [`Full`]: crate::fmt::format::Full
/// [`Compact`]: crate::fmt::format::Compact
/// [`Json`]: crate::fmt::format::Json
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Pretty {
    /// Whether the event's source location is displayed.
    display_location: bool,
}

/// The [visitor] produced by [`Pretty`]'s [`MakeVisitor`] implementation.
///
/// [visitor]: crate::field::Visit
/// [`MakeVisitor`]: crate::field::MakeVisitor
#[derive(Debug)]
pub struct PrettyVisitor<'a> {
    /// The writer receiving pretty-formatted fields.
    writer: Writer<'a>,
    /// Whether no fields have been written yet.
    is_empty: bool,
    /// The active style used for fields.
    style: Style,
    /// The first formatting error encountered while visiting fields.
    result: fmt::Result,
}

/// An excessively pretty, human-readable [`MakeVisitor`] implementation.
///
/// [`MakeVisitor`]: crate::field::MakeVisitor
#[derive(Copy, Clone, Debug)]
pub struct PrettyFields {
    /// A value to override the provided `Writer`'s ANSI formatting
    /// configuration.
    ///
    /// If this is `Some`, we override the `Writer`'s ANSI setting. This is
    /// necessary in order to continue supporting the deprecated
    /// `PrettyFields::with_ansi` method. If it is `None`, we don't override the
    /// ANSI formatting configuration (because the deprecated method was not
    /// called).
    // TODO: when `PrettyFields::with_ansi` is removed, we can get rid
    // of this entirely.
    ansi: Option<bool>,
}

// === impl Pretty ===

impl Default for Pretty {
    fn default() -> Self {
        Self {
            display_location: true,
        }
    }
}

impl Pretty {
    /// Returns the ANSI style used for `level`.
    #[allow(
        clippy::single_call_fn,
        reason = "pretty formatter styling keeps level-to-color mapping behind a named query"
    )]
    fn style_for(level: Level) -> Style {
        match level {
            Level::TRACE => Style::new().fg(Color::Purple),
            Level::DEBUG => Style::new().fg(Color::Blue),
            Level::INFO => Style::new().fg(Color::Green),
            Level::WARN => Style::new().fg(Color::Yellow),
            Level::ERROR => Style::new().fg(Color::Red),
        }
    }

    /// Sets whether the event's source code location is displayed.
    ///
    /// This defaults to `true`.
    #[deprecated(
        since = "0.3.6",
        note = "all formatters now support configurable source locations. Use `Format::with_source_location` instead."
    )]
    #[must_use]
    pub const fn with_source_location(self, display_location: bool) -> Self {
        let mut pretty = self;
        pretty.display_location = display_location;
        pretty
    }
}

impl<T> Format<Pretty, T> {
    /// Writes source-location and thread context lines after the event fields.
    fn write_source_and_thread(
        &self,
        writer: &mut Writer<'_>,
        meta: &Metadata<'_>,
        maybe_line_number: Option<u32>,
        dimmed: &Style,
    ) -> fmt::Result {
        let displays_thread = self.display.thread_name() || self.display.thread_id();
        let thread_name_separator = if self.display.thread_id() { " " } else { "" };

        if let (Some(file), true, true) = (
            meta.file(),
            self.kind.display_location,
            self.display.filename(),
        ) {
            write!(writer, "    {} {}", dimmed.paint("at"), file)?;

            if let Some(line_number) = maybe_line_number {
                write!(writer, ":{line_number}")?;
            }
            writer.write_char(if displays_thread { ' ' } else { '\n' })?;
        } else if displays_thread {
            write!(writer, "    ")?;
        } else {
            // No source location or thread prefix is enabled.
        }

        if displays_thread {
            write!(writer, "{} ", dimmed.paint("on"))?;
            let current_thread = thread::current();
            if self.display.thread_name()
                && let Some(name) = current_thread.name()
            {
                write!(writer, "{name}")?;
                writer.write_str(thread_name_separator)?;
            }
            if self.display.thread_id() {
                write!(writer, "{}", FmtThreadId::new(current_thread.id()))?;
            }
            writer.write_char('\n')?;
        }

        Ok(())
    }

    /// Writes the current span stack after the event context line.
    fn write_span_context<C, N>(
        &self,
        ctx: &FmtContext<'_, C, N>,
        writer: &mut Writer<'_>,
        event: &Event<'_>,
        dimmed: &Style,
    ) -> fmt::Result
    where
        C: Subscriber + for<'lookup> LookupSpan<'lookup>,
        N: for<'writer> FormatFields<'writer> + 'static,
    {
        let bold = writer.bold();
        let current_span = event
            .parent()
            .copied()
            .and_then(|id| ctx.span(id))
            .or_else(|| ctx.lookup_current());

        let scope = current_span
            .into_iter()
            .flat_map(|span_ref| span_ref.scope());

        for span_ref in scope {
            let span_meta = span_ref.metadata();
            if self.display.target() {
                write!(
                    writer,
                    "    {} {}::{}",
                    dimmed.paint("in"),
                    span_meta.target(),
                    bold.paint(span_meta.name()),
                )?;
            } else {
                write!(
                    writer,
                    "    {} {}",
                    dimmed.paint("in"),
                    bold.paint(span_meta.name()),
                )?;
            }

            Self::write_span_fields::<C, N>(writer, &span_ref, dimmed)?;
            writer.write_char('\n')?;
        }

        Ok(())
    }

    /// Writes formatted fields for one span in the pretty span stack.
    #[allow(
        clippy::single_call_fn,
        reason = "pretty span formatting isolates extension access and releases the borrow before recursion"
    )]
    fn write_span_fields<C, N>(
        writer: &mut Writer<'_>,
        span_ref: &SpanRef<'_, C>,
        dimmed: &Style,
    ) -> fmt::Result
    where
        C: for<'lookup> LookupSpan<'lookup>,
        N: for<'writer> FormatFields<'writer> + 'static,
    {
        let extensions = span_ref.extensions();
        let Some(fields) = extensions.get::<FormattedFields<N>>() else {
            return Err(fmt::Error);
        };
        if !fields.is_empty() {
            write!(writer, " {} {}", dimmed.paint("with"), fields)?;
        }
        drop(extensions);
        Ok(())
    }
}

impl<C, N, T> FormatEvent<C, N> for Format<Pretty, T>
where
    C: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, C, N>,
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
        write!(&mut writer, "  ")?;

        // if the `Format` struct *also* has an ANSI color configuration,
        // override the writer...the API for configuring ANSI color codes on the
        // `Format` struct is deprecated, but we still need to honor those
        // configurations.
        if let Some(ansi) = self.ansi {
            writer = writer.with_ansi(ansi);
        }

        self.format_timestamp(&mut writer)?;

        let style = if self.display.level() && writer.has_ansi_escapes() {
            Pretty::style_for(*meta.level())
        } else {
            Style::new()
        };

        if self.display.level() {
            write!(
                writer,
                "{} ",
                super::fmt_level(meta.level(), &writer)
            )?;
        }

        if self.display.target() {
            let target_style = if writer.has_ansi_escapes() {
                style.bold()
            } else {
                style
            };
            write!(
                writer,
                "{}{}{}:",
                target_style.prefix(),
                meta.target(),
                target_style.infix(style)
            )?;
        }
        let maybe_line_number = if self.display.line_number() {
            meta.line()
        } else {
            None
        };

        // If the file name is disabled, format the line number right after the
        // target. Otherwise, if we also display the file, it'll go on a
        // separate line.
        if let (Some(event_line_number), false, true) = (
            maybe_line_number,
            self.display.filename(),
            self.kind.display_location,
        ) {
            write!(
                writer,
                "{}{}{}:",
                style.prefix(),
                event_line_number,
                style.infix(style)
            )?;
        }

        writer.write_char(' ')?;

        let mut visitor = PrettyVisitor::new(writer.by_ref(), true).with_style(&style);
        event.record(&mut visitor);
        visitor.finish()?;
        writer.write_char('\n')?;

        let dimmed = if writer.has_ansi_escapes() {
            Style::new().dimmed().italic()
        } else {
            Style::new()
        };
        self.write_source_and_thread(&mut writer, meta, maybe_line_number, &dimmed)?;
        self.write_span_context(ctx, &mut writer, event, &dimmed)?;

        writer.write_char('\n')
    }
}

impl<'writer> FormatFields<'writer> for Pretty {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut visitor = PrettyVisitor::new(writer, true);
        fields.record(&mut visitor);
        visitor.finish()
    }

    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &span::Record<'_>,
    ) -> fmt::Result {
        let empty = current.is_empty();
        let writer = current.as_writer();
        let mut visitor = PrettyVisitor::new(writer, empty);
        fields.record(&mut visitor);
        visitor.finish()
    }
}

// === impl PrettyFields ===

impl Default for PrettyFields {
    fn default() -> Self {
        Self::new()
    }
}

impl PrettyFields {
    /// Returns a new default [`PrettyFields`] implementation.
    #[must_use]
    #[allow(
        clippy::single_call_fn,
        reason = "public constructor is part of the documented pretty field formatter API"
    )]
    pub const fn new() -> Self {
        // By default, don't override the `Writer`'s ANSI colors
        // configuration. We'll only do this if the user calls the
        // deprecated `PrettyFields::with_ansi` method.
        Self { ansi: None }
    }

    /// Enable ANSI encoding for formatted fields.
    #[deprecated(
        since = "0.3.3",
        note = "Use `fmt::Subscriber::with_ansi` or `fmt::Layer::with_ansi` instead."
    )]
    #[must_use]
    pub const fn with_ansi(self, ansi: bool) -> Self {
        let mut fields = self;
        fields.ansi = Some(ansi);
        fields
    }
}

impl<'a> MakeVisitor<Writer<'a>> for PrettyFields {
    type Visitor = PrettyVisitor<'a>;

    #[inline]
    fn make_visitor(&self, mut target: Writer<'a>) -> Self::Visitor {
        if let Some(ansi) = self.ansi {
            target = target.with_ansi(ansi);
        }
        PrettyVisitor::new(target, true)
    }
}

// === impl PrettyVisitor ===

impl<'a> PrettyVisitor<'a> {
    /// Returns a new default visitor that formats to the provided `writer`.
    ///
    /// # Arguments
    /// - `writer`: the writer to format to.
    /// - `is_empty`: whether or not any fields have been previously written to
    ///   that writer.
    #[must_use]
    pub fn new(writer: Writer<'a>, is_empty: bool) -> Self {
        Self {
            writer,
            is_empty,
            style: Style::default(),
            result: Ok(()),
        }
    }

    /// Returns this visitor with a field style override.
    pub(crate) const fn with_style(self, style: &Style) -> Self {
        Self { style: *style, ..self }
    }

    /// Writes `fragment` with the field separator required by this visitor state.
    fn write_padded(&mut self, fragment: impl fmt::Display) {
        let padding = if self.is_empty {
            self.is_empty = false;
            ""
        } else {
            ", "
        };
        self.result = write!(self.writer, "{padding}{fragment}");
    }

    /// Returns bold styling when ANSI support is enabled for this writer.
    fn bold(&self) -> Style {
        if self.writer.has_ansi_escapes() {
            self.style.bold()
        } else {
            Style::new()
        }
    }
}

impl Visit for PrettyVisitor<'_> {
    fn record_str(&mut self, field: &Field, field_value: &str) {
        if self.result.is_err() {
            return;
        }

        if field.name() == "message" {
            self.record_debug(field, &format_args!("{field_value}"));
        } else {
            self.record_debug(field, &field_value);
        }
    }

    fn record_error(&mut self, field: &Field, field_value: &(dyn Error + 'static)) {
        let sanitize = self.writer.sanitizes_ansi_escapes();
        if let Some(source) = field_value.source() {
            let bold = self.bold();
            self.record_debug(
                field,
                &format_args!(
                    "{}, {}{}.sources{}: {}",
                    EscapeGuard::new(format_args!("{field_value}"), sanitize),
                    bold.prefix(),
                    field,
                    bold.infix(self.style),
                    ErrorSourceList::new(source, sanitize),
                ),
            );
        } else {
            self.record_debug(
                field,
                &EscapeGuard::new(format_args!("{field_value}"), sanitize),
            );
        }
    }

    fn record_debug(&mut self, field: &Field, field_value: &dyn Debug) {
        if self.result.is_err() {
            return;
        }
        let bold = self.bold();
        match field.name() {
            "message" => {
                // Escape ANSI characters to prevent malicious patterns (e.g., terminal injection attacks)
                self.write_padded(format_args!(
                    "{}{}",
                    self.style.prefix(),
                    EscapeGuard::new(DebugValue(field_value), self.writer.sanitizes_ansi_escapes())
                ));
            }
            // Skip fields that are actually log metadata that have already been handled
            #[cfg(feature = "tracing-log")]
            name if name.starts_with("log.") => self.result = Ok(()),
            name => {
                let field_name = name.strip_prefix("r#").unwrap_or(name);
                self.write_padded(format_args!(
                    "{}{}{}: {}",
                    bold.prefix(),
                    field_name,
                    bold.infix(self.style),
                    DebugValue(field_value)
                ));
            }
        }
    }
}

impl VisitOutput<fmt::Result> for PrettyVisitor<'_> {
    fn finish(mut self) -> fmt::Result {
        write!(&mut self.writer, "{}", self.style.suffix())?;
        self.result
    }
}

impl VisitFmt for PrettyVisitor<'_> {
    fn writer(&mut self) -> &mut dyn Write {
        &mut self.writer
    }
}
