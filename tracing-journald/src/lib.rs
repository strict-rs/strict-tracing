//! # tracing-journald
//!
//! Support for logging [`tracing`] events natively to [journald],
//! preserving structured information.
//!
//! ## Overview
//!
//! [`tracing`] is a framework for instrumenting Rust programs to collect
//! scoped, structured, and async-aware diagnostics. `tracing-journald` provides a
//! [`tracing_subscriber::Layer`] implementation for logging `tracing` spans
//! and events to [`systemd-journald`][journald], on Linux distributions that
//! use `systemd`.
//!
//! *Compiler support: [requires `rustc` 1.96+][msrv]*
//!
//! [msrv]: #supported-rust-versions
//! [`tracing`]: https://crates.io/crates/tracing
//! [journald]: https://www.freedesktop.org/software/systemd/man/systemd-journald.service.html
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
#[cfg(unix)]
use std::env::args_os;
#[cfg(unix)]
use std::env::var_os;
use std::fmt;
use std::io;
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::net::UnixDatagram;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use rustix::process::geteuid;
use tracing_core::Field;
use tracing_core::Level;
use tracing_core::Metadata;
use tracing_core::Subscriber;
use tracing_core::event::Event;
use tracing_core::field::Visit;
use tracing_core::span::Attributes;
use tracing_core::span::Id;
use tracing_core::span::Record;
use tracing_core::subscriber::SubscriberResult;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

#[cfg(target_os = "linux")]
pub mod memfd;
#[cfg(target_os = "linux")]
pub mod socket;

/// Sends events and their fields to journald
///
/// [journald conventions] for structured field names differ from typical tracing idioms, and
/// journald discards fields which violate its conventions. Hence, this layer automatically
/// sanitizes field names by translating `.`s into `_`s, stripping leading `_`s and
/// non-ascii-alphanumeric characters other than `_`, and upcasing.
///
/// By default, levels are mapped losslessly to journald `PRIORITY` values as follows:
///
/// - `ERROR` => Error (3)
/// - `WARN` => Warning (4)
/// - `INFO` => Notice (5)
/// - `DEBUG` => Informational (6)
/// - `TRACE` => Debug (7)
///
/// These mappings can be changed with [`Layer::with_priority_mappings`].
///
/// The standard journald `CODE_LINE` and `CODE_FILE` fields are automatically emitted. A `TARGET`
/// field is emitted containing the event's target.
///
/// For events recorded inside spans, an additional `SPAN_NAME` field is emitted with the name of
/// each of the event's parent spans.
///
/// User-defined fields other than the event `message` field have a prefix applied by default to
/// prevent collision with standard fields.
///
/// [journald conventions]: https://www.freedesktop.org/software/systemd/man/systemd.journal-fields.html
pub struct Layer {
  /// Datagram socket used to send native protocol payloads to journald.
  #[cfg(unix)]
  socket:            UnixDatagram,
  /// Filesystem path for the selected journald socket.
  #[cfg(unix)]
  socket_path:       PathBuf,
  /// Prefix applied to user fields, except `message`.
  field_prefix:      Option<String>,
  /// Value emitted as `SYSLOG_IDENTIFIER`.
  syslog_identifier: String,
  /// Pre-encoded fields emitted with every payload.
  additional_fields: Vec<u8>,
  /// Mapping from tracing levels to journald priorities.
  priority_mappings: PriorityMappings,
}

impl fmt::Debug for Layer {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut debug = f.debug_struct("Layer");

    #[cfg(unix)]
    let _debug = debug.field("socket", &self.socket).field("socket_path", &self.socket_path);

    debug
      .field("field_prefix", &self.field_prefix)
      .field("syslog_identifier", &self.syslog_identifier)
      .field("additional_fields", &self.additional_fields)
      .field("priority_mappings", &self.priority_mappings)
      .finish()
  }
}

/// System journald socket path.
#[cfg(unix)]
const SYSTEM_JOURNALD_PATH: &str = "/run/systemd/journal/socket";

/// The journald socket namespace used by a [`Layer`].
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum JournalNamespace {
  /// Send events to the system journal.
  System,

  /// Send events to the current user's journal.
  User,
}

#[cfg(unix)]
impl JournalNamespace {
  /// Return the socket path for this namespace.
  fn socket_path(self) -> PathBuf {
    match self {
      Self::System => PathBuf::from(SYSTEM_JOURNALD_PATH),
      Self::User => user_journald_path(),
    }
  }
}

