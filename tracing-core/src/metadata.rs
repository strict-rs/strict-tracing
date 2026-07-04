//! Metadata describing trace data.
use core::cmp;
use core::error::Error;
use core::fmt;
use core::ptr;
use core::str::FromStr;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::callsite;
use super::field;

/// Metadata describing a [span] or [event].
///
/// All spans and events have the following metadata:
/// - A [name], represented as a static string.
/// - A [target], a string that categorizes part of the system where the span or event occurred. The
///   `tracing` macros default to using the module path where the span or event originated as the
///   target, but it may be overridden.
/// - A [verbosity level]. This determines how verbose a given span or event is, and allows enabling
///   or disabling more verbose diagnostics situationally. See the documentation for the [`Level`]
///   type for details.
/// - The names of the [fields] defined by the span or event.
/// - Whether the metadata corresponds to a span or event.
///
/// In addition, the following optional metadata describing the source code
/// location where the span or event originated _may_ be provided:
/// - The [file name]
/// - The [line number]
/// - The [module path]
///
/// Metadata is used by [`Subscriber`]s when filtering spans and events, and it
/// may also be used as part of their data payload.
///
/// When created by the `event!` or `span!` macro, the metadata describing a
/// particular event or span is constructed statically and exists as a single
/// static instance. Thus, the overhead of creating the metadata is
/// _significantly_ lower than that of creating the actual span. Therefore,
/// filtering is based on metadata, rather than on the constructed span.
///
/// ## Equality
///
/// In well-behaved applications, two `Metadata` with equal
/// [callsite identifiers] will be equal in all other ways (i.e., have the same
/// `name`, `target`, etc.). Consequently, in release builds, [`Metadata::eq`]
/// *only* checks that its arguments have equal callsites. However, the equality
/// of `Metadata`'s other fields is checked in debug builds.
///
/// [span]: super::span
/// [event]: super::event
/// [name]: Self::name
/// [target]: Self::target
/// [fields]: Self::fields
/// [verbosity level]: Self::level
/// [file name]: Self::file
/// [line number]: Self::line
/// [module path]: Self::module_path
/// [`Subscriber`]: super::subscriber::Subscriber
/// [callsite identifiers]: Self::callsite
pub struct Metadata<'a> {
  /// The name of the span described by this metadata.
  name: &'static str,

  /// The part of the system that the span that this metadata describes
  /// occurred in.
  target: &'a str,

  /// The level of verbosity of the described span.
  level: Level,

  /// The name of the Rust module where the span occurred, or `None` if this
  /// could not be determined.
  module_path: Option<&'a str>,

  /// The name of the source code file where the span occurred, or `None` if
  /// this could not be determined.
  file: Option<&'a str>,

  /// The line number in the source code file where the span occurred, or
  /// `None` if this could not be determined.
  line: Option<u32>,

  /// The names of the key-value fields attached to the described span or
  /// event.
  fields: field::FieldSet,

  /// The kind of the callsite.
  kind: Kind,
}

/// Indicates whether the callsite is a span or event.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Kind(u8);

