//! Adapters for connecting unstructured log records from the `log` crate into
//! the `tracing` ecosystem.
//!
//! # Overview
//!
//! [`tracing`] is a framework for instrumenting Rust programs with context-aware,
//! structured, event-based diagnostic information. This crate provides
//! compatibility layers for using `tracing` alongside the logging facade provided
//! by the [`log`] crate.
//!
//! This crate provides:
//!
//! - [`AsTrace`] and [`AsLog`] traits for converting between `tracing` and `log` types.
//! - [`LogTracer`], a [`log::Log`] implementation that consumes [`log::Record`]s
//!   and outputs them as [`tracing::Event`].
//!
//! *Compiler support: [requires `rustc` 1.96+][msrv]*
//!
//! [msrv]: #supported-rust-versions
//!
//! # Usage
//!
//! ## Convert log records to tracing `Event`s
//!
//! To convert [`log::Record`]s as [`tracing::Event`]s, set `LogTracer` as the default
//! logger by calling its [`init`] or [`init_with_filter`] methods.
//!
//! ```rust
//! # use std::error::Error;
//! use tracing_log::LogTracer;
//! use log;
//!
//! # fn main() -> Result<(), Box<dyn Error>> {
//! LogTracer::init()?;
//!
//! // will be available for Subscribers as a tracing Event
//! log::trace!("an example trace log");
//! # Ok(())
//! # }
//! ```
//!
//! This conversion does not convert unstructured data in log records (such as
//! values passed as format arguments to the `log!` macro) to structured
//! `tracing` fields. However, it *does* attach these new events to to the
//! span that was currently executing when the record was logged. This is the
//! primary use-case for this library: making it possible to locate the log
//! records emitted by dependencies which use `log` within the context of a
//! trace.
//!
//! ## Convert tracing `Event`s to logs
//!
//! Enabling the ["log" and "log-always" feature flags][flags] on the `tracing`
//! crate will cause all `tracing` spans and events to emit `log::Record`s as
//! they occur.
//!
//! ## Caution: Mixing both conversions
//!
//! Note that `log::Logger` implementations that convert log records to trace events
//! should not be used with `Subscriber`s that convert trace events _back_ into
//! `log` records, as doing so will result in the event recursing between the subscriber
//! and the logger forever (or, in real life, probably overflowing the call stack).
//!
//! If the logging of trace events generated from log records produced by the
//! `log` crate is desired, either the `log` crate should not be used to
//! implement this logging, or an additional layer of filtering will be
//! required to avoid infinitely converting between `Event` and `log::Record`.
//!
//! ## Feature Flags
//!
//! * `std`: enables features that require the Rust standard library (on by default)
//! * `log-tracer`: enables the `LogTracer` type (on by default)
//! * `interest-cache`: makes it possible to configure an interest cache for
//!   logs emitted through the `log` crate (see [`Builder::with_interest_cache`]); requires `std`
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
//!
//! [`init`]: LogTracer::init
//! [`init_with_filter`]: LogTracer::init_with_filter
//! [`tracing`]: https://crates.io/crates/tracing
//! [`tracing::Subscriber`]: https://docs.rs/tracing/latest/tracing/trait.Subscriber.html
//! [`Subscriber`]: https://docs.rs/tracing/latest/tracing/trait.Subscriber.html
//! [`tracing::Event`]: https://docs.rs/tracing/latest/tracing/struct.Event.html
//! [flags]: https://docs.rs/tracing/latest/tracing/#crate-feature-flags
//! [`Builder::with_interest_cache`]: log_tracer::Builder::with_interest_cache
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/logo-type.png",
    html_favicon_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/favicon.ico",
    issue_tracker_base_url = "https://github.com/strict-rs/strict-tracing/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg), deny(rustdoc::broken_intra_doc_links))]
use std::{borrow::Cow, fmt, io, sync::LazyLock};

use tracing_core::{
    Event, Metadata,
    callsite::{self, Callsite},
    dispatcher,
    field::{self, Field, Visit},
    identify_callsite,
    metadata::{Kind, Level, LevelFilter},
    subscriber,
};

