//! JSON event and field formatters.

use super::{Format, FormatEvent, FormatFields, FormatTime, Writer};
use crate::{
    field::{RecordFields, VisitFmt, VisitOutput},
    fmt::{
        fmt_layer::{FmtContext, FormattedFields},
        writer::WriteAdaptor,
    },
    layer::Context,
    registry::{LookupSpan, SpanRef},
};
use alloc::{
    borrow::ToOwned as _,
    collections::BTreeMap,
    fmt::{self, Debug, Write},
    format,
    string::String,
};
use core::marker::PhantomData;
use serde::{ser::{Error as _, SerializeMap, SerializeSeq as _, Serializer}, Serialize};
use serde_json::{Serializer as JsonSerializer, Value};
use std::thread;
use tracing_core::{
    field::{Field, Visit},
    span::Record,
    Event, Subscriber,
};
use tracing_serde::AsSerde as _;

use super::FmtThreadId;

#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent as _;

/// Marker for [`Format`] that indicates that the newline-delimited JSON log
/// format should be used.
///
/// This formatter is intended for production use with systems where structured
/// logs are consumed as JSON by analysis and viewing tools. The JSON output is
/// not optimized for human readability; instead, it should be pretty-printed
/// using external JSON tools such as `jq`, or using a JSON log viewer.
///
/// # Example Output
///
/// <pre><font color="#4E9A06"><b>:;</b></font> <font color="#4E9A06">cargo</font> run --example fmt-json
/// <font color="#4E9A06"><b>    Finished</b></font> dev [unoptimized + debuginfo] target(s) in 0.08s
/// <font color="#4E9A06"><b>     Running</b></font> `target/debug/examples/fmt-json`
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821315Z&quot;,&quot;level&quot;:&quot;INFO&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;preparing to shave yaks&quot;,&quot;number_of_yaks&quot;:3},&quot;target&quot;:&quot;fmt_json&quot;}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821422Z&quot;,&quot;level&quot;:&quot;INFO&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;shaving yaks&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821495Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;hello! I&apos;m gonna shave a yak&quot;,&quot;excitement&quot;:&quot;yay!&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;},{&quot;yak&quot;:1,&quot;name&quot;:&quot;shave&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821546Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;yak shaved successfully&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;},{&quot;yak&quot;:1,&quot;name&quot;:&quot;shave&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821598Z&quot;,&quot;level&quot;:&quot;DEBUG&quot;,&quot;fields&quot;:{&quot;yak&quot;:1,&quot;shaved&quot;:true},&quot;target&quot;:&quot;yak_events&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821637Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;yaks_shaved&quot;:1},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821684Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;hello! I&apos;m gonna shave a yak&quot;,&quot;excitement&quot;:&quot;yay!&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;},{&quot;yak&quot;:2,&quot;name&quot;:&quot;shave&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821727Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;yak shaved successfully&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;},{&quot;yak&quot;:2,&quot;name&quot;:&quot;shave&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821773Z&quot;,&quot;level&quot;:&quot;DEBUG&quot;,&quot;fields&quot;:{&quot;yak&quot;:2,&quot;shaved&quot;:true},&quot;target&quot;:&quot;yak_events&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821806Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;yaks_shaved&quot;:2},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821909Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;hello! I&apos;m gonna shave a yak&quot;,&quot;excitement&quot;:&quot;yay!&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;},{&quot;yak&quot;:3,&quot;name&quot;:&quot;shave&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.821956Z&quot;,&quot;level&quot;:&quot;WARN&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;could not locate yak&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;},{&quot;yak&quot;:3,&quot;name&quot;:&quot;shave&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.822006Z&quot;,&quot;level&quot;:&quot;DEBUG&quot;,&quot;fields&quot;:{&quot;yak&quot;:3,&quot;shaved&quot;:false},&quot;target&quot;:&quot;yak_events&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.822041Z&quot;,&quot;level&quot;:&quot;ERROR&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;failed to shave yak&quot;,&quot;yak&quot;:3,&quot;error&quot;:&quot;missing yak&quot;},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.822079Z&quot;,&quot;level&quot;:&quot;TRACE&quot;,&quot;fields&quot;:{&quot;yaks_shaved&quot;:2},&quot;target&quot;:&quot;fmt_json::yak_shave&quot;,&quot;spans&quot;:[{&quot;yaks&quot;:3,&quot;name&quot;:&quot;shaving_yaks&quot;}]}
/// {&quot;timestamp&quot;:&quot;2022-02-15T18:47:10.822117Z&quot;,&quot;level&quot;:&quot;INFO&quot;,&quot;fields&quot;:{&quot;message&quot;:&quot;yak shaving completed&quot;,&quot;all_yaks_shaved&quot;:false},&quot;target&quot;:&quot;fmt_json&quot;}
/// </pre>
///
/// # Options
///
/// This formatter exposes additional options to configure the structure of the
/// output JSON objects:
///
/// - [`Json::flatten_event`] can be used to enable flattening event fields into
///   the root
/// - [`Json::with_current_span`] can be used to control logging of the current
///   span
/// - [`Json::with_span_list`] can be used to control logging of the span list
///   object.
///
/// By default, event fields are not flattened, and both current span and span
/// list are logged.
///
/// # Valuable Support
///
/// Experimental support is available for using the [`valuable`] crate to record
/// user-defined values as structured JSON. When the ["valuable" unstable
/// feature][unstable] is enabled, types implementing [`valuable::Valuable`] will
/// be recorded as structured JSON, rather than
/// using their [`std::fmt::Debug`] implementations.
///
/// **Note**: This is an experimental feature. [Unstable features][unstable]
/// must be enabled in order to use `valuable` support.
///
/// [`Json::flatten_event`]: Json::flatten_event()
/// [`Json::with_current_span`]: Json::with_current_span()
/// [`Json::with_span_list`]: Json::with_span_list()
/// [`valuable`]: https://crates.io/crates/valuable
/// [unstable]: crate#unstable-features
/// [`valuable::Valuable`]: https://docs.rs/valuable/latest/valuable/trait.Valuable.html
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Json {
    /// Bitset of enabled JSON output options.
    bits: u8,
}