impl Layer {
  /// Construct a journald layer
  ///
  /// Fails if the journald socket couldn't be opened. Returns a `NotFound` error unconditionally
  /// in non-Unix environments.
  ///
  /// # Errors
  ///
  /// Returns an error when the platform has no journald socket, or when the socket probe cannot
  /// be sent to journald.
  #[allow(
    clippy::single_call_fn,
    reason = "public constructor preserves the system-journal API entry point"
  )]
  pub fn new() -> io::Result<Self> {
    #[cfg(unix)]
    {
      Self::new_in_namespace(JournalNamespace::System)
    }
    #[cfg(not(unix))]
    Err(io::Error::new(
      io::ErrorKind::NotFound,
      "journald does not exist in this environment",
    ))
  }

  /// Construct a journald layer targeting the current user's journal.
  ///
  /// Fails if the user journald socket couldn't be opened.
  ///
  /// # Errors
  ///
  /// Returns an error when the current user's journald socket cannot be opened or probed.
  #[allow(
    clippy::single_call_fn,
    reason = "public constructor preserves the user-journal API entry point"
  )]
  pub fn new_user() -> io::Result<Self> {
    Self::new_in_namespace(JournalNamespace::User)
  }

  /// Construct a journald layer targeting a specific journald namespace.
  ///
  /// Fails if the selected journald socket couldn't be opened.
  ///
  /// # Errors
  ///
  /// Returns an error when the selected journald namespace has no reachable socket.
  pub fn new_in_namespace(namespace: JournalNamespace) -> io::Result<Self> {
    #[cfg(unix)]
    {
      Self::new_with_socket_path(namespace.socket_path())
    }
    #[cfg(not(unix))]
    Err(io::Error::new(
      io::ErrorKind::NotFound,
      "journald does not exist in this environment",
    ))
  }

  /// Construct a layer using an explicit journald socket path.
  #[cfg(unix)]
  #[allow(
    clippy::single_call_fn,
    reason = "socket-path construction keeps Unix socket probing separate from namespace selection"
  )]
  fn new_with_socket_path(socket_path: PathBuf) -> io::Result<Self> {
    let socket = UnixDatagram::unbound()?;
    let layer = Self {
      socket,
      socket_path,
      field_prefix: Some("F".into()),
      syslog_identifier: args_os()
                .next()
                .as_ref()
                .and_then(|path| Path::new(path).file_name())
                .map(|name| name.to_string_lossy().into_owned())
                // If we fail to get the name of the current executable fall back to an empty string.
                .unwrap_or_default(),
      additional_fields: Vec::new(),
      priority_mappings: PriorityMappings::new(),
    };
    // Check that we can talk to journald, by sending empty payload which journald discards.
    // However if the socket didn't exist or if none listened we'd get an error here.
    let _bytes_sent = layer.send_payload(&[])?;
    Ok(layer)
  }

  /// Sets the prefix to apply to names of user-defined fields other than the event `message`
  /// field. Defaults to `Some("F")`.
  #[must_use]
  pub fn with_field_prefix(mut self, prefix: Option<String>) -> Self {
    self.field_prefix = prefix;
    self
  }

  /// Sets how [`tracing_core::Level`]s are mapped to [journald priorities](Priority).
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tracing::error;
  /// use tracing_journald::Priority;
  /// use tracing_journald::PriorityMappings;
  /// use tracing_subscriber::prelude::*;
  ///
  /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
  /// let registry = tracing_subscriber::registry();
  /// match tracing_journald::layer() {
  ///   Ok(layer) => {
  ///     registry.with(layer
  ///                 // We can tweak the mappings between the trace level and
  ///                 // the journal priorities.
  ///                 .with_priority_mappings(PriorityMappings {
  ///                     info: Priority::Informational,
  ///                     ..PriorityMappings::new()
  ///                 }));
  ///   }
  ///   // journald is typically available on Linux systems, but nowhere else. Portable software
  ///   // should handle its absence gracefully.
  ///   Err(e) => {
  ///     registry.try_init()?;
  ///     error!("couldn't connect to journald: {}", e);
  ///   }
  /// }
  /// # Ok(()) }
  /// ```
  #[must_use]
  pub const fn with_priority_mappings(mut self, mappings: PriorityMappings) -> Self {
    self.priority_mappings = mappings;
    self
  }

  /// Sets the syslog identifier for this logger.
  ///
  /// The syslog identifier comes from the classic syslog interface (`openlog()`
  /// and `syslog()`) and tags log entries with a given identifier.
  /// Systemd exposes it in the `SYSLOG_IDENTIFIER` journal field, and allows
  /// filtering log messages by syslog identifier with `journalctl -t`.
  /// Unlike the unit (`journalctl -u`) this field is not trusted, i.e. applications
  /// can set it freely, and use it e.g. to further categorize log entries emitted under
  /// the same systemd unit or in the same process.  It also allows to filter for log
  /// entries of processes not started in their own unit.
  ///
  /// See [Journal Fields](https://www.freedesktop.org/software/systemd/man/systemd.journal-fields.html)
  /// and [journalctl](https://www.freedesktop.org/software/systemd/man/journalctl.html)
  /// for more information.
  ///
  /// Defaults to the file name of the executable of the current process, if any.
  #[must_use]
  pub fn with_syslog_identifier(mut self, identifier: String) -> Self {
    self.syslog_identifier = identifier;
    self
  }

  /// Adds fields that will get be passed to journald with every log entry.
  ///
  /// The input values of this function are interpreted as `(field, value)` pairs.
  ///
  /// This can for example be used to configure the syslog facility.
  /// See [Journal Fields](https://www.freedesktop.org/software/systemd/man/systemd.journal-fields.html)
  /// and [journalctl](https://www.freedesktop.org/software/systemd/man/journalctl.html)
  /// for more information.
  ///
  /// Fields specified using this method will be added to the journald
  /// message alongside fields generated from the event's fields, its
  /// metadata, and the span context. If the name of a field provided using
  /// this method is the same as the name of a field generated by the
  /// layer, both fields will be sent to journald.
  ///
  /// ```no_run
  /// # use tracing_journald::Layer;
  /// # fn main() -> std::io::Result<()> {
  /// let layer = Layer::new()?.with_custom_fields([("SYSLOG_FACILITY", "17")]);
  /// # let _layer = layer;
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn with_custom_fields<T: AsRef<str>, U: AsRef<[u8]>>(mut self, fields: impl IntoIterator<Item = (T, U)>) -> Self {
    for (name, field_value) in fields {
      put_field_length_encoded(&mut self.additional_fields, name.as_ref(), |value_buf| {
        value_buf.extend_from_slice(field_value.as_ref());
      });
    }
    self
  }

  /// Returns the syslog identifier in use.
  #[must_use]
  pub fn syslog_identifier(&self) -> &str {
    &self.syslog_identifier
  }

  #[cfg(not(unix))]
  fn send_payload(&self, _opayload: &[u8]) -> io::Result<()> {
    Err(io::Error::new(io::ErrorKind::Other, "journald not supported on non-Unix"))
  }

  #[cfg(unix)]
  /// Send a native protocol payload to journald.
  fn send_payload(&self, payload: &[u8]) -> io::Result<usize> {
    use rustix::io::Errno;

    self.socket.send_to(payload, &self.socket_path).or_else(|error| {
      if Some(Errno::MSGSIZE.raw_os_error()) == error.raw_os_error() {
        self.send_large_payload(payload)
      } else {
        Err(error)
      }
    })
  }

  #[cfg(all(unix, not(target_os = "linux")))]
  fn send_large_payload(&self, _payload: &[u8]) -> io::Result<usize> {
    Err(io::Error::new(io::ErrorKind::Other, "Large payloads not supported on non-Linux OS"))
  }

  /// Send large payloads to journald via a memfd.
  #[cfg(target_os = "linux")]
  fn send_large_payload(&self, payload: &[u8]) -> io::Result<usize> {
    // If the payload's too large for a single datagram, send it through a memfd, see
    // https://systemd.io/JOURNAL_NATIVE_PROTOCOL/
    use rustix::fd::AsFd as _;

    // Write the whole payload to a memfd
    let mut mem = memfd::create_sealable()?;
    mem.write_all(payload)?;
    // Fully seal the memfd to signal journald that its backing data won't resize anymore
    // and so is safe to mmap.
    memfd::seal_fully(&mem)?;
    socket::send_one_fd_to(&self.socket, mem.as_fd(), &self.socket_path)
  }

  /// Put the journald priority field for `meta` into `buf`.
  fn put_priority(&self, buf: &mut Vec<u8>, meta: &Metadata<'_>) {
    put_field_wellformed(buf, "PRIORITY", &[match *meta.level() {
      Level::ERROR => self.priority_mappings.error.as_byte(),
      Level::WARN => self.priority_mappings.warn.as_byte(),
      Level::INFO => self.priority_mappings.info.as_byte(),
      Level::DEBUG => self.priority_mappings.debug.as_byte(),
      Level::TRACE => self.priority_mappings.trace.as_byte(),
    }]);
  }
}