#[cfg(feature = "log-tracer")]
#[cfg_attr(docsrs, doc(cfg(feature = "log-tracer")))]
pub mod log_tracer;

#[cfg(feature = "log-tracer")]
#[cfg_attr(docsrs, doc(cfg(feature = "log-tracer")))]
#[doc(inline)]
pub use self::log_tracer::LogTracer;

pub use log;

#[cfg(all(feature = "interest-cache", feature = "log-tracer", feature = "std"))]
/// Per-thread interest cache for log metadata filtering.
#[doc(hidden)]
pub mod interest_cache;

#[cfg(all(feature = "interest-cache", feature = "log-tracer", feature = "std"))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(feature = "interest-cache", feature = "log-tracer", feature = "std")))
)]
pub use crate::interest_cache::InterestCacheConfig;

/// Format a log record as a trace event in the current span.
///
/// # Errors
///
/// Returns [`io::ErrorKind::InvalidData`] if the crate's synthetic log
/// callsite metadata is missing one of the fields required to reconstruct a
/// `tracing` event.
pub fn format_trace(record: &log::Record<'_>) -> io::Result<()> {
    dispatch_record(record)
}

/// Dispatch a log record into the current tracing dispatcher.
pub(crate) fn dispatch_record(record: &log::Record<'_>) -> io::Result<()> {
    dispatcher::get_default(|dispatch| {
        let filter_meta = record.as_trace();
        if !dispatch.enabled(&filter_meta).unwrap_or(false) {
            return Ok(());
        }

        let (_, meta) = loglevel_to_cs(record.level());
        let keys = loglevel_to_fields(record.level())?;

        let log_module = record.module_path();
        let log_file = record.file();
        let log_line = record.line();

        let module = log_module.as_ref().map(|module_path| {
            let value: &dyn field::Value = module_path;
            value
        });
        let file = log_file.as_ref().map(|file_path| {
            let value: &dyn field::Value = file_path;
            value
        });
        let line = log_line.as_ref().map(|line_number| {
            let value: &dyn field::Value = line_number;
            value
        });
        let message: &dyn field::Value = record.args();

        let _delivery_result = dispatch.event(&Event::new(
            meta,
            &meta.fields().value_set(&[
                (&keys.message, Some(message)),
                (&keys.target, Some(&record.target())),
                (&keys.module, module),
                (&keys.file, file),
                (&keys.line, line),
            ]),
        ));

        Ok(())
    })
}

/// Trait implemented for `tracing` types that can be converted to a `log`
/// equivalent.
pub trait AsLog: sealed::Sealed {
    /// The `log` type that this type can be converted into.
    type Log;
    /// Returns the `log` equivalent of `self`.
    fn as_log(&self) -> Self::Log;
}

/// Trait implemented for `log` types that can be converted to a `tracing`
/// equivalent.
pub trait AsTrace: sealed::Sealed {
    /// The `tracing` type that this type can be converted into.
    type Trace;
    /// Returns the `tracing` equivalent of `self`.
    fn as_trace(&self) -> Self::Trace;
}

impl sealed::Sealed for Metadata<'_> {}

impl<'a> AsLog for Metadata<'a> {
    type Log = log::Metadata<'a>;
    fn as_log(&self) -> Self::Log {
        log::Metadata::builder()
            .level(self.level().as_log())
            .target(self.target())
            .build()
    }
}
impl sealed::Sealed for log::Metadata<'_> {}

impl<'a> AsTrace for log::Metadata<'a> {
    type Trace = Metadata<'a>;
    fn as_trace(&self) -> Self::Trace {
        let cs_id = identify_callsite!(loglevel_to_cs(self.level()).0);
        Metadata::new(
            "log record",
            self.target(),
            self.level().as_trace(),
            None,
            None,
            None,
            &field::FieldSet::new(FIELD_NAMES, cs_id),
            Kind::EVENT,
        )
    }
}

/// Field handles used by the synthetic log callsites.
struct Fields {
    /// Field containing formatted log arguments.
    message: Field,
    /// Field containing the original log target.
    target: Field,
    /// Field containing the original Rust module path.
    module: Field,
    /// Field containing the original source file.
    file: Field,
    /// Field containing the original source line.
    line: Field,
}