impl Json {
    /// Event field flattening option.
    const FLATTEN_EVENT: u8 = 1 << 0;
    /// Current-span display option.
    const CURRENT_SPAN: u8 = 1 << 1;
    /// Span-list display option.
    const SPAN_LIST: u8 = 1 << 2;
    /// Default JSON output options.
    const DEFAULT: Self = Self {
        bits: Self::CURRENT_SPAN | Self::SPAN_LIST,
    };

    /// Returns a copy with `flag` set to `enabled`.
    const fn with_flag(mut self, flag: u8, enabled: bool) -> Self {
        if enabled {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
        self
    }

    /// Returns whether `flag` is enabled.
    const fn contains(self, flag: u8) -> bool {
        self.bits & flag == flag
    }

    /// If set to `true` event metadata will be flattened into the root object.
    pub const fn flatten_event(&mut self, flatten_event: bool) {
        *self = self.with_flag(Self::FLATTEN_EVENT, flatten_event);
    }

    /// If set to `false`, formatted events won't contain a field for the current span.
    pub const fn with_current_span(&mut self, display_current_span: bool) {
        *self = self.with_flag(Self::CURRENT_SPAN, display_current_span);
    }

    /// If set to `false`, formatted events won't contain a list of all currently
    /// entered spans. Spans are logged in a list from root to leaf.
    pub const fn with_span_list(&mut self, display_span_list: bool) {
        *self = self.with_flag(Self::SPAN_LIST, display_span_list);
    }

    /// Returns whether event fields are flattened into the root object.
    pub(crate) const fn flattens_event(self) -> bool {
        self.contains(Self::FLATTEN_EVENT)
    }

    /// Returns whether formatted events include the current span.
    pub(crate) const fn displays_current_span(self) -> bool {
        self.contains(Self::CURRENT_SPAN)
    }

    /// Returns whether formatted events include the entered span list.
    pub(crate) const fn displays_span_list(self) -> bool {
        self.contains(Self::SPAN_LIST)
    }
}

/// A serializable view of the current span context.
struct SerializableContext<'a, 'b, Span, N>(
    &'b Context<'a, Span>,
    PhantomData<N>,
)
where
    Span: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static;

impl<Span, N> Serialize for SerializableContext<'_, '_, Span, N>
where
    Span: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
    where
        Ser: Serializer,
    {
        let mut sequence = serializer.serialize_seq(None)?;

        if let Some(leaf_span) = self.0.lookup_current() {
            for span in leaf_span.scope().root_to_leaf() {
                sequence.serialize_element(&SerializableSpan(&span, self.1))?;
            }
        }

        sequence.end()
    }
}