/// Construct a journald layer
///
/// Fails if the journald socket couldn't be opened.
///
/// # Errors
///
/// Returns an error when the system journald socket cannot be opened or probed.
pub fn layer() -> io::Result<Layer> {
  Layer::new()
}

/// Construct a journald layer targeting the current user's journal.
///
/// Fails if the user journald socket couldn't be opened.
///
/// # Errors
///
/// Returns an error when the current user's journald socket cannot be opened or probed.
pub fn user_layer() -> io::Result<Layer> {
  Layer::new_user()
}

#[cfg(unix)]
/// Return the current user's journald socket path.
#[allow(
  clippy::single_call_fn,
  reason = "user namespace resolution keeps XDG and uid fallback policy in one named helper"
)]
fn user_journald_path() -> PathBuf {
  if let Some(runtime_dir) = var_os("XDG_RUNTIME_DIR") {
    return PathBuf::from(runtime_dir).join("systemd/journal/socket");
  }

  let uid = geteuid();
  Path::new("/run/user").join(uid.to_string()).join("systemd/journal/socket")
}

impl<S> tracing_subscriber::Layer<S> for Layer
where
  S: Subscriber + for<'span> LookupSpan<'span>,
{
  fn on_new_span(&self, attrs: &Attributes<'_>, id: Id, ctx: Context<'_, S>) -> SubscriberResult {
    let Some(span) = ctx.span(id) else {
      return Ok(());
    };
    let mut buf = Vec::with_capacity(256);

    put_field_wellformed(&mut buf, "SPAN_NAME", span.name().as_bytes());
    put_metadata(&mut buf, span.metadata(), Some("SPAN_"));

    attrs.record(&mut SpanVisitor {
      buf:          &mut buf,
      field_prefix: self.field_prefix.as_deref(),
    });

    {
      let mut extensions = span.extensions_mut();
      if extensions.get_mut::<SpanFields>().is_some() {
        return Ok(());
      }
      let _previous_fields = extensions.insert(SpanFields(buf));
    }
    Ok(())
  }

  fn on_record(&self, id: Id, values: &Record<'_>, ctx: Context<'_, S>) -> SubscriberResult {
    let Some(span) = ctx.span(id) else {
      return Ok(());
    };
    let mut recorded_fields = Vec::new();
    values.record(&mut SpanVisitor {
      buf:          &mut recorded_fields,
      field_prefix: self.field_prefix.as_deref(),
    });
    {
      let mut exts = span.extensions_mut();
      let Some(fields) = exts.get_mut::<SpanFields>() else {
        return Ok(());
      };
      fields.0.extend_from_slice(&recorded_fields);
      drop(exts);
    };
    Ok(())
  }

  fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) -> SubscriberResult {
    let mut buf = Vec::with_capacity(256);

    // Record span fields
    for span in ctx.lookup_current().into_iter().flat_map(|span| span.scope().root_to_leaf()) {
      let exts = span.extensions();
      if let Some(fields) = exts.get::<SpanFields>() {
        buf.extend_from_slice(&fields.0);
      }
    }

    // Record event fields
    self.put_priority(&mut buf, event.metadata());
    put_metadata(&mut buf, event.metadata(), None);
    put_field_length_encoded(&mut buf, "SYSLOG_IDENTIFIER", |value_buf| {
      value_buf.extend_from_slice(self.syslog_identifier.as_bytes());
    });
    buf.extend_from_slice(&self.additional_fields);

    let mut visitor = EventVisitor {
      buf:    &mut buf,
      prefix: self.field_prefix.as_deref(),
    };
    event.record(&mut visitor);

    // At this point we can't handle the error anymore so just ignore it.
    let _send_result = self.send_payload(&buf);
    Ok(())
  }
}