/// Field names attached to every synthetic log callsite.
static FIELD_NAMES: &[&str] = &[
    "message",
    "log.target",
    "log.module_path",
    "log.file",
    "log.line",
];

impl Fields {
    /// Build field handles from a synthetic log callsite's metadata.
    fn new(cs: &'static dyn Callsite) -> Result<Self, io::ErrorKind> {
        let fieldset = cs.metadata().fields();
        let Some(message) = fieldset.field("message") else {
            return Err(io::ErrorKind::InvalidData);
        };
        let Some(target) = fieldset.field("log.target") else {
            return Err(io::ErrorKind::InvalidData);
        };
        let Some(module) = fieldset.field("log.module_path") else {
            return Err(io::ErrorKind::InvalidData);
        };
        let Some(file) = fieldset.field("log.file") else {
            return Err(io::ErrorKind::InvalidData);
        };
        let Some(line) = fieldset.field("log.line") else {
            return Err(io::ErrorKind::InvalidData);
        };
        Ok(Self {
            message,
            target,
            module,
            file,
            line,
        })
    }
}

/// Declare one synthetic callsite and its metadata for a log level.
macro_rules! log_cs {
    ($level:expr, $cs:ident, $meta:ident, $ty:ident) => {
        struct $ty;
        static $cs: $ty = $ty;
        static $meta: Metadata<'static> = Metadata::new(
            "log event",
            "log",
            $level,
            ::core::option::Option::None,
            ::core::option::Option::None,
            ::core::option::Option::None,
            &field::FieldSet::new(FIELD_NAMES, identify_callsite!(&$cs)),
            Kind::EVENT,
        );

        impl callsite::Callsite for $ty {
            fn set_interest(&self, _: subscriber::Interest) {}
            fn metadata(&self) -> &'static Metadata<'static> {
                &$meta
            }
        }
    };
}

log_cs!(Level::TRACE, TRACE_CS, TRACE_META, TraceCallsite);
log_cs!(Level::DEBUG, DEBUG_CS, DEBUG_META, DebugCallsite);
log_cs!(Level::INFO, INFO_CS, INFO_META, InfoCallsite);
log_cs!(Level::WARN, WARN_CS, WARN_META, WarnCallsite);
log_cs!(Level::ERROR, ERROR_CS, ERROR_META, ErrorCallsite);

/// Field handles for the trace-level synthetic callsite.
static TRACE_FIELDS: LazyLock<Result<Fields, io::ErrorKind>> =
    LazyLock::new(|| Fields::new(&TRACE_CS));
/// Field handles for the debug-level synthetic callsite.
static DEBUG_FIELDS: LazyLock<Result<Fields, io::ErrorKind>> =
    LazyLock::new(|| Fields::new(&DEBUG_CS));
/// Field handles for the info-level synthetic callsite.
static INFO_FIELDS: LazyLock<Result<Fields, io::ErrorKind>> =
    LazyLock::new(|| Fields::new(&INFO_CS));
/// Field handles for the warn-level synthetic callsite.
static WARN_FIELDS: LazyLock<Result<Fields, io::ErrorKind>> =
    LazyLock::new(|| Fields::new(&WARN_CS));
/// Field handles for the error-level synthetic callsite.
static ERROR_FIELDS: LazyLock<Result<Fields, io::ErrorKind>> =
    LazyLock::new(|| Fields::new(&ERROR_CS));

/// Return the synthetic tracing callsite for a tracing level.
#[allow(
    clippy::single_call_fn,
    reason = "NormalizeEvent uses this named lookup for identify_callsite comparisons against synthetic trace callsites"
)]
fn level_to_cs(level: Level) -> &'static dyn Callsite {
    match level {
        Level::TRACE => &TRACE_CS,
        Level::DEBUG => &DEBUG_CS,
        Level::INFO => &INFO_CS,
        Level::WARN => &WARN_CS,
        Level::ERROR => &ERROR_CS,
    }
}