/// Describes the level of verbosity of a span or event.
///
/// # Comparing Levels
///
/// `Level` implements the [`PartialOrd`] and [`Ord`] traits, allowing two
/// `Level`s to be compared to determine which is considered more or less
/// verbose. Levels which are more verbose are considered "greater than" levels
/// which are less verbose, with [`Level::ERROR`] considered the lowest, and
/// [`Level::TRACE`] considered the highest.
///
/// For example:
/// ```
/// use tracing_core::Level;
///
/// if Level::TRACE <= Level::DEBUG {
///   return Err("TRACE should be more verbose than DEBUG".into());
/// }
/// if Level::ERROR >= Level::WARN {
///   return Err("ERROR should be less verbose than WARN".into());
/// }
/// if Level::INFO > Level::DEBUG {
///   return Err("INFO should not be more verbose than DEBUG".into());
/// }
/// if Level::TRACE != Level::TRACE {
///   return Err("TRACE should equal itself".into());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Filtering
///
/// `Level`s are typically used to implement filtering that determines which
/// spans and events are enabled. Depending on the use case, more or less
/// verbose diagnostics may be desired. For example, when running in
/// development, [`DEBUG`]-level traces may be enabled by default. When running in
/// production, only [`INFO`]-level and lower traces might be enabled. Libraries
/// may include very verbose diagnostics at the [`DEBUG`] and/or [`TRACE`] levels.
/// Applications using those libraries typically chose to ignore those traces. However, when
/// debugging an issue involving said libraries, it may be useful to temporarily
/// enable the more verbose traces.
///
/// The [`LevelFilter`] type is provided to enable filtering traces by
/// verbosity. `Level`s can be compared against [`LevelFilter`]s, and
/// [`LevelFilter`] has a variant for each `Level`, which compares analogously
/// to that level. In addition, [`LevelFilter`] adds a [`LevelFilter::OFF`]
/// variant, which is considered "less verbose" than every other `Level`. This is
/// intended to allow filters to completely disable tracing in a particular context.
///
/// For example:
/// ```
/// use tracing_core::Level;
/// use tracing_core::LevelFilter;
///
/// if LevelFilter::OFF >= Level::TRACE {
///   return Err("OFF should be less verbose than TRACE".into());
/// }
/// if LevelFilter::TRACE <= Level::DEBUG {
///   return Err("TRACE should be more verbose than DEBUG".into());
/// }
/// if LevelFilter::ERROR >= Level::WARN {
///   return Err("ERROR should be less verbose than WARN".into());
/// }
/// if LevelFilter::INFO > Level::DEBUG {
///   return Err("INFO should not be more verbose than DEBUG".into());
/// }
/// if LevelFilter::INFO < Level::INFO {
///   return Err("INFO should compare equal to itself".into());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// ## Examples
///
/// Below is a simple example of how a [`Subscriber`] could implement filtering through
/// a [`LevelFilter`]. When a span or event is recorded, the [`Subscriber::enabled`] method
/// compares the span or event's `Level` against the configured [`LevelFilter`].
/// The optional [`Subscriber::max_level_hint`] method can also be implemented to allow spans
/// and events above a maximum verbosity level to be skipped more efficiently,
/// often improving performance in short-lived programs.
///
/// ```
/// use tracing_core::Event;
/// use tracing_core::Level;
/// use tracing_core::LevelFilter;
/// use tracing_core::Metadata;
/// use tracing_core::Subscriber;
/// use tracing_core::span;
/// # use tracing_core::span::{Id, Record, Current};
/// # use tracing_core::subscriber::SubscriberResult;
///
/// #[derive(Debug)]
/// pub struct MySubscriber {
///   /// The most verbose level that this subscriber will enable.
///   max_level: LevelFilter,
///   // ...
/// }
///
/// impl MySubscriber {
///   /// Returns a new `MySubscriber` which will record spans and events up to
///   /// `max_level`.
///   pub fn with_max_level(max_level: LevelFilter) -> Self {
///     Self {
///       max_level,
///       // ...
///     }
///   }
/// }
/// impl Subscriber for MySubscriber {
///   fn enabled(&self, meta: &Metadata<'_>) -> SubscriberResult<bool> {
///     // A span or event is enabled if it is at or below the configured
///     // maximum level.
///     Ok(meta.level() <= &self.max_level)
///   }
///
///   // This optional method returns the most verbose level that this
///   // subscriber will enable. Although implementing this method is not
///   // *required*, it permits additional optimizations when it is provided,
///   // allowing spans and events above the max level to be skipped
///   // more efficiently.
///   fn max_level_hint(&self) -> Option<LevelFilter> {
///     Some(self.max_level)
///   }
///
///   // Implement the rest of the subscriber...
///   fn new_span(&self, span: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
///     // ...
///         # drop(span); Ok(Id::from_non_zero_u64(core::num::NonZeroU64::MIN))
///   }
///
///   fn event(&self, event: &Event<'_>) -> SubscriberResult {
///     // ...
///         # drop(event);
///     Ok(())
///   }
///
///   // ...
///     # fn enter(&self, _: Id) -> SubscriberResult { Ok(()) }
///     # fn exit(&self, _: Id) -> SubscriberResult { Ok(()) }
///     # fn record(&self, _: Id, _: &Record<'_>) -> SubscriberResult { Ok(()) }
///     # fn record_follows_from(&self, _: Id, _: Id) -> SubscriberResult { Ok(()) }
/// }
/// ```
///
/// It is worth noting that the `tracing-subscriber` crate provides [additional
/// APIs][envfilter] for performing more sophisticated filtering, such as
/// enabling different levels based on which module or crate a span or event is
/// recorded in.
///
/// [`DEBUG`]: Level::DEBUG
/// [`INFO`]: Level::INFO
/// [`TRACE`]: Level::TRACE
/// [`Subscriber::enabled`]: crate::subscriber::Subscriber::enabled
/// [`Subscriber::max_level_hint`]: crate::subscriber::Subscriber::max_level_hint
/// [`Subscriber`]: crate::subscriber::Subscriber
/// [envfilter]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Level(LevelInner);

/// A filter comparable to a verbosity [`Level`].
///
/// If a [`Level`] is considered less than or equal to a `LevelFilter`, it
/// should be considered enabled; if greater than the `LevelFilter`, that level
/// is disabled. See [`LevelFilter::current`] for more details.
///
/// Note that this is essentially identical to the `Level` type, but with the
/// addition of an [`OFF`] level that completely disables all trace
/// instrumentation.
///
/// See the documentation for the [`Level`] type to see how `Level`s
/// and `LevelFilter`s interact.
///
/// [`OFF`]: LevelFilter::OFF
#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct LevelFilter(Option<Level>);