/// Pre-encoded journald fields attached to a span.
struct SpanFields(
  /// Native protocol bytes that should be replayed for events inside the span.
  Vec<u8>,
);

/// Visitor that serializes span fields into journald native protocol bytes.
struct SpanVisitor<'a> {
  /// Output buffer receiving serialized fields.
  buf:          &'a mut Vec<u8>,
  /// Prefix applied to user fields.
  field_prefix: Option<&'a str>,
}

impl SpanVisitor<'_> {
  /// Append the configured span field prefix when present.
  fn put_span_prefix(&mut self) {
    if let Some(prefix) = self.field_prefix {
      self.buf.extend_from_slice(prefix.as_bytes());
      self.buf.push(b'_');
    }
  }
}

impl Visit for SpanVisitor<'_> {
  fn record_str(&mut self, field: &Field, field_value: &str) {
    self.put_span_prefix();
    put_field_length_encoded(self.buf, field.name(), |value_buf| {
      value_buf.extend_from_slice(field_value.as_bytes());
    });
  }

  fn record_debug(&mut self, field: &Field, field_value: &dyn fmt::Debug) {
    self.put_span_prefix();
    put_field_length_encoded(self.buf, field.name(), |value_buf| {
      push_display(value_buf, DebugValue {
        field_value,
      });
    });
  }
}

/// Helper for generating the journal export format, which is consumed by journald:
/// <https://www.freedesktop.org/wiki/Software/systemd/export/>.
struct EventVisitor<'a> {
  /// Output buffer receiving serialized fields.
  buf:    &'a mut Vec<u8>,
  /// Prefix applied to user fields other than `message`.
  prefix: Option<&'a str>,
}

impl EventVisitor<'_> {
  /// Append the configured event field prefix when `field` is not `message`.
  fn put_prefix(&mut self, field: &Field) {
    if let Some(prefix) = self.prefix
      && field.name() != "message"
    {
      // message maps to the standard MESSAGE field so don't prefix it
      self.buf.extend_from_slice(prefix.as_bytes());
      self.buf.push(b'_');
    }
  }
}