/// A serializable view of a span and its formatted fields.
struct SerializableSpan<'a, 'b, Span, N>(
    &'b SpanRef<'a, Span>,
    PhantomData<N>,
)
where
    Span: for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static;

impl<Span, N> Serialize for SerializableSpan<'_, '_, Span, N>
where
    Span: for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn serialize<Ser>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error>
    where
        Ser: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;

        let data = {
            let extensions = self.0.extensions();
            extensions
                .get::<FormattedFields<N>>()
                .ok_or_else(|| Ser::Error::custom("missing formatted span fields"))?
                .fields()
                .to_owned()
        };

        // TODO: let's _not_ do this, but this resolves
        // https://github.com/tokio-rs/tracing/issues/391.
        // We should probably rework this to use a `Value` or something
        // similar in a JSON-specific layer, but I'd (david)
        // rather have a uglier fix now rather than shipping broken JSON.
        match serde_json::from_str::<Value>(&data) {
            Ok(Value::Object(fields)) => {
                for field in fields {
                    map.serialize_entry(&field.0, &field.1)?;
                }
            }
            // If we *aren't* in debug mode, it's probably best not to
            // crash the program, let's log the field found but also an
            // message saying it's type  is invalid
            Ok(value) => {
                map.serialize_entry("field", &value)?;
                map.serialize_entry("field_error", "field was no a valid object")?;
            }
            // If we *aren't* in debug mode, it's probably best not
            // crash the program, but let's at least make sure it's clear
            // that the fields are not supposed to be missing.
            Err(error) => map.serialize_entry("field_error", &format!("{error}"))?,
        }
        map.serialize_entry("name", self.0.metadata().name())?;
        SerializeMap::end(map)
    }
}

impl<S, N, T> FormatEvent<S, N> for Format<Json, T>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let mut timestamp = String::new();
        self.timer.format_time(&mut Writer::new(&mut timestamp))?;

        #[cfg(feature = "tracing-log")]
        let normalized = event.normalized_metadata();
        #[cfg(feature = "tracing-log")]
        let normalized_meta = normalized.as_ref().map(|meta| meta.as_metadata());
        #[cfg(feature = "tracing-log")]
        let meta = normalized_meta.as_ref().unwrap_or_else(|| event.metadata());
        #[cfg(not(feature = "tracing-log"))]
        let meta = event.metadata();

        let mut visit = || {
            let mut json_serializer = JsonSerializer::new(WriteAdaptor::new(&mut writer));

            let mut serializer = json_serializer.serialize_map(None)?;

            if self.display.timestamp() {
                serializer.serialize_entry("timestamp", &timestamp)?;
            }

            if self.display.level() {
                serializer.serialize_entry("level", &meta.level().as_serde())?;
            }

            let format_field_marker: PhantomData<N> = PhantomData;

            let current_span = if self.kind.displays_current_span() || self.kind.displays_span_list() {
                event
                    .parent()
                    .copied()
                    .and_then(|id| ctx.span(id))
                    .or_else(|| ctx.lookup_current())
            } else {
                None
            };

            if self.kind.flattens_event() {
                let mut visitor = tracing_serde::SerdeMapVisitor::new(serializer);
                event.record(&mut visitor);

                serializer = visitor.take_serializer()?;
            } else {
                use tracing_serde::fields::AsMap as _;
                serializer.serialize_entry("fields", &event.field_map())?;
            }

            if self.display.target() {
                serializer.serialize_entry("target", meta.target())?;
            }

            if self.display.filename()
                && let Some(filename) = meta.file()
            {
                serializer.serialize_entry("filename", filename)?;
            }

            if self.display.line_number()
                && let Some(line_number) = meta.line()
            {
                serializer.serialize_entry("line_number", &line_number)?;
            }

            if self.kind.displays_current_span()
                && let Some(ref span) = current_span
            {
                serializer
                    .serialize_entry("span", &SerializableSpan(span, format_field_marker))
                    ?;
            }

            if self.kind.displays_span_list() && current_span.is_some() {
                serializer.serialize_entry(
                    "spans",
                    &SerializableContext(&ctx.ctx, format_field_marker),
                )?;
            }

            if self.display.thread_name() {
                let current_thread = thread::current();
                match current_thread.name() {
                    Some(name) => {
                        serializer.serialize_entry("threadName", name)?;
                    }
                    // fall-back to thread id when name is absent and ids are not enabled
                    None if !self.display.thread_id() => {
                        serializer.serialize_entry(
                            "threadName",
                            &format!("{}", FmtThreadId::new(current_thread.id())),
                        )?;
                    }
                    _ => {}
                }
            }

            if self.display.thread_id() {
                serializer.serialize_entry(
                    "threadId",
                    &format!("{}", FmtThreadId::new(thread::current().id())),
                )?;
            }

            SerializeMap::end(serializer)
        };

        visit().map_err(|error| {
            drop(error);
            fmt::Error
        })?;
        writeln!(writer)
    }
}