/// Indicates that a string could not be parsed to a valid level.
#[derive(Copy, Clone, Debug)]
pub struct ParseLevelFilterError(());

/// Most verbose level currently enabled by registered dispatchers.
static MAX_LEVEL: AtomicUsize = AtomicUsize::new(LevelFilter::OFF_USIZE);

// ===== impl Metadata =====

impl<'a> Metadata<'a> {
  /// Construct new metadata for a span or event, with a name, target, level, field
  /// names, and optional source code location.
  #[allow(
    clippy::single_call_fn,
    reason = "public metadata constructor is used by instrumentation macros and downstream callsites"
  )]
  #[must_use]
  pub const fn new(
    name: &'static str,
    target: &'a str,
    level: Level,
    file: Option<&'a str>,
    line: Option<u32>,
    module_path: Option<&'a str>,
    fields: &field::FieldSet,
    kind: Kind,
  ) -> Self {
    Metadata {
      name,
      target,
      level,
      module_path,
      file,
      line,
      fields: *fields,
      kind,
    }
  }

  /// Returns the names of the fields on the described span or event.
  #[inline]
  #[must_use]
  pub const fn fields(&self) -> &field::FieldSet {
    &self.fields
  }

  /// Returns the level of verbosity of the described span or event.
  #[must_use]
  pub const fn level(&self) -> &Level {
    &self.level
  }

  /// Returns the name of the span.
  #[must_use]
  pub const fn name(&self) -> &'static str {
    self.name
  }

  /// Returns a string describing the part of the system where the span or
  /// event that this metadata describes occurred.
  ///
  /// Typically, this is the module path, but alternate targets may be set
  /// when spans or events are constructed.
  #[must_use]
  pub const fn target(&self) -> &'a str {
    self.target
  }

  /// Returns the path to the Rust module where the span occurred, or
  /// `None` if the module path is unknown.
  #[must_use]
  pub const fn module_path(&self) -> Option<&'a str> {
    self.module_path
  }

  /// Returns the name of the source code file where the span
  /// occurred, or `None` if the file is unknown
  #[must_use]
  pub const fn file(&self) -> Option<&'a str> {
    self.file
  }

  /// Returns the line number in the source code file where the span
  /// occurred, or `None` if the line number is unknown.
  #[must_use]
  pub const fn line(&self) -> Option<u32> {
    self.line
  }

  /// Returns an opaque `Identifier` that uniquely identifies the callsite
  /// this `Metadata` originated from.
  #[inline]
  #[must_use]
  pub const fn callsite(&self) -> callsite::Identifier {
    self.fields.callsite()
  }

  /// Returns true if the callsite kind is `Event`.
  #[must_use]
  pub const fn is_event(&self) -> bool {
    self.kind.is_event()
  }

  /// Return true if the callsite kind is `Span`.
  #[must_use]
  pub const fn is_span(&self) -> bool {
    self.kind.is_span()
  }

  /// Generate a fake field that will never match a real field.
  ///
  /// Used via valueset to fill in for unknown fields.
  #[doc(hidden)]
  #[must_use]
  pub const fn private_fake_field(&self) -> field::Field {
    self.fields.fake_field()
  }
}

impl fmt::Debug for Metadata<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut meta = f.debug_struct("Metadata");
    let _core_builder: &mut fmt::DebugStruct<'_, '_> = meta
      .field("name", &self.name)
      .field("target", &self.target)
      .field("level", &self.level);

    if let Some(path) = self.module_path() {
      let _module_path_builder: &mut fmt::DebugStruct<'_, '_> = meta.field("module_path", &path);
    }

    match (self.file(), self.line()) {
      (Some(file), Some(line)) => {
        let _location_builder: &mut fmt::DebugStruct<'_, '_> = meta.field("location", &format_args!("{file}:{line}"));
      }
      (Some(file), None) => {
        let _file_builder: &mut fmt::DebugStruct<'_, '_> = meta.field("file", &format_args!("{file}"));
      }

      // Note: a line num with no file is a kind of weird case that _probably_ never occurs...
      (None, Some(line)) => {
        let _line_builder: &mut fmt::DebugStruct<'_, '_> = meta.field("line", &line);
      }
      (None, None) => {}
    }

    meta
      .field("fields", &format_args!("{}", self.fields))
      .field("callsite", &self.callsite())
      .field("kind", &self.kind)
      .finish()
  }
}

impl Kind {
  /// Bit flag indicating that this callsite describes an event.
  const EVENT_BIT: u8 = 1 << 0;
  /// Bit flag indicating that this callsite describes a span.
  const SPAN_BIT: u8 = 1 << 1;
  /// Bit flag indicating that this callsite is an enablement hint.
  const HINT_BIT: u8 = 1 << 2;

  /// `Event` callsite
  pub const EVENT: Self = Self(Self::EVENT_BIT);

  /// `Span` callsite
  pub const SPAN: Self = Self(Self::SPAN_BIT);