impl Visit for EventVisitor<'_> {
  fn record_str(&mut self, field: &Field, field_value: &str) {
    self.put_prefix(field);
    put_field_length_encoded(self.buf, field.name(), |value_buf| {
      value_buf.extend_from_slice(field_value.as_bytes());
    });
  }

  fn record_debug(&mut self, field: &Field, field_value: &dyn fmt::Debug) {
    self.put_prefix(field);
    put_field_length_encoded(self.buf, field.name(), |value_buf| {
      push_display(value_buf, DebugValue {
        field_value,
      });
    });
  }
}

/// Display adapter that intentionally renders tracing's debug-only field values.
struct DebugValue<'a> {
  /// Debug value supplied by the `tracing_core::field::Visit` API.
  field_value: &'a dyn fmt::Debug,
}

impl fmt::Display for DebugValue<'_> {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Debug::fmt(self.field_value, formatter)
  }
}

/// Append a display value to `buf`.
fn push_display(buf: &mut Vec<u8>, field_value: impl fmt::Display) {
  let rendered = field_value.to_string();
  buf.extend_from_slice(rendered.as_bytes());
}

/// A priority (called "severity code" by syslog) is used to mark the
/// importance of a message.
///
/// Descriptions and examples are taken from the [Arch Linux wiki].
/// Priorities are also documented in the
/// [section 6.2.1 of the Syslog protocol RFC][syslog].
///
/// [Arch Linux wiki]: https://wiki.archlinux.org/title/Systemd/Journal#Priority_level
/// [syslog]: https://www.rfc-editor.org/rfc/rfc5424#section-6.2.1
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[repr(u8)]
pub enum Priority {
  /// System is unusable.
  ///
  /// Examples:
  ///
  /// - severe Kernel BUG
  /// - systemd dumped core
  ///
  /// This level should not be used by applications.
  Emergency     = b'0',
  /// Should be corrected immediately.
  ///
  /// Examples:
  ///
  /// - Vital subsystem goes out of work, data loss:
  /// - `kernel: BUG: unable to handle kernel paging request at ffffc90403238ffc`
  Alert         = b'1',
  /// Critical conditions
  ///
  /// Examples:
  ///
  /// - Crashe, coredumps
  /// - `systemd-coredump[25319]: Process 25310 (plugin-container) of user 1000 dumped core`
  Critical      = b'2',
  /// Error conditions
  ///
  /// Examples:
  ///
  /// - Not severe error reported
  /// - `kernel: usb 1-3: 3:1: cannot get freq at ep 0x84, systemd[1]: Failed unmounting /var`
  /// - `libvirtd[1720]: internal error: Failed to initialize a valid firewall backend`
  Error         = b'3',
  /// May indicate that an error will occur if action is not taken.
  ///
  /// Examples:
  ///
  /// - a non-root file system has only 1GB free
  /// - `org.freedesktop. Notifications[1860]: (process:5999): Gtk-WARNING **: Locale not supported
  ///   by C library. Using the fallback 'C' locale`
  Warning       = b'4',
  /// Events that are unusual, but not error conditions.
  ///
  /// Examples:
  ///
  /// - `systemd[1]: var.mount: Directory /var to mount over is not empty, mounting anyway`
  /// - `gcr-prompter[4997]: Gtk: GtkDialog mapped without a transient parent. This is discouraged`
  Notice        = b'5',
  /// Normal operational messages that require no action.
  ///
  /// Example: `lvm[585]: 7 logical volume(s) in volume group "archvg" now active`
  Informational = b'6',
  /// Information useful to developers for debugging the
  /// application.
  ///
  /// Example: `kdeinit5[1900]: powerdevil: Scheduling inhibition from ":1.14" "firefox" with cookie
  /// 13 and reason "screen"`
  Debug         = b'7',
}

impl Priority {
  /// Return the ASCII byte journald expects for this priority.
  #[must_use]
  const fn as_byte(self) -> u8 {
    match self {
      Self::Emergency => b'0',
      Self::Alert => b'1',
      Self::Critical => b'2',
      Self::Error => b'3',
      Self::Warning => b'4',
      Self::Notice => b'5',
      Self::Informational => b'6',
      Self::Debug => b'7',
    }
  }
}

/// Mappings from tracing [`Level`]s to journald [priorities].
///
/// [priorities]: Priority
#[derive(Copy, Clone, Debug)]
pub struct PriorityMappings {
  /// Priority mapped to the `ERROR` level
  pub error: Priority,
  /// Priority mapped to the `WARN` level
  pub warn:  Priority,
  /// Priority mapped to the `INFO` level
  pub info:  Priority,
  /// Priority mapped to the `DEBUG` level
  pub debug: Priority,
  /// Priority mapped to the `TRACE` level
  pub trace: Priority,
}