/// Return the synthetic field handles for a tracing level.
#[allow(
    clippy::single_call_fn,
    reason = "normalized metadata keeps trace-level field handles aligned with synthetic callsites through this lookup"
)]
fn level_to_fields(level: Level) -> io::Result<&'static Fields> {
    let fields = match level {
        Level::TRACE => &TRACE_FIELDS,
        Level::DEBUG => &DEBUG_FIELDS,
        Level::INFO => &INFO_FIELDS,
        Level::WARN => &WARN_FIELDS,
        Level::ERROR => &ERROR_FIELDS,
    };
    field_handles(fields)
}

/// Return the synthetic tracing callsite for a log level.
#[allow(
    clippy::single_call_fn,
    reason = "log-to-trace conversion uses this named lookup for identify_callsite and dispatch metadata"
)]
fn loglevel_to_cs(level: log::Level) -> (&'static dyn Callsite, &'static Metadata<'static>) {
    match level {
        log::Level::Trace => (&TRACE_CS, &TRACE_META),
        log::Level::Debug => (&DEBUG_CS, &DEBUG_META),
        log::Level::Info => (&INFO_CS, &INFO_META),
        log::Level::Warn => (&WARN_CS, &WARN_META),
        log::Level::Error => (&ERROR_CS, &ERROR_META),
    }
}

/// Return the synthetic field handles for a log level.
#[allow(
    clippy::single_call_fn,
    reason = "dispatch_record keeps log-level field handles aligned with synthetic callsites through this lookup"
)]
fn loglevel_to_fields(level: log::Level) -> io::Result<&'static Fields> {
    let fields = match level {
        log::Level::Trace => &TRACE_FIELDS,
        log::Level::Debug => &DEBUG_FIELDS,
        log::Level::Info => &INFO_FIELDS,
        log::Level::Warn => &WARN_FIELDS,
        log::Level::Error => &ERROR_FIELDS,
    };
    field_handles(fields)
}

/// Borrow initialized field handles or convert the stored error kind.
fn field_handles(
    field_result: &'static Result<Fields, io::ErrorKind>,
) -> io::Result<&'static Fields> {
    match field_result.as_ref() {
        Ok(handles) => Ok(handles),
        Err(error_kind) => Err((*error_kind).into()),
    }
}

impl sealed::Sealed for log::Record<'_> {}

impl<'a> AsTrace for log::Record<'a> {
    type Trace = Metadata<'a>;
    fn as_trace(&self) -> Self::Trace {
        let cs_id = identify_callsite!(loglevel_to_cs(self.level()).0);
        Metadata::new(
            "log record",
            self.target(),
            self.level().as_trace(),
            self.file(),
            self.line(),
            self.module_path(),
            &field::FieldSet::new(FIELD_NAMES, cs_id),
            Kind::EVENT,
        )
    }
}

impl sealed::Sealed for Level {}

impl AsLog for Level {
    type Log = log::Level;
    fn as_log(&self) -> log::Level {
        match *self {
            Self::ERROR => log::Level::Error,
            Self::WARN => log::Level::Warn,
            Self::INFO => log::Level::Info,
            Self::DEBUG => log::Level::Debug,
            Self::TRACE => log::Level::Trace,
        }
    }
}

impl sealed::Sealed for log::Level {}

impl AsTrace for log::Level {
    type Trace = Level;
    #[inline]
    fn as_trace(&self) -> Level {
        match *self {
            Self::Error => Level::ERROR,
            Self::Warn => Level::WARN,
            Self::Info => Level::INFO,
            Self::Debug => Level::DEBUG,
            Self::Trace => Level::TRACE,
        }
    }
}

impl sealed::Sealed for log::LevelFilter {}

impl AsTrace for log::LevelFilter {
    type Trace = LevelFilter;
    #[inline]
    fn as_trace(&self) -> LevelFilter {
        match *self {
            Self::Off => LevelFilter::OFF,
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
            Self::Trace => LevelFilter::TRACE,
        }
    }
}

impl sealed::Sealed for LevelFilter {}