impl Default for Json {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The JSON [`FormatFields`] implementation.
///
#[derive(Copy, Clone, Debug)]
pub struct JsonFields {
    // reserve the ability to add fields to this without causing a breaking
    // change in the future.
    /// Prevents external construction and reserves room for future fields.
    _private: (),
}

impl JsonFields {
    /// Returns a new JSON [`FormatFields`] implementation.
    ///
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for JsonFields {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> FormatFields<'a> for JsonFields {
    /// Format the provided `fields` to the provided `writer`, returning a result.
    fn format_fields<R: RecordFields>(&self, mut writer: Writer<'_>, fields: R) -> fmt::Result {
        let mut visitor = JsonVisitor::new(&mut writer);
        fields.record(&mut visitor);
        visitor.finish()
    }

    /// Record additional field(s) on an existing span.
    ///
    /// By default, this appends a space to the current set of fields if it is
    /// non-empty, and then calls `self.format_fields`. If different behavior is
    /// required, the default implementation of this method can be overridden.
    fn add_fields(
        &self,
        current: &'a mut FormattedFields<Self>,
        fields: &Record<'_>,
    ) -> fmt::Result {
        if current.is_empty() {
            // If there are no previously recorded fields, we can just reuse the
            // existing string.
            let mut writer = current.as_writer();
            let mut visitor = JsonVisitor::new(&mut writer);
            fields.record(&mut visitor);
            visitor.finish()?;
            return Ok(());
        }

        // If fields were previously recorded on this span, we need to parse
        // the current set of fields as JSON, add the new fields, and
        // re-serialize them. Otherwise, if we just appended the new fields
        // to a previously serialized JSON object, we would end up with
        // malformed JSON.
        //
        // XXX(eliza): this is far from efficient, but unfortunately, it is
        // necessary as long as the JSON formatter is implemented on top of
        // an interface that stores all formatted fields as strings.
        //
        // We should consider reimplementing the JSON formatter as a
        // separate layer, rather than a formatter for the `fmt` layer —
        // then, we could store fields as JSON values, and add to them
        // without having to parse and re-serialize.
        let mut new = String::new();
        let map: BTreeMap<&'_ str, Value> = serde_json::from_str(current).map_err(|error| {
            drop(error);
            fmt::Error
        })?;
        let mut visitor = JsonVisitor::new(&mut new);
        visitor.values = map;
        fields.record(&mut visitor);
        visitor.finish()?;
        *current.fields_mut() = new;

        Ok(())
    }
}

/// The [visitor] produced by [`JsonFields`]'s [`MakeVisitor`] implementation.
///
/// [visitor]: crate::field::Visit
/// [`MakeVisitor`]: crate::field::MakeVisitor
pub struct JsonVisitor<'a> {
    /// Recorded field values keyed by field name.
    values: BTreeMap<&'a str, Value>,
    /// The writer receiving serialized JSON.
    writer: &'a mut dyn Write,
}

impl Debug for JsonVisitor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("JsonVisitor {{ values: {:?} }}", self.values))
    }
}