impl PriorityMappings {
  /// Returns the default priority mappings:
  ///
  /// - [`tracing::Level::ERROR`][]: [`Priority::Error`] (3)
  /// - [`tracing::Level::WARN`][]: [`Priority::Warning`] (4)
  /// - [`tracing::Level::INFO`][]: [`Priority::Notice`] (5)
  /// - [`tracing::Level::DEBUG`][]: [`Priority::Informational`] (6)
  /// - [`tracing::Level::TRACE`][]: [`Priority::Debug`] (7)
  ///
  /// [`tracing::Level::ERROR`]: tracing_core::Level::ERROR
  /// [`tracing::Level::WARN`]: tracing_core::Level::WARN
  /// [`tracing::Level::INFO`]: tracing_core::Level::INFO
  /// [`tracing::Level::DEBUG`]: tracing_core::Level::DEBUG
  /// [`tracing::Level::TRACE`]: tracing_core::Level::TRACE
  #[must_use]
  pub const fn new() -> Self {
    Self {
      error: Priority::Error,
      warn:  Priority::Warning,
      info:  Priority::Notice,
      debug: Priority::Informational,
      trace: Priority::Debug,
    }
  }
}

impl Default for PriorityMappings {
  fn default() -> Self {
    Self::new()
  }
}

/// Append metadata fields for an event or span.
fn put_metadata(buf: &mut Vec<u8>, meta: &Metadata<'_>, field_prefix: Option<&str>) {
  if let Some(prefix) = field_prefix {
    buf.extend_from_slice(prefix.as_bytes());
  }
  put_field_wellformed(buf, "TARGET", meta.target().as_bytes());
  if let Some(file) = meta.file() {
    if let Some(prefix) = field_prefix {
      buf.extend_from_slice(prefix.as_bytes());
    }
    put_field_wellformed(buf, "CODE_FILE", file.as_bytes());
  }
  if let Some(line_number) = meta.line() {
    if let Some(prefix) = field_prefix {
      buf.extend_from_slice(prefix.as_bytes());
    }
    // Text format is safe as a line number can't possibly contain anything funny
    let line = line_number.to_string();
    put_field_wellformed(buf, "CODE_LINE", line.as_bytes());
  }
}

/// Append a sanitized and length-encoded field into `buf`.
///
/// Unlike `put_field_wellformed` this function handles arbitrary field names and values.
///
/// `name` denotes the field name. It gets sanitized before being appended to `buf`.
///
/// `write_value` is invoked with `buf` as argument to append the value data to `buf`.  It must
/// not delete from `buf`, but may append arbitrary data.  This function then determines the length
/// of the data written and adds it in the appropriate place in `buf`.
fn put_field_length_encoded(buf: &mut Vec<u8>, name: &str, write_value: impl FnOnce(&mut Vec<u8>)) {
  const LENGTH_TAG_BYTES: usize = 8;

  let field_start = buf.len();
  sanitize_name(name, buf);
  buf.push(b'\n');
  let length_slot_start = buf.len();
  buf.extend_from_slice(&[0; LENGTH_TAG_BYTES]); // Length tag, to be populated
  let value_start = buf.len();
  write_value(buf);
  let value_end = buf.len();
  let written_len = value_end.saturating_sub(value_start);
  let Ok(encoded_len) = u64::try_from(written_len) else {
    buf.truncate(field_start);
    return;
  };
  if let Some(length_slot) = buf.get_mut(length_slot_start..value_start) {
    for (slot, byte) in length_slot.iter_mut().zip(encoded_len.to_le_bytes()) {
      *slot = byte;
    }
  }
  buf.push(b'\n');
}

/// Mangle a name into journald-compliant form
#[allow(
  clippy::single_call_fn,
  reason = "journald field-name sanitization remains isolated from length encoding"
)]
fn sanitize_name(name: &str, buf: &mut Vec<u8>) {
  buf.extend(
    name
      .bytes()
      .map(|byte| if byte == b'.' { b'_' } else { byte })
      .skip_while(|byte| *byte == b'_')
      .filter(|byte| *byte == b'_' || byte.is_ascii_alphanumeric())
      .map(|byte| byte.to_ascii_uppercase()),
  );
}

/// Append arbitrary data with a well-formed name and value.
///
/// `field_value` must not contain an internal newline, because this function
/// writes `field_value` in the new-line separated format.
///
/// For a "newline-safe" variant, see `put_field_length_encoded`.
fn put_field_wellformed(buf: &mut Vec<u8>, name: &str, field_value: &[u8]) {
  buf.extend_from_slice(name.as_bytes());
  buf.push(b'\n');
  let Ok(value_len) = u64::try_from(field_value.len()) else {
    return;
  };
  buf.extend_from_slice(&value_len.to_le_bytes());
  buf.extend_from_slice(field_value);
  buf.push(b'\n');
}