  /// `enabled!` callsite. [`Subscriber`][`crate::subscriber::Subscriber`]s can assume
  /// this `Kind` means they will never receive a
  /// full event with this [`Metadata`].
  pub const HINT: Self = Self(Self::HINT_BIT);

  /// Return true if the callsite kind is `Span`
  #[must_use]
  pub const fn is_span(self) -> bool {
    self.0 & Self::SPAN_BIT == Self::SPAN_BIT
  }

  /// Return true if the callsite kind is `Event`
  #[must_use]
  pub const fn is_event(self) -> bool {
    self.0 & Self::EVENT_BIT == Self::EVENT_BIT
  }

  /// Return true if the callsite kind is `Hint`
  #[must_use]
  pub const fn is_hint(self) -> bool {
    self.0 & Self::HINT_BIT == Self::HINT_BIT
  }

  /// Sets that this `Kind` is a [hint](Self::HINT).
  ///
  /// This can be called on [`SPAN`](Self::SPAN) and [`EVENT`](Self::EVENT)
  /// kinds to construct a hint callsite that also counts as a span or event.
  #[must_use]
  pub const fn hint(self) -> Self {
    Self(self.0 | Self::HINT_BIT)
  }
}

impl fmt::Debug for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Kind(")?;
    let mut has_bits = false;
    let mut write_bit = |name: &str| {
      if has_bits {
        f.write_str(" | ")?;
      }
      f.write_str(name)?;
      has_bits = true;
      Ok(())
    };

    if self.is_event() {
      write_bit("EVENT")?;
    }

    if self.is_span() {
      write_bit("SPAN")?;
    }

    if self.is_hint() {
      write_bit("HINT")?;
    }

    // if none of the expected bits were set, something is messed up, so
    // just print the bits for debugging purposes
    if !has_bits {
      write!(f, "{:#b}", self.0)?;
    }

    f.write_str(")")
  }
}

impl Eq for Metadata<'_> {}

impl PartialEq for Metadata<'_> {
  #[inline]
  fn eq(&self, other: &Self) -> bool {
    if ptr::eq(self, other) {
      true
    } else if cfg!(not(debug_assertions)) {
      // In a well-behaving application, two `Metadata` can be assumed to
      // be totally equal so long as they share the same callsite.
      self.callsite() == other.callsite()
    } else {
      // However, when debug-assertions are enabled, do not assume that
      // the application is well-behaving; check every field of `Metadata`
      // for equality.

      // `Metadata` is destructured here to ensure a compile-error if the
      // fields of `Metadata` change.
      let &Metadata {
        name: lhs_name,
        target: lhs_target,
        level: lhs_level,
        module_path: lhs_module_path,
        file: lhs_file,
        line: lhs_line,
        fields: lhs_fields,
        kind: lhs_kind,
      } = self;

      let &Metadata {
        name: rhs_name,
        target: rhs_target,
        level: rhs_level,
        module_path: rhs_module_path,
        file: rhs_file,
        line: rhs_line,
        fields: rhs_fields,
        kind: rhs_kind,
      } = other;

      // The initial comparison of callsites is purely an optimization;
      // it can be removed without affecting the overall semantics of the
      // expression.
      self.callsite() == other.callsite()
        && lhs_name == rhs_name
        && lhs_target == rhs_target
        && lhs_level == rhs_level
        && lhs_module_path == rhs_module_path
        && lhs_file == rhs_file
        && lhs_line == rhs_line
        && lhs_fields == rhs_fields
        && lhs_kind == rhs_kind
    }
  }
}

// ===== impl Level =====

impl Level {
  /// The "error" level.
  ///
  /// Designates very serious errors.
  pub const ERROR: Self = Self(LevelInner::Error);
  /// The "warn" level.
  ///
  /// Designates hazardous situations.
  pub const WARN: Self = Self(LevelInner::Warn);
  /// The "info" level.
  ///
  /// Designates useful information.
  pub const INFO: Self = Self(LevelInner::Info);
  /// The "debug" level.
  ///
  /// Designates lower priority information.
  pub const DEBUG: Self = Self(LevelInner::Debug);
  /// The "trace" level.
  ///
  /// Designates very low priority, often extremely verbose, information.
  pub const TRACE: Self = Self(LevelInner::Trace);

  /// Returns the string representation of the `Level`.
  ///
  /// This returns the same string as the `fmt::Display` implementation.
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::TRACE => "TRACE",
      Self::DEBUG => "DEBUG",
      Self::INFO => "INFO",
      Self::WARN => "WARN",
      Self::ERROR => "ERROR",
    }
  }

  /// Returns the comparison encoding used by level ordering.
  const fn as_usize(self) -> usize {
    match self.0 {
      LevelInner::Trace => LevelFilter::TRACE_USIZE,
      LevelInner::Debug => LevelFilter::DEBUG_USIZE,
      LevelInner::Info => LevelFilter::INFO_USIZE,
      LevelInner::Warn => LevelFilter::WARN_USIZE,
      LevelInner::Error => LevelFilter::ERROR_USIZE,
    }
  }
}