impl<'a> JsonVisitor<'a> {
    /// Returns a new default visitor that formats to the provided `writer`.
    ///
    /// # Arguments
    /// - `writer`: the writer to format to.
    /// - `is_empty`: whether or not any fields have been previously written to
    ///   that writer.
    pub fn new(writer: &'a mut dyn Write) -> Self {
        Self {
            values: BTreeMap::new(),
            writer,
        }
    }
}

impl VisitFmt for JsonVisitor<'_> {
    fn writer(&mut self) -> &mut dyn Write {
        self.writer
    }
}

impl VisitOutput<fmt::Result> for JsonVisitor<'_> {
    fn finish(self) -> fmt::Result {
        let inner = || {
            let mut serializer = JsonSerializer::new(WriteAdaptor::new(self.writer));
            let mut ser_map = serializer.serialize_map(None)?;

            for (key, value) in self.values {
                ser_map.serialize_entry(key, &value)?;
            }

            SerializeMap::end(ser_map)
        };

        if inner().is_err() {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl Visit for JsonVisitor<'_> {
    #[cfg(all(tracing_unstable, feature = "valuable"))]
    fn record_value(&mut self, field: &Field, value: valuable_crate::Value<'_>) {
        let value = match serde_json::to_value(valuable_serde::Serializable::new(value)) {
            Ok(value) => value,
            Err(_e) => {
                #[cfg(debug_assertions)]
                unreachable!(
                    "`valuable::Valuable` implementations should always serialize \
                    successfully, but an error occurred: {}",
                    _e,
                );

                #[cfg(not(debug_assertions))]
                return;
            }
        };

        let _previous = self.values.insert(field.name(), value);
    }

    /// Visit a double precision floating point value.
    fn record_f64(&mut self, field: &Field, value: f64) {
        let _previous = self
            .values
            .insert(field.name(), Value::from(value));
    }

    /// Visit a signed 64-bit integer value.
    fn record_i64(&mut self, field: &Field, value: i64) {
        let _previous = self
            .values
            .insert(field.name(), Value::from(value));
    }

    /// Visit an unsigned 64-bit integer value.
    fn record_u64(&mut self, field: &Field, value: u64) {
        let _previous = self
            .values
            .insert(field.name(), Value::from(value));
    }

    /// Visit a boolean value.
    fn record_bool(&mut self, field: &Field, value: bool) {
        let _previous = self
            .values
            .insert(field.name(), Value::from(value));
    }

    /// Visit a string value.
    fn record_str(&mut self, field: &Field, value: &str) {
        let _previous = self
            .values
            .insert(field.name(), Value::from(value));
    }

    fn record_bytes(&mut self, field: &Field, value: &[u8]) {
        let _previous = self
            .values
            .insert(field.name(), Value::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        match field.name() {
            // Skip fields that are actually log metadata that have already been handled
            #[cfg(feature = "tracing-log")]
            name if name.starts_with("log.") => (),
            name if name.starts_with("r#") => {
                let raw_name = name.strip_prefix("r#").unwrap_or(name);
                let _previous = self
                    .values
                    .insert(raw_name, Value::from(format!("{value:?}")));
            }
            name => {
                let _previous = self
                    .values
                    .insert(name, Value::from(format!("{value:?}")));
            }
        }
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use crate::fmt::{format::FmtSpan, test::MockMakeWriter, time::FormatTime, SubscriberBuilder};
    use core::fmt::Result as FmtResult;
    use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok, ensure_some};
    use tracing::{self, field::Empty, subscriber::with_default};

    use std::{collections::HashMap, path::Path, str};

    struct MockTime;
    impl FormatTime for MockTime {
        fn format_time(&self, writer: &mut Writer<'_>) -> FmtResult {
            write!(writer, "fake time")
        }
    }

    fn subscriber() -> SubscriberBuilder<JsonFields, Format<Json>> {
        SubscriberBuilder::default().json()
    }

    #[test]
    fn json() -> Result<(), TestFailure> {
        let expected =
        "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"span\":{\"answer\":42,\"name\":\"json_span\",\"number\":3,\"slice\":[97,98,99]},\"spans\":[{\"answer\":42,\"name\":\"json_span\",\"number\":3,\"slice\":[97,98,99]}],\"target\":\"tracing_subscriber::fmt::format::json::test\",\"fields\":{\"message\":\"some json test\"}}\n";
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(true)
            .with_span_list(true);
        test_json(expected, subscriber, || {
            let span = tracing::span!(
                tracing::Level::INFO,
                "json_span",
                answer = 42,
                number = 3,
                slice = &b"abc"[..]
            );
            let _span_guard = span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_filename() -> Result<(), TestFailure> {
        let current_path = current_path()?
            // escape windows backslashes
            .replace('\\', "\\\\");
        let expected = &format!(
            "{}{}{}",
            "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"span\":{\"answer\":42,\"name\":\"json_span\",\"number\":3},\"spans\":[{\"answer\":42,\"name\":\"json_span\",\"number\":3}],\"target\":\"tracing_subscriber::fmt::format::json::test\",\"filename\":\"",
            current_path,
            "\",\"fields\":{\"message\":\"some json test\"}}\n"
        );
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(true)
            .with_file(true)
            .with_span_list(true);
        test_json(expected, subscriber, || {
            let span = tracing::span!(tracing::Level::INFO, "json_span", answer = 42, number = 3);
            let _span_guard = span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_line_number() -> Result<(), TestFailure> {
        let expected =
            "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"span\":{\"answer\":42,\"name\":\"json_span\",\"number\":3},\"spans\":[{\"answer\":42,\"name\":\"json_span\",\"number\":3}],\"target\":\"tracing_subscriber::fmt::format::json::test\",\"line_number\":42,\"fields\":{\"message\":\"some json test\"}}\n";
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(true)
            .with_line_number(true)
            .with_span_list(true);
        test_json_with_line_number(expected, subscriber, || {
            let span = tracing::span!(tracing::Level::INFO, "json_span", answer = 42, number = 3);
            let _span_guard = span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_flattened_event() -> Result<(), TestFailure> {
        let expected =
        "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"span\":{\"answer\":42,\"name\":\"json_span\",\"number\":3},\"spans\":[{\"answer\":42,\"name\":\"json_span\",\"number\":3}],\"target\":\"tracing_subscriber::fmt::format::json::test\",\"message\":\"some json test\"}\n";

        let subscriber = subscriber()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(true);
        test_json(expected, subscriber, || {
            let span = tracing::span!(tracing::Level::INFO, "json_span", answer = 42, number = 3);
            let _span_guard = span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_disabled_current_span_event() -> Result<(), TestFailure> {
        let expected =
        "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"spans\":[{\"answer\":42,\"name\":\"json_span\",\"number\":3}],\"target\":\"tracing_subscriber::fmt::format::json::test\",\"fields\":{\"message\":\"some json test\"}}\n";
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(false)
            .with_span_list(true);
        test_json(expected, subscriber, || {
            let span = tracing::span!(tracing::Level::INFO, "json_span", answer = 42, number = 3);
            let _span_guard = span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_disabled_span_list_event() -> Result<(), TestFailure> {
        let expected =
        "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"span\":{\"answer\":42,\"name\":\"json_span\",\"number\":3},\"target\":\"tracing_subscriber::fmt::format::json::test\",\"fields\":{\"message\":\"some json test\"}}\n";
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(true)
            .with_span_list(false);
        test_json(expected, subscriber, || {
            let span = tracing::span!(tracing::Level::INFO, "json_span", answer = 42, number = 3);
            let _span_guard = span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_nested_span() -> Result<(), TestFailure> {
        let expected =
        "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"span\":{\"answer\":43,\"name\":\"nested_json_span\",\"number\":4},\"spans\":[{\"answer\":42,\"name\":\"json_span\",\"number\":3},{\"answer\":43,\"name\":\"nested_json_span\",\"number\":4}],\"target\":\"tracing_subscriber::fmt::format::json::test\",\"fields\":{\"message\":\"some json test\"}}\n";
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(true)
            .with_span_list(true);
        test_json(expected, subscriber, || {
            let parent_span =
                tracing::span!(tracing::Level::INFO, "json_span", answer = 42, number = 3);
            let _parent_guard = parent_span.enter();
            let nested_span = tracing::span!(
                tracing::Level::INFO,
                "nested_json_span",
                answer = 43,
                number = 4
            );
            let _nested_guard = nested_span.enter();
            tracing::info!("some json test");
        })
    }

    #[test]
    fn json_no_span() -> Result<(), TestFailure> {
        let expected =
        "{\"timestamp\":\"fake time\",\"level\":\"INFO\",\"target\":\"tracing_subscriber::fmt::format::json::test\",\"fields\":{\"message\":\"some json test\"}}\n";
        let subscriber = subscriber()
            .flatten_event(false)
            .with_current_span(true)
            .with_span_list(true);
        test_json(expected, subscriber, || {
            tracing::info!("some json test");
        })
    }

    #[test]
    fn record_works() -> Result<(), TestFailure> {
        // This test reproduces issue #707, where using `Span::record` causes
        // any events inside the span to be ignored.

        let make_writer = MockMakeWriter::default();
        let subscriber = crate::fmt()
            .json()
            .with_writer(make_writer.clone())
            .finish();

        with_default(subscriber, || -> Result<(), TestFailure> {
            tracing::info!("an event outside the root span");
            ensure_json_path_eq(
                &parse_as_json(&make_writer)?,
                &["fields", "message"],
                "an event outside the root span",
                "outside-root event message matches",
            )?;

            let span = tracing::info_span!("the span", na = Empty);
            let _span = span.record("na", "value");
            let _enter = span.enter();

            tracing::info!("an event inside the root span");
            ensure_json_path_eq(
                &parse_as_json(&make_writer)?,
                &["fields", "message"],
                "an event inside the root span",
                "inside-root event message matches",
            )
        })
    }

    #[test]
    fn json_span_event_show_correct_context() -> Result<(), TestFailure> {
        let buffer = MockMakeWriter::default();
        let subscriber = subscriber()
            .with_writer(buffer.clone())
            .flatten_event(false)
            .with_current_span(true)
            .with_span_list(false)
            .with_span_events(FmtSpan::FULL)
            .finish();

        with_default(subscriber, || -> Result<(), TestFailure> {
            let parent_context = "parent";
            let parent_span = tracing::info_span!("parent_span", context = parent_context);

            ensure_span_event(&buffer, "new", "parent")?;

            let parent_enter = parent_span.enter();
            ensure_span_event(&buffer, "enter", "parent")?;

            let child_context = "child";
            let child_span = tracing::info_span!("child_span", context = child_context);
            ensure_span_event(&buffer, "new", "child")?;

            let child_enter = child_span.enter();
            ensure_span_event(&buffer, "enter", "child")?;

            drop(child_enter);
            ensure_span_event(&buffer, "exit", "child")?;

            drop(child_span);
            ensure_span_event(&buffer, "close", "child")?;

            drop(parent_enter);
            ensure_span_event(&buffer, "exit", "parent")?;

            drop(parent_span);
            ensure_span_event(&buffer, "close", "parent")
        })
    }

    #[test]
    fn json_span_event_with_no_fields() -> Result<(), TestFailure> {
        // Check span events serialize correctly.
        // Discussion: https://github.com/tokio-rs/tracing/issues/829#issuecomment-661984255
        let buffer = MockMakeWriter::default();
        let subscriber = subscriber()
            .with_writer(buffer.clone())
            .flatten_event(false)
            .with_current_span(false)
            .with_span_list(false)
            .with_span_events(FmtSpan::FULL)
            .finish();

        with_default(subscriber, || -> Result<(), TestFailure> {
            let span = tracing::info_span!("valid_json");
            ensure_json_message(&buffer, "new")?;

            let enter = span.enter();
            ensure_json_message(&buffer, "enter")?;

            drop(enter);
            ensure_json_message(&buffer, "exit")?;

            drop(span);
            ensure_json_message(&buffer, "close")
        })
    }

    fn parse_as_json(buffer: &MockMakeWriter) -> Result<Value, TestFailure> {
        let buf = ensure_ok(
            String::from_utf8(buffer.buf().to_vec()),
            "json buffer is valid utf8",
        )?;
        let json = ensure_some(buf.lines().last(), "json buffer contains a line")?;
        ensure_ok(serde_json::from_str(json), "json line parses")
    }

    fn ensure_json_message(buffer: &MockMakeWriter, expected: &'static str) -> Result<(), TestFailure> {
        ensure_json_path_eq(
            &parse_as_json(buffer)?,
            &["fields", "message"],
            expected,
            "json span event message matches",
        )
    }

    fn ensure_span_event(
        buffer: &MockMakeWriter,
        expected_message: &'static str,
        expected_context: &'static str,
    ) -> Result<(), TestFailure> {
        let event = parse_as_json(buffer)?;
        ensure_json_path_eq(
            &event,
            &["fields", "message"],
            expected_message,
            "json span event message matches",
        )?;
        ensure_json_path_eq(
            &event,
            &["span", "context"],
            expected_context,
            "json span event context matches",
        )
    }

    fn ensure_json_path_eq(
        value: &Value,
        path: &'static [&'static str],
        expected: &'static str,
        context: &'static str,
    ) -> Result<(), TestFailure> {
        let actual = json_path(value, path)?;
        ensure_eq(actual, &Value::from(expected), context)
    }

    #[allow(
        clippy::single_call_fn,
        reason = "JSON tests keep nested path traversal as a named validation helper"
    )]
    fn json_path<'a>(
        value: &'a Value,
        path: &'static [&'static str],
    ) -> Result<&'a Value, TestFailure> {
        let mut current = value;
        for segment in path {
            let object = ensure_some(current.as_object(), "json path segment is an object")?;
            current = ensure_some(object.get(*segment), "json path segment exists")?;
        }
        Ok(current)
    }

    fn test_json<T>(
        expected: &str,
        builder: SubscriberBuilder<JsonFields, Format<Json>>,
        producer: impl FnOnce() -> T,
    ) -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder
            .with_writer(make_writer.clone())
            .with_timer(MockTime)
            .finish();

        let _producer_output = with_default(subscriber, producer);

        let actual = {
            let buf = make_writer.buf();
            ensure_ok(str::from_utf8(&buf[..]), "json output is valid utf8")?.to_owned()
        };
        let expected_json = ensure_ok(
            serde_json::from_str::<HashMap<String, Value>>(expected),
            "expected json parses",
        )?;
        let actual_json = ensure_ok(
            serde_json::from_str::<HashMap<String, Value>>(&actual),
            "actual json parses",
        )?;
        ensure(actual_json == expected_json, "actual json matches expected json")
    }

    #[allow(
        clippy::single_call_fn,
        reason = "JSON tests isolate line-number normalization from exact-output assertions"
    )]
    fn test_json_with_line_number<T>(
        expected: &str,
        builder: SubscriberBuilder<JsonFields, Format<Json>>,
        producer: impl FnOnce() -> T,
    ) -> Result<(), TestFailure> {
        let make_writer = MockMakeWriter::default();
        let subscriber = builder
            .with_writer(make_writer.clone())
            .with_timer(MockTime)
            .finish();

        let _producer_output = with_default(subscriber, producer);

        let actual = {
            let buf = make_writer.buf();
            ensure_ok(str::from_utf8(&buf[..]), "json output is valid utf8")?.to_owned()
        };
        let mut expected_json = ensure_ok(
            serde_json::from_str::<HashMap<String, Value>>(expected),
            "expected json parses",
        )?;
        let expect_line_number = expected_json.remove("line_number").is_some();
        let mut actual_json = ensure_ok(
            serde_json::from_str::<HashMap<String, Value>>(&actual),
            "actual json parses",
        )?;
        let line_number = actual_json.remove("line_number");
        if expect_line_number {
            ensure(
                line_number.is_some_and(|value| value.is_number()),
                "line number is present and numeric",
            )?;
        } else {
            ensure(line_number.is_none(), "line number is absent")?;
        }
        ensure(
            actual_json == expected_json,
            "actual json without line number matches expected json",
        )
    }

    #[allow(
        clippy::single_call_fn,
        reason = "JSON tests keep platform-specific expected path construction named"
    )]
    fn current_path() -> Result<String, TestFailure> {
        let owned_path = Path::new("tracing-subscriber")
            .join("src")
            .join("fmt")
            .join("format")
            .join("json.rs");
        let path_str = ensure_some(owned_path.to_str(), "json test path is valid unicode")?;
        Ok(path_str.to_owned())
    }
}