#[cfg(test)]
mod tests {
  #[cfg(unix)]
  use std::os::unix::net::UnixDatagram;
  #[cfg(unix)]
  use std::path::Path;
  #[cfg(unix)]
  use std::path::PathBuf;
  use std::sync::Arc;

  use parking_lot::Mutex;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_ok;
  #[cfg(unix)]
  use tracing::dispatcher::with_default;
  use tracing_core::callsite::Callsite;
  use tracing_core::field::FieldSet;
  use tracing_core::metadata::Kind;
  use tracing_core::metadata::SourceLocation;
  use tracing_core::subscriber::Interest;
  use tracing_subscriber::prelude::*;

  use super::*;

  /// Callsite used by journald encoding tests.
  struct JournaldTestCallsite;

  /// Static callsite used for test metadata identity.
  static JOURNALD_TEST_CALLSITE: JournaldTestCallsite = JournaldTestCallsite;

  /// Source location used by journald metadata tests.
  const JOURNALD_TEST_LOCATION: SourceLocation<'static> = SourceLocation::empty()
    .with_module_path(Some("tracing_journald::tests"))
    .with_file(Some("journald_contract.rs"))
    .with_line(Some(77));

  impl Callsite for JournaldTestCallsite {
    fn set_interest(&self, _: Interest) {}