impl fmt::Display for Level {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::TRACE => f.pad("TRACE"),
      Self::DEBUG => f.pad("DEBUG"),
      Self::INFO => f.pad("INFO"),
      Self::WARN => f.pad("WARN"),
      Self::ERROR => f.pad("ERROR"),
    }
  }
}

impl Error for ParseLevelError {}

impl FromStr for Level {
  type Err = ParseLevelError;
  fn from_str(input: &str) -> Result<Self, ParseLevelError> {
    input
      .parse::<usize>()
      .map_err(|_error| ParseLevelError {
        _p: ()
      })
      .and_then(|num| match num {
        1 => Ok(Self::ERROR),
        2 => Ok(Self::WARN),
        3 => Ok(Self::INFO),
        4 => Ok(Self::DEBUG),
        5 => Ok(Self::TRACE),
        _ => Err(ParseLevelError {
          _p: ()
        }),
      })
      .or_else(|_error| match input {
        level if level.eq_ignore_ascii_case("error") => Ok(Self::ERROR),
        level if level.eq_ignore_ascii_case("warn") => Ok(Self::WARN),
        level if level.eq_ignore_ascii_case("info") => Ok(Self::INFO),
        level if level.eq_ignore_ascii_case("debug") => Ok(Self::DEBUG),
        level if level.eq_ignore_ascii_case("trace") => Ok(Self::TRACE),
        _ => Err(ParseLevelError {
          _p: ()
        }),
      })
  }
}

/// Internal representation used to order verbosity levels.
#[repr(usize)]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
enum LevelInner {
  /// The "trace" level.
  ///
  /// Designates very low priority, often extremely verbose, information.
  Trace = 0,
  /// The "debug" level.
  ///
  /// Designates lower priority information.
  Debug = 1,
  /// The "info" level.
  ///
  /// Designates useful information.
  Info  = 2,
  /// The "warn" level.
  ///
  /// Designates hazardous situations.
  Warn  = 3,
  /// The "error" level.
  ///
  /// Designates very serious errors.
  Error = 4,
}

// === impl LevelFilter ===

impl From<Level> for LevelFilter {
  #[inline]
  fn from(level: Level) -> Self {
    Self::from_level(level)
  }
}

impl From<Option<Level>> for LevelFilter {
  #[inline]
  fn from(level: Option<Level>) -> Self {
    Self(level)
  }
}

impl From<LevelFilter> for Option<Level> {
  #[inline]
  fn from(filter: LevelFilter) -> Self {
    filter.into_level()
  }
}

impl LevelFilter {
  /// The "off" level.
  ///
  /// Designates that trace instrumentation should be completely disabled.
  pub const OFF: Self = Self(None);
  /// The "error" level.
  ///
  /// Designates very serious errors.
  pub const ERROR: Self = Self::from_level(Level::ERROR);
  /// The "warn" level.
  ///
  /// Designates hazardous situations.
  pub const WARN: Self = Self::from_level(Level::WARN);
  /// The "info" level.
  ///
  /// Designates useful information.
  pub const INFO: Self = Self::from_level(Level::INFO);
  /// The "debug" level.
  ///
  /// Designates lower priority information.
  pub const DEBUG: Self = Self::from_level(Level::DEBUG);
  /// The "trace" level.
  ///
  /// Designates very low priority, often extremely verbose, information.
  pub const TRACE: Self = Self(Some(Level::TRACE));

  /// Returns a `LevelFilter` that enables spans and events with verbosity up
  /// to and including `level`.
  #[must_use]
  pub const fn from_level(level: Level) -> Self {
    Self(Some(level))
  }

  /// Returns the most verbose [`Level`] that this filter accepts, or `None`
  /// if it is [`OFF`].
  ///
  /// [`OFF`]: LevelFilter::OFF
  #[must_use]
  pub const fn into_level(self) -> Option<Level> {
    self.0
  }

  // These consts are necessary because `as` casts are not allowed as
  // match patterns.
  /// Encoded `TRACE` filter value.
  const TRACE_USIZE: usize = 0;
  /// Encoded `DEBUG` filter value.
  const DEBUG_USIZE: usize = 1;
  /// Encoded `INFO` filter value.
  const INFO_USIZE: usize = 2;
  /// Encoded `WARN` filter value.
  const WARN_USIZE: usize = 3;
  /// Encoded `ERROR` filter value.
  const ERROR_USIZE: usize = 4;
  /// Encoded `OFF` filter value.
  const OFF_USIZE: usize = 5;