impl AsLog for LevelFilter {
    type Log = log::LevelFilter;
    #[inline]
    fn as_log(&self) -> Self::Log {
        match *self {
            Self::OFF => log::LevelFilter::Off,
            Self::ERROR => log::LevelFilter::Error,
            Self::WARN => log::LevelFilter::Warn,
            Self::INFO => log::LevelFilter::Info,
            Self::DEBUG => log::LevelFilter::Debug,
            Self::TRACE => log::LevelFilter::Trace,
        }
    }
}
/// Field names used by normalized metadata views.
static NORMALIZED_FIELD_NAMES: &[&str] = &["message"];

/// Metadata reconstructed from a `log` event.
///
/// This owns metadata strings that were stored as fields on a `tracing`
/// [`Event`], avoiding fabricated lifetimes while still allowing callers to
/// borrow a short-lived [`Metadata`] view with [`as_metadata`].
///
/// [`as_metadata`]: Self::as_metadata
#[derive(Clone, Debug)]
pub struct NormalizedMetadata<'a> {
    /// Metadata name.
    name: &'static str,
    /// Event target.
    target: Cow<'a, str>,
    /// Event level.
    level: Level,
    /// Optional module path.
    module_path: Option<Cow<'a, str>>,
    /// Optional source file.
    file: Option<Cow<'a, str>>,
    /// Optional source line.
    line: Option<u32>,
    /// Original callsite identifier.
    callsite: callsite::Identifier,
    /// Original metadata kind.
    kind: Kind,
}

impl NormalizedMetadata<'_> {
    /// Returns this metadata as a short-lived [`Metadata`] value.
    #[must_use]
    pub fn as_metadata(&self) -> Metadata<'_> {
        Metadata::new(
            self.name,
            self.target(),
            self.level,
            self.file(),
            self.line,
            self.module_path(),
            &field::FieldSet::new(NORMALIZED_FIELD_NAMES, self.callsite),
            self.kind,
        )
    }

    /// Returns the name of the event.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the target associated with the event.
    #[must_use]
    pub fn target(&self) -> &str {
        self.target.as_ref()
    }

    /// Returns the verbosity level of the event.
    #[must_use]
    pub const fn level(&self) -> &Level {
        &self.level
    }

    /// Returns the module path associated with the event, if known.
    #[must_use]
    pub fn module_path(&self) -> Option<&str> {
        self.module_path.as_deref()
    }

    /// Returns the source file associated with the event, if known.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Returns the source line associated with the event, if known.
    #[must_use]
    pub const fn line(&self) -> Option<u32> {
        self.line
    }

    /// Returns the callsite associated with the original event.
    #[must_use]
    pub const fn callsite(&self) -> callsite::Identifier {
        self.callsite
    }

    /// Returns the field set used by the normalized metadata view.
    #[must_use]
    pub const fn fields(&self) -> field::FieldSet {
        field::FieldSet::new(NORMALIZED_FIELD_NAMES, self.callsite)
    }

    /// Returns true if the normalized metadata describes an event.
    #[must_use]
    pub const fn is_event(&self) -> bool {
        self.kind.is_event()
    }

    /// Returns true if the normalized metadata describes a span.
    #[must_use]
    pub const fn is_span(&self) -> bool {
        self.kind.is_span()
    }
}

/// Extends log `Event`s to provide complete `Metadata`.
///
/// In `tracing-log`, an `Event` produced by a log (through [`AsTrace`]) has an hard coded
/// "log" target and no `file`, `line`, or `module_path` attributes. This happens because `Event`
/// requires its `Metadata` to be `'static`, while [`log::Record`]s provide them with a generic
/// lifetime.
///
/// However, these values are stored in the `Event`'s fields and
/// the [`normalized_metadata`] method allows building an owned
/// [`NormalizedMetadata`] value with complete data and a short-lived
/// [`Metadata`] view.
///
/// It can typically be used by `Subscriber`s when processing an `Event`,
/// to allow accessing its complete metadata in a consistent way,
/// regardless of the source of its source.
///
/// [`normalized_metadata`]: NormalizeEvent#normalized_metadata
pub trait NormalizeEvent<'a>: sealed::Sealed {
    /// If this `Event` comes from a `log`, this method provides a new
    /// normalized `Metadata` which has all available attributes
    /// from the original log, including `file`, `line`, `module_path`
    /// and `target`.
    /// Returns `None` is the `Event` is not issued from a `log`.
    fn normalized_metadata(&'a self) -> Option<NormalizedMetadata<'a>>;
    /// Returns whether this `Event` represents a log (from the `log` crate)
    fn is_log(&self) -> bool;
}