    fn metadata(&self) -> &Metadata<'_> {
      static META: Metadata<'static> = Metadata::new(
        "journald_test",
        "journal_target",
        Level::INFO,
        &JOURNALD_TEST_LOCATION,
        &FieldSet::new(&["message"], tracing_core::identify_callsite!(&JOURNALD_TEST_CALLSITE)),
        Kind::EVENT,
      );
      &META
    }
  }

  /// Ensures a native-protocol payload contains the expected field.
  fn ensure_field(payload: &[u8], name: &str, field_value: &[u8], context: &'static str) -> Result<(), TestFailure> {
    let mut cursor = 0_usize;
    while cursor < payload.len() {
      let Some(remainder) = payload.get(cursor..) else {
        return ensure(false, "native field cursor is in bounds");
      };
      let Some(name_offset) = remainder.iter().position(|byte| *byte == b'\n') else {
        return ensure(false, "native field name terminator is present");
      };
      let name_end = cursor.saturating_add(name_offset);
      let length_start = name_end.saturating_add(1);
      let length_end = length_start.saturating_add(8);
      let Some(length_bytes) = payload.get(length_start..length_end) else {
        return ensure(false, "native field length is present");
      };
      let mut length_buffer = [0_u8; 8];
      for (slot, byte) in length_buffer.iter_mut().zip(length_bytes.iter().copied()) {
        *slot = byte;
      }
      let Ok(value_len) = usize::try_from(u64::from_le_bytes(length_buffer)) else {
        return ensure(false, "native field length fits in usize");
      };
      let Some(value_end) = length_end.checked_add(value_len) else {
        return ensure(false, "native field length does not overflow");
      };
      let Some(actual_name) = payload.get(cursor..name_end) else {
        return ensure(false, "native field name is in bounds");
      };
      let Some(actual_value) = payload.get(length_end..value_end) else {
        return ensure(false, "native field value is in bounds");
      };
      if actual_name == name.as_bytes() && actual_value == field_value {
        return Ok(());
      }
      cursor = value_end.saturating_add(1);
    }

    ensure(false, context)
  }

  #[test]
  fn priority_mappings_preserve_journald_priority_bytes() -> Result<(), TestFailure> {
    let mappings = PriorityMappings::new();
    ensure(mappings.error.as_byte() == b'3', "default error priority is journald error")?;
    ensure(mappings.warn.as_byte() == b'4', "default warn priority is journald warning")?;
    ensure(mappings.info.as_byte() == b'5', "default info priority is journald notice")?;
    ensure(mappings.debug.as_byte() == b'6', "default debug priority is journald informational")?;
    ensure(mappings.trace.as_byte() == b'7', "default trace priority is journald debug")?;
    ensure(Priority::Emergency.as_byte() == b'0', "emergency priority byte is stable")?;
    ensure(Priority::Alert.as_byte() == b'1', "alert priority byte is stable")?;
    ensure(Priority::Critical.as_byte() == b'2', "critical priority byte is stable")
  }

  #[test]
  fn field_encoding_sanitizes_names_and_preserves_multiline_values() -> Result<(), TestFailure> {
    let mut payload = Vec::new();
    put_field_length_encoded(&mut payload, "__bad.field-name", |buf| {
      buf.extend_from_slice(b"first\nsecond\0third");
    });
    ensure_field(
      &payload,
      "BAD_FIELDNAME",
      b"first\nsecond\0third",
      "length-encoded fields preserve multiline and binary values",
    )?;

    put_field_length_encoded(&mut payload, "!!!", |buf| {
      buf.extend_from_slice(b"empty-name");
    });
    ensure_field(
      &payload,
      "",
      b"empty-name",
      "fields whose names sanitize to empty retain an empty native name",
    )
  }

  #[test]
  fn metadata_encoding_writes_target_file_and_line_with_optional_prefix() -> Result<(), TestFailure> {
    let mut payload = Vec::new();
    put_metadata(&mut payload, JOURNALD_TEST_CALLSITE.metadata(), Some("SPAN_"));

    ensure_field(&payload, "SPAN_TARGET", b"journal_target", "metadata target is encoded")?;
    ensure_field(
      &payload,
      "SPAN_CODE_FILE",
      b"journald_contract.rs",
      "metadata file is encoded with prefix",
    )?;
    ensure_field(&payload, "SPAN_CODE_LINE", b"77", "metadata line is encoded with prefix")
  }

  #[test]
  fn wellformed_fields_and_priority_encoding_match_native_protocol() -> Result<(), TestFailure> {
    let mut payload = Vec::new();
    put_field_wellformed(&mut payload, "TARGET", b"journal");
    ensure_field(&payload, "TARGET", b"journal", "well-formed fields use native encoding")?;

    let layer = test_layer()?;
    layer.put_priority(&mut payload, JOURNALD_TEST_CALLSITE.metadata());
    ensure_field(&payload, "PRIORITY", b"5", "layer encodes priorities from metadata")
  }

  #[cfg(unix)]
  #[test]
  fn layer_configuration_debug_includes_journald_fields() -> Result<(), TestFailure> {
    let layer = test_layer()?
      .with_field_prefix(Some("APP".to_owned()))
      .with_syslog_identifier("journald-test".to_owned())
      .with_custom_fields([("SYSLOG_FACILITY", "17")]);
    let rendered = format!("{layer:?}");
    ensure_contains(&rendered, "field_prefix", "debug output names field prefix")?;
    ensure_contains(&rendered, "journald-test", "debug output includes syslog identifier")?;
    ensure_contains(&rendered, "additional_fields", "debug output includes custom field storage")?;
    ensure_contains(&rendered, "priority_mappings", "debug output includes priority mappings")
  }

  #[cfg(unix)]
  #[test]
  fn layer_formats_span_records_and_events_before_ignoring_socket_errors() -> Result<(), TestFailure> {
    #[derive(Clone, Debug, Default)]
    struct EventCounter {
      events: Arc<Mutex<Vec<String>>>,
    }

    impl<S> tracing_subscriber::Layer<S> for EventCounter
    where
      S: Subscriber,
    {
      fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) -> SubscriberResult {
        let mut events = self.events.lock();
        events.push(event.metadata().target().to_owned());
        drop(events);
        Ok(())
      }
    }

    let layer = test_layer()?
      .with_field_prefix(Some("APP".to_owned()))
      .with_syslog_identifier("journald-test".to_owned())
      .with_custom_fields([("EXTRA_FIELD", "extra")]);
    let counter = EventCounter::default();
    let dispatcher = tracing::Dispatch::new(tracing_subscriber::registry().with(layer).with(counter.clone()));

    with_default(&dispatcher, || {
      let span = tracing::info_span!("journal_span", span_field = "before");
      let _entered = span.enter();
      let _recorded_span = span.record("span_field", "after");
      tracing::warn!(target: "journald_contracts", event_field = "visible", "journal event");
    });

    let events = counter.events.lock();
    ensure(
      *events == ["journald_contracts".to_owned()],
      "journald layer returns success even when sending to a missing socket fails",
    )
  }

  #[cfg(unix)]
  #[test]
  fn namespace_paths_select_system_and_user_journal_locations() -> Result<(), TestFailure> {
    ensure(
      JournalNamespace::System.socket_path().as_path() == Path::new(SYSTEM_JOURNALD_PATH),
      "system namespace selects the system journald socket",
    )?;
    ensure(
      JournalNamespace::User
        .socket_path()
        .ends_with(Path::new("systemd/journal/socket")),
      "user namespace selects a user journald socket suffix",
    )
  }

  /// Builds a journald layer pointed at a deliberately missing socket.
  #[cfg(unix)]
  fn test_layer() -> Result<Layer, TestFailure> {
    Ok(Layer {
      socket:            ensure_ok(UnixDatagram::unbound(), "test journald datagram socket opens")?,
      socket_path:       PathBuf::from("/tmp/strict-tracing-missing-journald.sock"),
      field_prefix:      Some("F".to_owned()),
      syslog_identifier: "test-binary".to_owned(),
      additional_fields: Vec::new(),
      priority_mappings: PriorityMappings::new(),
    })
  }
}