  /// Encodes this filter for atomic storage.
  const fn encode(self) -> usize {
    match self.0 {
      None => Self::OFF_USIZE,
      Some(Level(LevelInner::Error)) => Self::ERROR_USIZE,
      Some(Level(LevelInner::Warn)) => Self::WARN_USIZE,
      Some(Level(LevelInner::Info)) => Self::INFO_USIZE,
      Some(Level(LevelInner::Debug)) => Self::DEBUG_USIZE,
      Some(Level(LevelInner::Trace)) => Self::TRACE_USIZE,
    }
  }

  /// Returns the comparison encoding used by level-filter ordering.
  const fn as_usize(self) -> usize {
    self.encode()
  }

  /// Decodes a filter from atomic storage.
  #[allow(
    clippy::single_call_fn,
    reason = "pair atomic LevelFilter decoding with encoding and round-trip validation"
  )]
  const fn decode(value: usize) -> Self {
    match value {
      Self::ERROR_USIZE => Self::ERROR,
      Self::WARN_USIZE => Self::WARN,
      Self::INFO_USIZE => Self::INFO,
      Self::DEBUG_USIZE => Self::DEBUG,
      Self::TRACE_USIZE => Self::TRACE,
      _ => Self::OFF,
    }
  }

  /// Returns a `LevelFilter` that matches the most verbose [`Level`] that any
  /// currently active [`Subscriber`] will enable.
  ///
  /// User code should treat this as a *hint*. If a given span or event has a
  /// level *higher* than the returned `LevelFilter`, it will not be enabled.
  /// However, if the level is less than or equal to this value, the span or
  /// event is *not* guaranteed to be enabled; the subscriber will still
  /// filter each callsite individually.
  ///
  /// Therefore, comparing a given span or event's level to the returned
  /// `LevelFilter` **can** be used for determining if something is
  /// *disabled*, but **should not** be used for determining if something is
  /// *enabled*.
  ///
  /// [`Level`]: super::Level
  /// [`Subscriber`]: super::Subscriber
  #[inline]
  pub fn current() -> Self {
    Self::decode(MAX_LEVEL.load(Ordering::Relaxed))
  }

  /// Updates the process-wide maximum level hint.
  #[allow(
    clippy::single_call_fn,
    reason = "centralize max-level atomic encoding owned by LevelFilter"
  )]
  pub(crate) fn set_max(filter: Self) {
    let val = filter.encode();

    // using an AcqRel swap ensures an ordered relationship of writes to the
    // max level.
    let _previous_max_level = MAX_LEVEL.swap(val, Ordering::AcqRel);
  }
}

impl fmt::Display for LevelFilter {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::OFF => f.pad("off"),
      Self::ERROR => f.pad("error"),
      Self::WARN => f.pad("warn"),
      Self::INFO => f.pad("info"),
      Self::DEBUG => f.pad("debug"),
      Self::TRACE => f.pad("trace"),
    }
  }
}

impl fmt::Debug for LevelFilter {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::OFF => f.pad("LevelFilter::OFF"),
      Self::ERROR => f.pad("LevelFilter::ERROR"),
      Self::WARN => f.pad("LevelFilter::WARN"),
      Self::INFO => f.pad("LevelFilter::INFO"),
      Self::DEBUG => f.pad("LevelFilter::DEBUG"),
      Self::TRACE => f.pad("LevelFilter::TRACE"),
    }
  }
}

impl FromStr for LevelFilter {
  type Err = ParseLevelFilterError;
  fn from_str(from: &str) -> Result<Self, Self::Err> {
    from
      .parse::<usize>()
      .ok()
      .and_then(|num| match num {
        0 => Some(Self::OFF),
        1 => Some(Self::ERROR),
        2 => Some(Self::WARN),
        3 => Some(Self::INFO),
        4 => Some(Self::DEBUG),
        5 => Some(Self::TRACE),
        _ => None,
      })
      .or_else(|| match from {
        "" => Some(Self::ERROR),
        level if level.eq_ignore_ascii_case("error") => Some(Self::ERROR),
        level if level.eq_ignore_ascii_case("warn") => Some(Self::WARN),
        level if level.eq_ignore_ascii_case("info") => Some(Self::INFO),
        level if level.eq_ignore_ascii_case("debug") => Some(Self::DEBUG),
        level if level.eq_ignore_ascii_case("trace") => Some(Self::TRACE),
        level if level.eq_ignore_ascii_case("off") => Some(Self::OFF),
        _ => None,
      })
      .ok_or(ParseLevelFilterError(()))
  }
}

/// Returned if parsing a `Level` fails.
#[derive(Copy, Clone, Debug)]
pub struct ParseLevelError {
  /// Prevents downstream crates from constructing this error directly.
  _p: (),
}

impl fmt::Display for ParseLevelError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad("error parsing level: expected one of \"error\", \"warn\", \"info\", \"debug\", \"trace\", or a number 1-5")
  }
}

impl fmt::Display for ParseLevelFilterError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.pad("error parsing level filter: expected one of \"off\", \"error\", \"warn\", \"info\", \"debug\", \"trace\", or a number 0-5")
  }
}