impl sealed::Sealed for Event<'_> {}

impl<'a> NormalizeEvent<'a> for Event<'a> {
    fn normalized_metadata(&'a self) -> Option<NormalizedMetadata<'a>> {
        let original = self.metadata();
        if !self.is_log() {
            return None;
        }

        let Ok(field_handles) = level_to_fields(*original.level()) else {
            return None;
        };
        let mut visitor = LogVisitor {
            target: None,
            module_path: None,
            file: None,
            line: None,
            fields: field_handles,
        };
        self.record(&mut visitor);

        Some(NormalizedMetadata {
            name: "log event",
            target: visitor
                .target
                .map_or_else(|| Cow::Borrowed("log"), Cow::Owned),
            level: *original.level(),
            file: visitor.file.map(Cow::Owned),
            line: visitor.line.and_then(|line| u32::try_from(line).ok()),
            module_path: visitor.module_path.map(Cow::Owned),
            callsite: original.callsite(),
            kind: Kind::EVENT,
        })
    }

    fn is_log(&self) -> bool {
        self.metadata().callsite() == identify_callsite!(level_to_cs(*self.metadata().level()))
    }
}

/// Visitor that reconstructs `log` metadata from tracing event fields.
struct LogVisitor {
    /// Original log target.
    target: Option<String>,
    /// Original module path.
    module_path: Option<String>,
    /// Original source file.
    file: Option<String>,
    /// Original source line.
    line: Option<u64>,
    /// Field handles for the event's synthetic log callsite.
    fields: &'static Fields,
}

impl Visit for LogVisitor {
    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field == &self.fields.line {
            self.line = Some(value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field == &self.fields.file {
            self.file = Some(value.to_owned());
        }
        if field == &self.fields.target {
            self.target = Some(value.to_owned());
        }
        if field == &self.fields.module {
            self.module_path = Some(value.to_owned());
        }
    }
}

/// Sealed trait support for extension traits in this crate.
mod sealed {
    /// Marker trait preventing external implementations.
    pub trait Sealed {}
}

#[cfg(test)]
mod test {

    use super::*;
    use strict_test_support::{TestFailure, ensure, ensure_eq};

    fn test_callsite(level: log::Level) -> Result<(), TestFailure> {
        let record = log::Record::builder()
            .args(format_args!("Error!"))
            .level(level)
            .target("myApp")
            .file(Some("server.rs"))
            .line(Some(144))
            .module_path(Some("server"))
            .build();

        let meta = record.as_trace();
        let (cs, _) = loglevel_to_cs(record.level());
        let cs_meta = cs.metadata();
        ensure(
            meta.callsite() == cs_meta.callsite(),
            "record metadata callsite matches synthetic metadata",
        )?;
        ensure_eq(
            meta.level(),
            &level.as_trace(),
            "metadata level matches log level",
        )
    }

    #[test]
    fn error_callsite_is_correct() -> Result<(), TestFailure> {
        test_callsite(log::Level::Error)
    }

    #[test]
    fn warn_callsite_is_correct() -> Result<(), TestFailure> {
        test_callsite(log::Level::Warn)
    }

    #[test]
    fn info_callsite_is_correct() -> Result<(), TestFailure> {
        test_callsite(log::Level::Info)
    }

    #[test]
    fn debug_callsite_is_correct() -> Result<(), TestFailure> {
        test_callsite(log::Level::Debug)
    }

    #[test]
    fn trace_callsite_is_correct() -> Result<(), TestFailure> {
        test_callsite(log::Level::Trace)
    }
}