impl Error for ParseLevelFilterError {}

// ==== Level and LevelFilter comparisons ====

// /!\ BIG, IMPORTANT WARNING /!\
// Do NOT mess with these implementations! They are hand-written for a reason!
//
// Since comparing `Level`s and `LevelFilter`s happens in a *very* hot path
// (potentially, every time a span or event macro is hit, regardless of whether
// or not is enabled), we *need* to ensure that these comparisons are as fast as
// possible. Therefore, we have some requirements:
//
// 1. We want to do our best to ensure that rustc will generate integer-integer comparisons wherever
//    possible.
//
//    The derived `Ord`/`PartialOrd` impls for `LevelFilter` will not do this,
//    because `LevelFilter`s are represented by `Option<Level>`, rather than as
//    a separate `#[repr(usize)]` enum. This was (unfortunately) necessary for
//    backwards-compatibility reasons, as the  `tracing` crate's original
//    version of `LevelFilter` defined `const fn` conversions between `Level`s
//    and `LevelFilter`, so we're stuck with the `Option<Level>` repr.
//    Therefore, we need hand-written `PartialOrd` impls that cast both sides of
//    the comparison to `usize`s, to force the compiler to generate integer
//    compares.
//
// 2. The hottest `Level`/`LevelFilter` comparison, the one that happens every time a callsite is
//    hit, occurs *within the `tracing` crate's macros*. This means that the comparison is happening
//    *inside* a crate that *depends* on `tracing-core`, not in `tracing-core` itself. The compiler
//    will only inline function calls across crate boundaries if the called function is annotated
//    with an `#[inline]` attribute, and we *definitely* want the comparison functions to be
//    inlined: as previously mentioned, they should compile down to a single integer comparison on
//    release builds, and it seems really sad to push an entire stack frame to call a function
//    consisting of one `cmp` instruction!
//
//    Therefore, we need to ensure that all the comparison methods have
//    `#[inline]` or `#[inline]` attributes. It's not sufficient to just
//    add the attribute to `partial_cmp` in a manual implementation of the
//    trait, since it's the comparison operators (`lt`, `le`, `gt`, and `ge`)
//    that will actually be *used*, and the default implementation of *those*
//    methods, which calls `partial_cmp`, does not have an inline annotation.
//
// 3. We need the comparisons to be inverted. The discriminants for the `LevelInner` enum are
//    assigned in "backwards" order, with `TRACE` having the *lowest* value. However, we want
//    `TRACE` to compare greater-than all other levels.
//
//    Why are the numeric values inverted? In order to ensure that `LevelFilter`
//    (which, as previously mentioned, *has* to be internally represented by an
//    `Option<Level>`) compiles down to a single integer value. This is
//    necessary for storing the global max in an `AtomicUsize`, and for ensuring
//    that we use fast integer-integer comparisons, as mentioned previously. In
//    order to ensure this, we exploit the niche optimization. The niche
//    optimization for `Option<{enum with a numeric repr}>` will choose
//    `(HIGHEST_DISCRIMINANT_VALUE + 1)` as the representation for `None`.
//    Therefore, the integer representation of `LevelFilter::OFF` (which is
//    `None`) will be the number 5. `OFF` must compare higher than every other
//    level in order for it to filter as expected. Since we want to use a single
//    `cmp` instruction, we can't special-case the integer value of `OFF` to
//    compare higher, as that will generate more code. Instead, we need it to be
//    on one end of the enum, with `ERROR` on the opposite end, so we assign the
//    value 0 to `ERROR`.
//
//    This *does* mean that when parsing `LevelFilter`s or `Level`s from
//    `String`s, the integer values are inverted, but that doesn't happen in a
//    hot path.
//
//    Note that we manually invert the comparisons by swapping the left-hand and
//    right-hand side. Using `Ordering::reverse` generates significantly worse
//    code (per Matt Godbolt's Compiler Explorer).
//
// Anyway, that's a brief history of why this code is the way it is. Don't
// change it unless you know what you're doing.

impl PartialEq<LevelFilter> for Level {
  #[inline]
  fn eq(&self, other: &LevelFilter) -> bool {
    self.as_usize() == other.as_usize()
  }
}

impl PartialOrd for Level {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
    Some(self.cmp(other))
  }

  #[inline]
  fn lt(&self, other: &Self) -> bool {
    other.as_usize() < self.as_usize()
  }

  #[inline]
  fn le(&self, other: &Self) -> bool {
    other.as_usize() <= self.as_usize()
  }

  #[inline]
  fn gt(&self, other: &Self) -> bool {
    other.as_usize() > self.as_usize()
  }

  #[inline]
  fn ge(&self, other: &Self) -> bool {
    other.as_usize() >= self.as_usize()
  }
}

impl Ord for Level {
  #[inline]
  fn cmp(&self, other: &Self) -> cmp::Ordering {
    other.as_usize().cmp(&self.as_usize())
  }
}

impl PartialOrd<LevelFilter> for Level {
  #[inline]
  fn partial_cmp(&self, other: &LevelFilter) -> Option<cmp::Ordering> {
    Some(other.as_usize().cmp(&self.as_usize()))
  }

  #[inline]
  fn lt(&self, other: &LevelFilter) -> bool {
    other.as_usize() < self.as_usize()
  }

  #[inline]
  fn le(&self, other: &LevelFilter) -> bool {
    other.as_usize() <= self.as_usize()
  }

  #[inline]
  fn gt(&self, other: &LevelFilter) -> bool {
    other.as_usize() > self.as_usize()
  }

  #[inline]
  fn ge(&self, other: &LevelFilter) -> bool {
    other.as_usize() >= self.as_usize()
  }
}

impl PartialEq<Level> for LevelFilter {
  #[inline]
  fn eq(&self, other: &Level) -> bool {
    self.as_usize() == other.as_usize()
  }
}

impl PartialOrd for LevelFilter {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
    Some(self.cmp(other))
  }

  #[inline]
  fn lt(&self, other: &Self) -> bool {
    other.as_usize() < self.as_usize()
  }

  #[inline]
  fn le(&self, other: &Self) -> bool {
    other.as_usize() <= self.as_usize()
  }

  #[inline]
  fn gt(&self, other: &Self) -> bool {
    other.as_usize() > self.as_usize()
  }

  #[inline]
  fn ge(&self, other: &Self) -> bool {
    other.as_usize() >= self.as_usize()
  }
}

impl Ord for LevelFilter {
  #[inline]
  fn cmp(&self, other: &Self) -> cmp::Ordering {
    other.as_usize().cmp(&self.as_usize())
  }
}

impl PartialOrd<Level> for LevelFilter {
  #[inline]
  fn partial_cmp(&self, other: &Level) -> Option<cmp::Ordering> {
    Some(other.as_usize().cmp(&self.as_usize()))
  }

  #[inline]
  fn lt(&self, other: &Level) -> bool {
    other.as_usize() < self.as_usize()
  }

  #[inline]
  fn le(&self, other: &Level) -> bool {
    other.as_usize() <= self.as_usize()
  }

  #[inline]
  fn gt(&self, other: &Level) -> bool {
    other.as_usize() > self.as_usize()
  }

  #[inline]
  fn ge(&self, other: &Level) -> bool {
    other.as_usize() >= self.as_usize()
  }
}

#[cfg(test)]
mod tests {
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;

  use super::*;

  #[test]
  fn level_from_str() -> Result<(), TestFailure> {
    let error_level = ensure_ok("error".parse::<Level>(), "error level parses")?;
    let debug_level = ensure_ok("4".parse::<Level>(), "debug numeric level parses")?;

    ensure_eq(&error_level, &Level::ERROR, "error string maps to ERROR")?;
    ensure_eq(&debug_level, &Level::DEBUG, "4 maps to DEBUG")?;
    ensure("0".parse::<Level>().is_err(), "0 is not a valid Level")
  }

  #[test]
  fn filter_level_conversion() -> Result<(), TestFailure> {
    let mapping = [
      (LevelFilter::OFF, None),
      (LevelFilter::ERROR, Some(Level::ERROR)),
      (LevelFilter::WARN, Some(Level::WARN)),
      (LevelFilter::INFO, Some(Level::INFO)),
      (LevelFilter::DEBUG, Some(Level::DEBUG)),
      (LevelFilter::TRACE, Some(Level::TRACE)),
    ];
    for &(filter, level) in &mapping {
      ensure(filter.into_level() == level, "filter converts into expected level")?;
      if let Some(mapped_level) = level {
        let actual: LevelFilter = mapped_level.into();
        ensure_eq(&actual, &filter, "level converts back into filter")?;
      } else {
        let actual: LevelFilter = None.into();
        ensure_eq(&actual, &filter, "None converts into OFF filter")?;
      }
    }
    Ok(())
  }

  #[test]
  fn level_filter_encoding_round_trips() -> Result<(), TestFailure> {
    let mapping = [
      (LevelFilter::OFF, LevelFilter::OFF_USIZE),
      (LevelFilter::ERROR, LevelFilter::ERROR_USIZE),
      (LevelFilter::WARN, LevelFilter::WARN_USIZE),
      (LevelFilter::INFO, LevelFilter::INFO_USIZE),
      (LevelFilter::DEBUG, LevelFilter::DEBUG_USIZE),
      (LevelFilter::TRACE, LevelFilter::TRACE_USIZE),
    ];
    for &(filter, expected) in &mapping {
      ensure_eq(&expected, &filter.encode(), "level filter encodes")?;
      ensure_eq(&filter, &LevelFilter::decode(expected), "level filter decodes")?;
    }
    Ok(())
  }
}
