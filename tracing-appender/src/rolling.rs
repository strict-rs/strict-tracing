//! A rolling file appender.
//!
//! Creates a new log file at a fixed frequency as defined by [`Rotation`].
//! Logs will be written to this file for the duration of the period and will automatically roll over
//! to the newly created log file once the time period has elapsed.
//!
//! The log file is created at the specified directory and file name prefix which *may* be appended with
//! the date and time.
//!
//! The following helpers are available for creating a rolling file appender.
//!
//! - [`Rotation::minutely()`][minutely]: A new log file in the format of `some_directory/log_file_name_prefix.yyyy-MM-dd-HH-mm`
//!   will be created minutely (once per minute)
//! - [`Rotation::hourly()`][hourly]: A new log file in the format of `some_directory/log_file_name_prefix.yyyy-MM-dd-HH`
//!   will be created hourly
//! - [`Rotation::daily()`][daily]: A new log file in the format of `some_directory/log_file_name_prefix.yyyy-MM-dd`
//!   will be created daily
//! - [`Rotation::never()`][never()]: This will result in log file located at `some_directory/log_file_name`
//!
//!
//! # Examples
//!
//! ```rust
//! # fn docs() -> Result<(), Box<dyn std::error::Error>> {
//! use tracing_appender::rolling::{RollingFileAppender, Rotation};
//! let file_appender = RollingFileAppender::new(Rotation::HOURLY, "/some/directory", "prefix.log")?;
//! # drop(file_appender);
//! # Ok(())
//! # }
//! ```
use crate::sync::{RwLock, RwLockReadGuard};
use std::{
    fmt::{self, Debug},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicI64, Ordering},
    time::SystemTime,
};
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time, format_description};
use tracing_subscriber::fmt::writer::MakeWriter;

/// Builder-based rolling appender configuration.
mod builder;
pub use builder::{Builder, InitError};

/// A file appender with the ability to rotate log files at a fixed schedule.
///
/// `RollingFileAppender` implements the [`std:io::Write` trait][write] and will
/// block on write operations. It may be used with [`NonBlocking`] to perform
/// writes without blocking the current thread.
///
/// Additionally, `RollingFileAppender` also implements the [`MakeWriter`]
/// trait from `tracing-subscriber`, so it may also be used
/// directly, without [`NonBlocking`].
///
/// [write]: std::io::Write
/// [`NonBlocking`]: super::non_blocking::NonBlocking
///
/// # Examples
///
/// Rolling a log file once every hour:
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
/// let file_appender = tracing_appender::rolling::hourly("/some/directory", "prefix")?;
/// # drop(file_appender);
/// # Ok(())
/// # }
/// ```
///
/// Combining a `RollingFileAppender` with another [`MakeWriter`] implementation:
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// use tracing_subscriber::fmt::writer::MakeWriterExt;
///
/// // Log all events to a rolling log file.
/// let logfile = tracing_appender::rolling::hourly("/logs", "myapp-logs")?;
///
/// // Log `INFO` and above to stdout.
/// let stdout = std::io::stdout.with_max_level(tracing::Level::INFO);
///
/// tracing_subscriber::fmt()
///     // Combine the stdout and log file `MakeWriter`s into one
///     // `MakeWriter` that writes to both
///     .with_writer(stdout.and(logfile))
///     .try_init()?;
/// # Ok(())
/// # }
/// ```
///
/// [`MakeWriter`]: tracing_subscriber::fmt::writer::MakeWriter
pub struct RollingFileAppender {
    /// The immutable rolling appender configuration and rollover timestamp.
    state: Inner,
    /// The current log file, swapped when a rollover succeeds.
    writer: RwLock<File>,
    /// Test-only clock used to make rollover tests deterministic.
    #[cfg(test)]
    now: Box<dyn Fn() -> OffsetDateTime + Send + Sync>,
}

/// A [writer] that writes to a rolling log file.
///
/// This is returned by the [`MakeWriter`] implementation for [`RollingFileAppender`].
///
/// [writer]: std::io::Write
/// [`MakeWriter`]: tracing_subscriber::fmt::writer::MakeWriter
#[derive(Debug)]
pub struct RollingWriter<'a> {
    /// Active log file guard, or `None` when rollover failed before acquisition.
    file: Option<RwLockReadGuard<'a, File>>,
}

/// Parsed format description used for rolling log filename dates.
type DateFormat = Vec<format_description::BorrowedFormatItem<'static>>;

/// Stored timestamp for appenders that never roll over.
///
/// `time::OffsetDateTime` cannot represent this value, so it cannot collide
/// with a real Unix timestamp produced by [`OffsetDateTime::unix_timestamp`].
const NEVER_ROLLOVER_TIMESTAMP: i64 = i64::MIN;

/// Rolling appender state shared by direct writes and `MakeWriter` writers.
#[derive(Debug)]
struct Inner {
    /// Directory where log files are stored.
    log_directory: PathBuf,
    /// Optional filename prefix written before the timestamp.
    log_filename_prefix: Option<String>,
    /// Optional filename suffix written after the timestamp.
    log_filename_suffix: Option<String>,
    /// Optional symlink name that points to the latest log file.
    log_latest_symlink_name: Option<String>,
    /// Timestamp format used in rolling filenames.
    date_format: DateFormat,
    /// Rotation cadence used to round timestamps and compute the next rollover.
    rotation: Rotation,
    /// Unix timestamp for the next rollover, or [`NEVER_ROLLOVER_TIMESTAMP`].
    next_date: AtomicI64,
    /// Maximum number of matching log files to retain.
    max_files: Option<usize>,
}

// === impl RollingFileAppender ===

impl RollingFileAppender {
    /// Creates a new `RollingFileAppender`.
    ///
    /// A `RollingFileAppender` will have a fixed rotation whose frequency is
    /// defined by [`Rotation`]. The `directory` and `file_name_prefix`
    /// arguments determine the location and file name's _prefix_ of the log
    /// file. `RollingFileAppender` will automatically append the current date
    /// and hour (UTC format) to the file name.
    ///
    /// Alternatively, a `RollingFileAppender` can be constructed using one of the following helpers:
    ///
    /// - [`Rotation::minutely()`][minutely],
    /// - [`Rotation::hourly()`][hourly],
    /// - [`Rotation::daily()`][daily],
    /// - [`Rotation::never()`][never()]
    ///
    /// Additional parameters can be configured using [`RollingFileAppender::builder`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
    /// use tracing_appender::rolling::{RollingFileAppender, Rotation};
    /// let file_appender = RollingFileAppender::new(Rotation::HOURLY, "/some/directory", "prefix.log")?;
    /// # drop(file_appender);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`InitError`] when the filename prefix is not valid UTF-8 or the
    /// initial log file cannot be created.
    pub fn new(
        rotation: Rotation,
        directory: impl AsRef<Path>,
        filename_prefix: impl AsRef<Path>,
    ) -> Result<Self, InitError> {
        let filename_prefix_text = filename_prefix.as_ref().to_str().ok_or_else(|| {
            InitError::ctx("filename prefix must be a valid UTF-8 string")(
                io::ErrorKind::InvalidInput.into(),
            )
        })?;
        Self::builder()
            .rotation(rotation)
            .filename_prefix(filename_prefix_text)
            .build(directory)
    }

    /// Returns a new [`Builder`] for configuring a `RollingFileAppender`.
    ///
    /// The builder interface can be used to set additional configuration
    /// parameters when constructing a new appender.
    ///
    /// Like [`RollingFileAppender::new`], the [`Builder::build`] method returns
    /// a `Result` when the appender cannot be initialized. The builder
    /// interface additionally configures filename suffixes, latest-log
    /// symlinks, and log file retention.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
    /// use tracing_appender::rolling::{RollingFileAppender, Rotation};
    ///
    /// let file_appender = RollingFileAppender::builder()
    ///     .rotation(Rotation::HOURLY) // rotate log files once every hour
    ///     .filename_prefix("myapp") // log file names will be prefixed with `myapp.`
    ///     .filename_suffix("log") // log file names will be suffixed with `.log`
    ///     .build("/var/log") // try to build an appender that stores log files in `/var/log`
    ///     ?;
    /// # drop(file_appender);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    #[allow(
        clippy::single_call_fn,
        reason = "public builder entry point remains part of the rolling appender API"
    )]
    pub const fn builder() -> Builder {
        Builder::new()
    }

    /// Builds an appender from the public builder state.
    #[allow(
        clippy::single_call_fn,
        reason = "builder module delegates initialization without reaching into private rolling state"
    )]
    fn from_builder(builder: &Builder, directory: impl AsRef<Path>) -> Result<Self, InitError> {
        let log_directory = directory.as_ref().to_path_buf();
        let now = OffsetDateTime::now_utc();
        let (state, writer) = Inner::new(
            now,
            builder.rotation,
            log_directory,
            builder.prefix.clone(),
            builder.suffix.clone(),
            builder.latest_symlink.clone(),
            builder.max_files,
        )?;
        Ok(Self {
            state,
            writer,
            #[cfg(test)]
            now: Box::new(OffsetDateTime::now_utc),
        })
    }

    #[inline]
    /// Returns the current timestamp from the test clock.
    #[cfg(test)]
    fn now(&self) -> OffsetDateTime {
        (self.now)()
    }

    #[inline]
    /// Returns the current UTC timestamp.
    #[cfg(not(test))]
    fn now() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

impl Write for RollingFileAppender {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(test)]
        let now = self.now();
        #[cfg(not(test))]
        let now = Self::now();
        let writer = self.writer.get_mut();
        self.state.try_rollover(now, writer)?;
        writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.get_mut().flush()
    }
}

impl<'a> MakeWriter<'a> for RollingFileAppender {
    type Writer = RollingWriter<'a>;

    /// Creates a writer for the active rolling log file.
    fn make_writer(&'a self) -> Self::Writer {
        #[cfg(test)]
        let now = self.now();
        #[cfg(not(test))]
        let now = Self::now();

        // Should we try to roll over the log file?
        if self.state.should_rollover(now).is_some() {
            let refresh_result = {
                let mut writer = self.writer.write();
                self.state.try_rollover(now, &mut writer)
            };

            if refresh_result.is_err() {
                return RollingWriter { file: None };
            }
        }
        RollingWriter {
            file: Some(self.writer.read()),
        }
    }
}

impl Debug for RollingFileAppender {
    // This manual impl is required because of the `now` field (only present
    // with `cfg(test)`), which is not `Debug`...
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[cfg(test)]
        {
            f.debug_struct("RollingFileAppender")
                .field("state", &self.state)
                .field("writer", &self.writer)
                .field("now", &"test clock")
                .finish()
        }

        #[cfg(not(test))]
        {
            f.debug_struct("RollingFileAppender")
                .field("state", &self.state)
                .field("writer", &self.writer)
                .finish()
        }
    }
}

/// Creates a minutely-rotating file appender. This will rotate the log file once per minute.
///
/// The appender returned by `rolling::minutely` can be used with `non_blocking` to create
/// a non-blocking, minutely file appender.
///
/// The directory of the log file is specified with the `directory` argument.
/// `file_name_prefix` specifies the _prefix_ of the log file. `RollingFileAppender`
/// adds the current date, hour, and minute to the log file in UTC.
///
/// # Examples
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
///     let appender = tracing_appender::rolling::minutely("/some/path", "rolling.log")?;
///     let (non_blocking_appender, _guard) = tracing_appender::non_blocking(appender);
///
///     let subscriber = tracing_subscriber::fmt().with_writer(non_blocking_appender);
///
///     tracing::subscriber::with_default(subscriber.finish(), || {
///         tracing::event!(tracing::Level::INFO, "Hello");
///     });
/// # Ok(())
/// # }
/// ```
///
/// This will result in a log file located at `/some/path/rolling.log.yyyy-MM-dd-HH-mm`.
///
/// # Errors
///
/// Returns [`InitError`] when the filename prefix is not valid UTF-8 or the
/// initial log file cannot be created.
pub fn minutely(
    directory: impl AsRef<Path>,
    file_name_prefix: impl AsRef<Path>,
) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::new(Rotation::MINUTELY, directory, file_name_prefix)
}

/// Creates an hourly-rotating file appender.
///
/// The appender returned by `rolling::hourly` can be used with `non_blocking` to create
/// a non-blocking, hourly file appender.
///
/// The directory of the log file is specified with the `directory` argument.
/// `file_name_prefix` specifies the _prefix_ of the log file. `RollingFileAppender`
/// adds the current date and hour to the log file in UTC.
///
/// # Examples
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
///     let appender = tracing_appender::rolling::hourly("/some/path", "rolling.log")?;
///     let (non_blocking_appender, _guard) = tracing_appender::non_blocking(appender);
///
///     let subscriber = tracing_subscriber::fmt().with_writer(non_blocking_appender);
///
///     tracing::subscriber::with_default(subscriber.finish(), || {
///         tracing::event!(tracing::Level::INFO, "Hello");
///     });
/// # Ok(())
/// # }
/// ```
///
/// This will result in a log file located at `/some/path/rolling.log.yyyy-MM-dd-HH`.
///
/// # Errors
///
/// Returns [`InitError`] when the filename prefix is not valid UTF-8 or the
/// initial log file cannot be created.
pub fn hourly(
    directory: impl AsRef<Path>,
    file_name_prefix: impl AsRef<Path>,
) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::new(Rotation::HOURLY, directory, file_name_prefix)
}

/// Creates a daily-rotating file appender.
///
/// The appender returned by `rolling::daily` can be used with `non_blocking` to create
/// a non-blocking, daily file appender.
///
/// A `RollingFileAppender` has a fixed rotation whose frequency is
/// defined by [`Rotation`]. The `directory` and `file_name_prefix`
/// arguments determine the location and file name's _prefix_ of the log file.
/// `RollingFileAppender` automatically appends the current date in UTC.
///
/// # Examples
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
///     let appender = tracing_appender::rolling::daily("/some/path", "rolling.log")?;
///     let (non_blocking_appender, _guard) = tracing_appender::non_blocking(appender);
///
///     let subscriber = tracing_subscriber::fmt().with_writer(non_blocking_appender);
///
///     tracing::subscriber::with_default(subscriber.finish(), || {
///         tracing::event!(tracing::Level::INFO, "Hello");
///     });
/// # Ok(())
/// # }
/// ```
///
/// This will result in a log file located at `/some/path/rolling.log.yyyy-MM-dd`.
///
/// # Errors
///
/// Returns [`InitError`] when the filename prefix is not valid UTF-8 or the
/// initial log file cannot be created.
pub fn daily(
    directory: impl AsRef<Path>,
    file_name_prefix: impl AsRef<Path>,
) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::new(Rotation::DAILY, directory, file_name_prefix)
}

/// Creates a weekly-rotating file appender. The logs will rotate every Sunday at midnight UTC.
///
/// The appender returned by `rolling::weekly` can be used with `non_blocking` to create
/// a non-blocking, weekly file appender.
///
/// A `RollingFileAppender` has a fixed rotation whose frequency is
/// defined by [`Rotation`]. The `directory` and `file_name_prefix` arguments
/// determine the location and file name's _prefix_ of the log file.
/// `RollingFileAppender` automatically appends the current date in UTC.
///
/// # Examples
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
///     let appender = tracing_appender::rolling::weekly("/some/path", "rolling.log")?;
///     let (non_blocking_appender, _guard) = tracing_appender::non_blocking(appender);
///
///     let subscriber = tracing_subscriber::fmt().with_writer(non_blocking_appender);
///
///     tracing::subscriber::with_default(subscriber.finish(), || {
///         tracing::event!(tracing::Level::INFO, "Hello");
///     });
/// # Ok(())
/// # }
/// ```
///
/// This will result in a log file located at `/some/path/rolling.log.yyyy-MM-dd`.
///
/// # Errors
///
/// Returns [`InitError`] when the filename prefix is not valid UTF-8 or the
/// initial log file cannot be created.
pub fn weekly(
    directory: impl AsRef<Path>,
    file_name_prefix: impl AsRef<Path>,
) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::new(Rotation::WEEKLY, directory, file_name_prefix)
}

/// Creates a non-rolling file appender.
///
/// The appender returned by `rolling::never` can be used with `non_blocking` to create
/// a non-blocking, non-rotating appender.
///
/// The location of the log file will be specified the `directory` passed in.
/// `file_name` specifies the complete name of the log file (no date or time is appended).
///
/// # Examples
///
/// ```rust
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
///     let appender = tracing_appender::rolling::never("/some/path", "non-rolling.log")?;
///     let (non_blocking_appender, _guard) = tracing_appender::non_blocking(appender);
///
///     let subscriber = tracing_subscriber::fmt().with_writer(non_blocking_appender);
///
///     tracing::subscriber::with_default(subscriber.finish(), || {
///         tracing::event!(tracing::Level::INFO, "Hello");
///     });
/// # Ok(())
/// # }
/// ```
///
/// This will result in a log file located at `/some/path/non-rolling.log`.
///
/// # Errors
///
/// Returns [`InitError`] when the file name is not valid UTF-8 or the initial
/// log file cannot be created.
pub fn never(
    directory: impl AsRef<Path>,
    file_name: impl AsRef<Path>,
) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::new(Rotation::NEVER, directory, file_name)
}

/// Defines a fixed period for rolling of a log file.
///
/// To use a `Rotation`, pick one of the following options:
///
/// ### Minutely Rotation
/// ```rust
/// # fn docs() {
/// use tracing_appender::rolling::Rotation;
/// let rotation = tracing_appender::rolling::Rotation::MINUTELY;
/// # }
/// ```
///
/// ### Hourly Rotation
/// ```rust
/// # fn docs() {
/// use tracing_appender::rolling::Rotation;
/// let rotation = tracing_appender::rolling::Rotation::HOURLY;
/// # }
/// ```
///
/// ### Daily Rotation
/// ```rust
/// # fn docs() {
/// use tracing_appender::rolling::Rotation;
/// let rotation = tracing_appender::rolling::Rotation::DAILY;
/// # }
/// ```
///
/// ### Weekly Rotation
/// ```rust
/// # fn docs() {
/// use tracing_appender::rolling::Rotation;
/// let rotation = tracing_appender::rolling::Rotation::WEEKLY;
/// # }
/// ```
///
/// ### No Rotation
/// ```rust
/// # fn docs() {
/// use tracing_appender::rolling::Rotation;
/// let rotation = tracing_appender::rolling::Rotation::NEVER;
/// # }
/// ```
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct Rotation(RotationKind);

/// Supported rolling cadence variants.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
enum RotationKind {
    /// Rotate once per minute.
    Minutely,
    /// Rotate once per hour.
    Hourly,
    /// Rotate once per day.
    Daily,
    /// Rotate once per week on Sunday at midnight UTC.
    Weekly,
    /// Do not rotate.
    Never,
}

impl Rotation {
    /// Provides a minutely rotation.
    pub const MINUTELY: Self = Self(RotationKind::Minutely);
    /// Provides an hourly rotation.
    pub const HOURLY: Self = Self(RotationKind::Hourly);
    /// Provides a daily rotation.
    pub const DAILY: Self = Self(RotationKind::Daily);
    /// Provides a weekly rotation that rotates every Sunday at midnight UTC.
    pub const WEEKLY: Self = Self(RotationKind::Weekly);
    /// Provides a rotation that never rotates.
    pub const NEVER: Self = Self(RotationKind::Never);

    /// Determines the next date that should be rounded to, if this rotation rolls over.
    pub(crate) fn next_date(self, current_date: OffsetDateTime) -> Option<OffsetDateTime> {
        let unrounded_next_date = match self.0 {
            RotationKind::Minutely => current_date.checked_add(Duration::MINUTE)?,
            RotationKind::Hourly => current_date.checked_add(Duration::HOUR)?,
            RotationKind::Daily => current_date.checked_add(Duration::DAY)?,
            RotationKind::Weekly => current_date.checked_add(Duration::WEEK)?,
            RotationKind::Never => return None,
        };
        self.round_date(unrounded_next_date)
    }

    /// Rounds the date towards the past using the [`Rotation`] interval.
    pub(crate) fn round_date(self, date: OffsetDateTime) -> Option<OffsetDateTime> {
        match self.0 {
            RotationKind::Minutely => {
                let rounded_time = Time::from_hms(date.hour(), date.minute(), 0).ok()?;
                Some(date.replace_time(rounded_time))
            }
            RotationKind::Hourly => {
                let rounded_time = Time::from_hms(date.hour(), 0, 0).ok()?;
                Some(date.replace_time(rounded_time))
            }
            RotationKind::Daily => Some(date.replace_time(Time::MIDNIGHT)),
            RotationKind::Weekly => {
                let days_since_sunday = date.weekday().number_days_from_sunday();
                let rounded_date = date.checked_sub(Duration::days(days_since_sunday.into()))?;
                Some(rounded_date.replace_time(Time::MIDNIGHT))
            }
            RotationKind::Never => None,
        }
    }

    /// Returns the filename timestamp format used by this rotation.
    fn date_format(self) -> io::Result<DateFormat> {
        let format = match self.0 {
            RotationKind::Minutely => "[year]-[month]-[day]-[hour]-[minute]",
            RotationKind::Hourly => "[year]-[month]-[day]-[hour]",
            RotationKind::Daily | RotationKind::Weekly | RotationKind::Never => {
                "[year]-[month]-[day]"
            }
        };
        format_description::parse_borrowed::<1>(format).map_err(|_error| invalid_data_error())
    }
}

impl Write for RollingWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let Some(file) = self.file.as_mut() else {
            return Err(other_error());
        };

        let mut file_ref = &**file;
        file_ref.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Err(other_error());
        };

        let mut file_ref = &**file;
        file_ref.flush()
    }
}

// === impl Inner ===

impl Inner {
    /// Creates the rolling appender state and initial file writer.
    #[allow(
        clippy::single_call_fn,
        reason = "constructor isolates rolling state initialization, retention pruning, and first writer creation"
    )]
    fn new(
        now: OffsetDateTime,
        rotation: Rotation,
        directory: impl AsRef<Path>,
        log_filename_prefix: Option<String>,
        log_filename_suffix: Option<String>,
        log_latest_symlink_name: Option<String>,
        max_files: Option<usize>,
    ) -> Result<(Self, RwLock<File>), InitError> {
        let log_directory = directory.as_ref().to_path_buf();
        let date_format = rotation
            .date_format()
            .map_err(InitError::ctx("failed to build rolling date format"))?;
        let next_date = rotation.next_date(now);

        let inner = Self {
            log_directory,
            log_filename_prefix,
            log_filename_suffix,
            log_latest_symlink_name,
            date_format,
            next_date: AtomicI64::new(
                next_date.map_or(NEVER_ROLLOVER_TIMESTAMP, OffsetDateTime::unix_timestamp),
            ),
            rotation,
            max_files,
        };

        if let Some(max_file_count) = max_files {
            inner
                .prune_old_logs(max_file_count)
                .map_err(InitError::ctx("failed to prune old log files"))?;
        }

        let filename = inner
            .join_date(now)
            .map_err(InitError::ctx("failed to format rolling log filename"))?;
        let writer = RwLock::new(create_writer(
            inner.log_directory.as_ref(),
            &filename,
            inner.log_latest_symlink_name.as_deref(),
        )?);
        Ok((inner, writer))
    }

    /// Returns the full filename for the provided date, using [`Rotation`] to round accordingly.
    pub(crate) fn join_date(&self, date: OffsetDateTime) -> io::Result<String> {
        if self.rotation == Rotation::NEVER {
            return match (
                self.log_filename_prefix.as_deref(),
                self.log_filename_suffix.as_deref(),
            ) {
                (Some(filename), None) => Ok(filename.to_owned()),
                (Some(filename), Some(suffix)) => Ok(format!("{filename}.{suffix}")),
                (None, Some(suffix)) => Ok(suffix.to_owned()),
                (None, None) => format_date(date, &self.date_format),
            };
        }

        let rounded_date = self
            .rotation
            .round_date(date)
            .ok_or_else(invalid_data_error)?;
        let formatted_date = format_date(rounded_date, &self.date_format)?;

        let filename = match (
            self.log_filename_prefix.as_deref(),
            self.log_filename_suffix.as_deref(),
        ) {
            (Some(filename), Some(suffix)) => format!("{filename}.{formatted_date}.{suffix}"),
            (Some(filename), None) => format!("{filename}.{formatted_date}"),
            (None, Some(suffix)) => format!("{formatted_date}.{suffix}"),
            (None, None) => formatted_date,
        };

        Ok(filename)
    }

    /// Deletes the oldest matching log files when retention is configured.
    fn prune_old_logs(&self, max_files: usize) -> io::Result<()> {
        if max_files == 0 {
            return Ok(());
        }

        let mut files = fs::read_dir(&self.log_directory)?
            .filter_map(|entry_result| {
                let entry = entry_result.ok()?;
                let metadata = entry.metadata().ok()?;

                // The appender only creates files, not directories or symlinks,
                // so we should never delete a directory or symlink.
                if !metadata.is_file() {
                    return None;
                }

                let file_name = entry.file_name();
                // If the filename is not a UTF-8 string, skip it.
                let file_name_text = file_name.to_str()?;
                if let Some(prefix) = self.log_filename_prefix.as_deref()
                    && !file_name_text.starts_with(prefix)
                {
                    return None;
                }

                if let Some(suffix) = self.log_filename_suffix.as_deref()
                    && !file_name_text.ends_with(suffix)
                {
                    return None;
                }

                if self.log_filename_prefix.is_none()
                    && self.log_filename_suffix.is_none()
                    && Date::parse(file_name_text, &self.date_format).is_err()
                {
                    return None;
                }

                let created = metadata.created().ok().or_else(|| {
                    parse_date_from_filename(
                        file_name_text,
                        &self.date_format,
                        self.log_filename_prefix.as_deref(),
                        self.log_filename_suffix.as_deref(),
                    )
                })?;
                Some((entry, created))
            })
            .collect::<Vec<_>>();

        if files.len() < max_files {
            return Ok(());
        }

        // sort the files by their creation timestamps.
        files.sort_by_key(|file_entry| file_entry.1);

        // Delete files so that `max_files - 1` files remain, because rollover
        // will create one more log file immediately after pruning.
        let retained_file_count = max_files.saturating_sub(1);
        let removal_count = files.len().saturating_sub(retained_file_count);
        for file_entry in files.iter().take(removal_count) {
            fs::remove_file(file_entry.0.path())?;
        }

        Ok(())
    }

    /// Replaces the active file with the log file for `now`.
    fn refresh_writer(&self, now: OffsetDateTime, file: &mut File) -> io::Result<()> {
        let filename = self.join_date(now)?;

        if let Some(max_file_count) = self.max_files {
            self.prune_old_logs(max_file_count)?;
        }

        let new_file = create_writer(
            &self.log_directory,
            &filename,
            self.log_latest_symlink_name.as_deref(),
        )
        .map_err(|_error| other_error())?;
        file.flush()?;
        *file = new_file;
        Ok(())
    }

    /// Checks whether or not it's time to roll over the log file.
    ///
    /// Rather than returning a `bool`, this returns the current value of
    /// `next_date` so that we can perform a `compare_exchange` operation with
    /// that value when setting the next rollover time.
    ///
    /// If this method returns `Some`, we should roll to a new log file.
    /// Otherwise, if this returns we should not rotate the log file.
    fn should_rollover(&self, date: OffsetDateTime) -> Option<i64> {
        let next_date = self.next_date.load(Ordering::Acquire);
        // If the next date is the sentinel, this appender never rotates log files.
        if next_date == NEVER_ROLLOVER_TIMESTAMP {
            return None;
        }

        if date.unix_timestamp() >= next_date {
            return Some(next_date);
        }

        None
    }

    /// Advances the stored rollover timestamp if this caller won the rollover race.
    fn advance_date(&self, now: OffsetDateTime, current: i64) -> Option<i64> {
        let next_date = self
            .rotation
            .next_date(now)
            .map_or(NEVER_ROLLOVER_TIMESTAMP, OffsetDateTime::unix_timestamp);
        self.next_date
            .compare_exchange(current, next_date, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then_some(next_date)
    }

    /// Restores the previous rollover timestamp after a refresh failure.
    fn restore_date_after_failed_rollover(&self, attempted: i64, previous: i64) {
        let _restore_result = self.next_date.compare_exchange(
            attempted,
            previous,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Rolls the active writer over when the configured timestamp has elapsed.
    fn try_rollover(&self, now: OffsetDateTime, file: &mut File) -> io::Result<()> {
        let Some(current_timestamp) = self.should_rollover(now) else {
            return Ok(());
        };

        let Some(next_timestamp) = self.advance_date(now, current_timestamp) else {
            return Ok(());
        };

        if let Err(error) = self.refresh_writer(now, file) {
            self.restore_date_after_failed_rollover(next_timestamp, current_timestamp);
            return Err(error);
        }

        Ok(())
    }
}

/// Opens the current log file and updates the optional latest-log symlink.
fn create_writer(
    directory: &Path,
    filename: &str,
    latest_symlink_name: Option<&str>,
) -> Result<File, InitError> {
    let path = directory.join(filename);
    let mut open_options = OpenOptions::new();
    let options = open_options.append(true).create(true);

    let new_file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(InitError::ctx("failed to create log directory"))?;
            }
            options
                .open(&path)
                .map_err(InitError::ctx("failed to create log file"))?
        }
        Err(error) => return Err(InitError::ctx("failed to create log file")(error)),
    };

    if let Some(symlink_name) = latest_symlink_name {
        let symlink_path = directory.join(symlink_name);
        if let Err(error) = symlink::remove_symlink_file(&symlink_path)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(InitError::ctx(
                "failed to remove previous latest log symlink",
            )(error));
        }
        symlink::symlink_file(path, symlink_path).map_err(InitError::ctx(
            "failed to create symlink to latest log file",
        ))?;
    }

    Ok(new_file)
}

/// Formats an [`OffsetDateTime`] using the stored rolling filename format.
fn format_date(
    date: OffsetDateTime,
    date_format: &[format_description::BorrowedFormatItem<'_>],
) -> io::Result<String> {
    date.format(date_format)
        .map_err(|_error| invalid_data_error())
}

/// Returns a generic invalid-data I/O error for impossible static format failures.
fn invalid_data_error() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

/// Returns a generic rollover I/O error when the concrete error type cannot be exposed.
fn other_error() -> io::Error {
    io::ErrorKind::Other.into()
}

/// Parses the rolling timestamp embedded in a log filename.
#[allow(
    clippy::single_call_fn,
    reason = "retention pruning and focused tests share filename timestamp parsing as a named rule"
)]
fn parse_date_from_filename(
    filename: &str,
    date_format: &[format_description::BorrowedFormatItem<'_>],
    filename_prefix: Option<&str>,
    filename_suffix: Option<&str>,
) -> Option<SystemTime> {
    let mut datetime = filename;
    if let Some(prefix) = filename_prefix {
        datetime = datetime.strip_prefix(prefix)?;
        datetime = datetime.strip_prefix('.')?;
    }
    if let Some(suffix) = filename_suffix {
        datetime = datetime.strip_suffix(suffix)?;
        datetime = datetime.strip_suffix('.')?;
    }

    PrimitiveDateTime::parse(datetime, date_format)
        .or_else(|_| {
            Date::parse(datetime, date_format)
                .map(|parsed_date| parsed_date.with_time(Time::MIDNIGHT))
        })
        .ok()
        .map(|dt| dt.assume_utc().into())
}

#[cfg(test)]
mod test {
    use super::*;
    use parking_lot::Mutex;
    use std::sync::Arc;
    use std::{fs, thread, time::Duration as StdDuration};
    use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok, ensure_some};
    use tracing::subscriber::set_default;
    use tracing_subscriber::filter::LevelFilter;

    /// Builds an appender for `rotation` and verifies a single write reaches disk.
    fn test_appender(rotation: Rotation, file_prefix: &str) -> Result<(), TestFailure> {
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let mut appender = ensure_ok(
            RollingFileAppender::new(rotation, directory.path(), file_prefix),
            "initialize rolling file appender",
        )?;

        let expected_value = "Hello";
        ensure_ok(
            appender.write_all(expected_value.as_bytes()),
            "write to appender",
        )?;
        ensure_ok(appender.flush(), "flush appender")?;

        let dir_contents = ensure_ok(fs::read_dir(directory.path()), "read log directory")?;
        let mut found_expected_value = false;
        for entry in dir_contents {
            let path = ensure_ok(entry, "read log directory entry")?.path();
            let file = ensure_ok(fs::read_to_string(&path), "read log file")?;
            if file.as_str() == expected_value {
                found_expected_value = true;
                break;
            }
        }

        ensure(
            found_expected_value,
            "expected value is written to a log file",
        )?;

        ensure_ok(directory.close(), "close tempdir")
    }

    /// Verifies minutely logs can be written.
    #[test]
    fn write_minutely_log() -> Result<(), TestFailure> {
        test_appender(Rotation::MINUTELY, "minutely.log")
    }

    /// Verifies hourly logs can be written.
    #[test]
    fn write_hourly_log() -> Result<(), TestFailure> {
        test_appender(Rotation::HOURLY, "hourly.log")
    }

    /// Verifies daily logs can be written.
    #[test]
    fn write_daily_log() -> Result<(), TestFailure> {
        test_appender(Rotation::DAILY, "daily.log")
    }

    /// Verifies weekly logs can be written.
    #[test]
    fn write_weekly_log() -> Result<(), TestFailure> {
        test_appender(Rotation::WEEKLY, "weekly.log")
    }

    /// Verifies non-rolling logs can be written.
    #[test]
    fn write_never_log() -> Result<(), TestFailure> {
        test_appender(Rotation::NEVER, "never.log")
    }

    /// Verifies each rotation computes the expected next timestamp.
    #[test]
    fn test_rotations() -> Result<(), TestFailure> {
        // per-minute basis
        let minutely_now = OffsetDateTime::now_utc();
        let minutely_next = ensure_some(
            Rotation::MINUTELY.next_date(minutely_now),
            "minutely next date",
        )?;
        let minutely_expected_next = ensure_some(
            minutely_now.checked_add(Duration::MINUTE),
            "compute expected minutely next date",
        )?;
        ensure_eq(
            &minutely_expected_next.minute(),
            &minutely_next.minute(),
            "minutely rotation advances minute",
        )?;

        // per-hour basis
        let hourly_now = OffsetDateTime::now_utc();
        let hourly_next = ensure_some(Rotation::HOURLY.next_date(hourly_now), "hourly next date")?;
        let hourly_expected_next = ensure_some(
            hourly_now.checked_add(Duration::HOUR),
            "compute expected hourly next date",
        )?;
        ensure_eq(
            &hourly_expected_next.hour(),
            &hourly_next.hour(),
            "hourly rotation advances hour",
        )?;

        // per-day basis
        let daily_now = OffsetDateTime::now_utc();
        let daily_next = ensure_some(Rotation::DAILY.next_date(daily_now), "daily next date")?;
        let daily_expected_next = ensure_some(
            daily_now.checked_add(Duration::DAY),
            "compute expected daily next date",
        )?;
        ensure_eq(
            &daily_expected_next.day(),
            &daily_next.day(),
            "daily rotation advances day",
        )?;

        // per-week basis
        let weekly_now = OffsetDateTime::now_utc();
        let weekly_now_rounded = ensure_some(
            Rotation::WEEKLY.round_date(weekly_now),
            "weekly rounded date",
        )?;
        let weekly_next = ensure_some(Rotation::WEEKLY.next_date(weekly_now), "weekly next date")?;
        ensure(
            weekly_now_rounded < weekly_next,
            "weekly rotation advances after rounded now",
        )?;

        // never
        let never_now = OffsetDateTime::now_utc();
        let never_next = Rotation::NEVER.next_date(never_now);
        ensure(never_next.is_none(), "never rotation has no next date")
    }

    /// Builds the timestamp format used by fixed-date tests.
    fn test_datetime_format() -> Result<DateFormat, TestFailure> {
        ensure_ok(
            format_description::parse_borrowed::<1>(
                "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour \
         sign:mandatory]:[offset_minute]:[offset_second]",
            ),
            "parse test datetime format",
        )
    }

    /// Parses a fixed UTC timestamp used by rollover tests.
    fn parse_test_datetime(
        input: &str,
        format: &DateFormat,
    ) -> Result<OffsetDateTime, TestFailure> {
        ensure_ok(
            OffsetDateTime::parse(input, format),
            "parse fixed test datetime",
        )
    }

    /// Builds [`Inner`] with test-controlled configuration.
    fn build_inner(
        now: OffsetDateTime,
        rotation: Rotation,
        directory: &Path,
        prefix: Option<&str>,
        suffix: Option<&str>,
        latest_symlink_name: Option<&str>,
        max_files: Option<usize>,
    ) -> Result<(Inner, RwLock<File>), TestFailure> {
        ensure_ok(
            Inner::new(
                now,
                rotation,
                directory,
                prefix.map(ToOwned::to_owned),
                suffix.map(ToOwned::to_owned),
                latest_symlink_name.map(ToOwned::to_owned),
                max_files,
            ),
            "initialize rolling appender inner state",
        )
    }

    /// Returns the extension containing a test log file's rolling timestamp.
    fn file_date_extension(path: &Path) -> Result<&str, TestFailure> {
        let extension = ensure_some(path.extension(), "log file has date extension")?;
        ensure_some(extension.to_str(), "log file extension is UTF-8")
    }

    /// Reads all log files in `directory` with their paths.
    fn read_log_entries(directory: &Path) -> Result<Vec<(PathBuf, String)>, TestFailure> {
        let dir_contents = ensure_ok(fs::read_dir(directory), "read log directory")?;
        dir_contents
            .map(|entry| {
                let path = ensure_ok(entry, "read log directory entry")?.path();
                let file = ensure_ok(fs::read_to_string(&path), "read log file")?;
                Ok((path, file))
            })
            .collect()
    }

    /// Creates a clock callback backed by a shared test timestamp.
    fn clocked_now(
        clock: &Arc<Mutex<OffsetDateTime>>,
    ) -> Box<dyn Fn() -> OffsetDateTime + Send + Sync> {
        let clock_for_now = Arc::clone(clock);
        Box::new(move || *clock_for_now.lock())
    }

    /// Advances a shared test clock by `duration`.
    fn advance_clock(clock: &Mutex<OffsetDateTime>, duration: Duration) -> Result<(), TestFailure> {
        let current_time = *clock.lock();
        let advanced_time = ensure_some(current_time.checked_add(duration), "advance test clock")?;
        *clock.lock() = advanced_time;
        Ok(())
    }

    /// Builds a [`SystemTime`] offset from the Unix epoch by `seconds`.
    fn unix_time(seconds: u64) -> Result<SystemTime, TestFailure> {
        ensure_some(
            SystemTime::UNIX_EPOCH.checked_add(StdDuration::from_secs(seconds)),
            "build expected unix timestamp",
        )
    }

    /// Verifies date joining for each rotation kind.
    #[test]
    fn test_join_date() -> Result<(), TestFailure> {
        /// Expected filename for a joined-date test case.
        struct TestCase {
            /// Expected joined filename.
            expected: &'static str,
            /// Rotation used by this case.
            rotation: Rotation,
            /// Optional filename prefix.
            prefix: Option<&'static str>,
            /// Optional filename suffix.
            suffix: Option<&'static str>,
            /// Timestamp passed to [`Inner::join_date`].
            now: OffsetDateTime,
        }

        let format = test_datetime_format()?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;

        let test_cases = [
            TestCase {
                expected: "my_prefix.2025-02-16.log",
                rotation: Rotation::WEEKLY,
                prefix: Some("my_prefix"),
                suffix: Some("log"),
                now: parse_test_datetime("2025-02-17 10:01:00 +00:00:00", &format)?,
            },
            // Make sure weekly rotation rounds to the preceding year when appropriate
            TestCase {
                expected: "my_prefix.2024-12-29.log",
                rotation: Rotation::WEEKLY,
                prefix: Some("my_prefix"),
                suffix: Some("log"),
                now: parse_test_datetime("2025-01-01 10:01:00 +00:00:00", &format)?,
            },
            TestCase {
                expected: "my_prefix.2025-02-17.log",
                rotation: Rotation::DAILY,
                prefix: Some("my_prefix"),
                suffix: Some("log"),
                now: parse_test_datetime("2025-02-17 10:01:00 +00:00:00", &format)?,
            },
            TestCase {
                expected: "my_prefix.2025-02-17-10.log",
                rotation: Rotation::HOURLY,
                prefix: Some("my_prefix"),
                suffix: Some("log"),
                now: parse_test_datetime("2025-02-17 10:01:00 +00:00:00", &format)?,
            },
            TestCase {
                expected: "my_prefix.2025-02-17-10-01.log",
                rotation: Rotation::MINUTELY,
                prefix: Some("my_prefix"),
                suffix: Some("log"),
                now: parse_test_datetime("2025-02-17 10:01:00 +00:00:00", &format)?,
            },
            TestCase {
                expected: "my_prefix.log",
                rotation: Rotation::NEVER,
                prefix: Some("my_prefix"),
                suffix: Some("log"),
                now: parse_test_datetime("2025-02-17 10:01:00 +00:00:00", &format)?,
            },
        ];

        for test_case in test_cases {
            let (inner, _) = build_inner(
                test_case.now,
                test_case.rotation,
                directory.path(),
                test_case.prefix,
                test_case.suffix,
                None,
                None,
            )?;
            let path = ensure_ok(inner.join_date(test_case.now), "join rolling date")?;

            ensure_eq(
                &path.as_str(),
                &test_case.expected,
                "joined path matches expected rotation format",
            )?;
        }

        Ok(())
    }

    /// Verifies non-rolling rotation has no rounded rollover timestamp.
    #[test]
    fn test_never_date_rounding() -> Result<(), TestFailure> {
        let now = OffsetDateTime::now_utc();
        ensure(
            Rotation::NEVER.round_date(now).is_none(),
            "never rotation cannot be rounded",
        )
    }

    /// Expected filename for a prefix/suffix layout case.
    struct PathTestCase {
        /// Expected joined filename.
        expected: &'static str,
        /// Rotation used by this case.
        rotation: Rotation,
        /// Optional filename prefix.
        prefix: Option<&'static str>,
        /// Optional filename suffix.
        suffix: Option<&'static str>,
    }

    /// Checks path concatenation cases against a shared timestamp and directory.
    fn check_path_cases(
        now: OffsetDateTime,
        directory: &Path,
        test_cases: &[PathTestCase],
    ) -> Result<(), TestFailure> {
        for test_case in test_cases {
            let (inner, _) = build_inner(
                now,
                test_case.rotation,
                directory,
                test_case.prefix,
                test_case.suffix,
                None,
                None,
            )?;
            let path = ensure_ok(inner.join_date(now), "join rolling date")?;
            ensure(
                test_case.expected == path,
                "joined path matches expected prefix and suffix layout",
            )?;
        }

        Ok(())
    }

    /// Verifies filename layouts with only a prefix.
    #[test]
    fn test_path_concatenation_prefix_only() -> Result<(), TestFailure> {
        let format = test_datetime_format()?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let now = parse_test_datetime("2020-02-01 10:01:00 +00:00:00", &format)?;
        let test_cases = [
            PathTestCase {
                expected: "app.log.2020-02-01-10-01",
                rotation: Rotation::MINUTELY,
                prefix: Some("app.log"),
                suffix: None,
            },
            PathTestCase {
                expected: "app.log.2020-02-01-10",
                rotation: Rotation::HOURLY,
                prefix: Some("app.log"),
                suffix: None,
            },
            PathTestCase {
                expected: "app.log.2020-02-01",
                rotation: Rotation::DAILY,
                prefix: Some("app.log"),
                suffix: None,
            },
            PathTestCase {
                expected: "app.log",
                rotation: Rotation::NEVER,
                prefix: Some("app.log"),
                suffix: None,
            },
        ];

        check_path_cases(now, directory.path(), &test_cases)
    }

    /// Verifies filename layouts with a prefix and suffix.
    #[test]
    fn test_path_concatenation_prefix_and_suffix() -> Result<(), TestFailure> {
        let format = test_datetime_format()?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let now = parse_test_datetime("2020-02-01 10:01:00 +00:00:00", &format)?;
        let test_cases = [
            PathTestCase {
                expected: "app.2020-02-01-10-01.log",
                rotation: Rotation::MINUTELY,
                prefix: Some("app"),
                suffix: Some("log"),
            },
            PathTestCase {
                expected: "app.2020-02-01-10.log",
                rotation: Rotation::HOURLY,
                prefix: Some("app"),
                suffix: Some("log"),
            },
            PathTestCase {
                expected: "app.2020-02-01.log",
                rotation: Rotation::DAILY,
                prefix: Some("app"),
                suffix: Some("log"),
            },
            PathTestCase {
                expected: "app.log",
                rotation: Rotation::NEVER,
                prefix: Some("app"),
                suffix: Some("log"),
            },
        ];

        check_path_cases(now, directory.path(), &test_cases)
    }

    /// Verifies filename layouts with only a suffix.
    #[test]
    fn test_path_concatenation_suffix_only() -> Result<(), TestFailure> {
        let format = test_datetime_format()?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let now = parse_test_datetime("2020-02-01 10:01:00 +00:00:00", &format)?;
        let test_cases = [
            PathTestCase {
                expected: "2020-02-01-10-01.log",
                rotation: Rotation::MINUTELY,
                prefix: None,
                suffix: Some("log"),
            },
            PathTestCase {
                expected: "2020-02-01-10.log",
                rotation: Rotation::HOURLY,
                prefix: None,
                suffix: Some("log"),
            },
            PathTestCase {
                expected: "2020-02-01.log",
                rotation: Rotation::DAILY,
                prefix: None,
                suffix: Some("log"),
            },
            PathTestCase {
                expected: "log",
                rotation: Rotation::NEVER,
                prefix: None,
                suffix: Some("log"),
            },
        ];

        check_path_cases(now, directory.path(), &test_cases)
    }

    /// Verifies `MakeWriter` rolls files as the clock crosses an hourly boundary.
    #[test]
    fn test_make_writer() -> Result<(), TestFailure> {
        let format = test_datetime_format()?;

        let start_time = parse_test_datetime("2020-02-01 10:01:00 +00:00:00", &format)?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let (state, writer) = build_inner(
            start_time,
            Rotation::HOURLY,
            directory.path(),
            Some("test_make_writer"),
            None,
            None,
            None,
        )?;

        let clock = Arc::new(Mutex::new(start_time));
        let now_fn = clocked_now(&clock);
        let appender = RollingFileAppender {
            state,
            writer,
            now: now_fn,
        };
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_level(false)
            .with_target(false)
            .with_max_level(LevelFilter::TRACE)
            .with_writer(appender)
            .finish();
        let default = set_default(subscriber);

        tracing::info!("file 1");

        // advance time by one second
        advance_clock(&clock, Duration::SECOND)?;

        tracing::info!("file 1");

        // advance time by one hour
        advance_clock(&clock, Duration::HOUR)?;

        tracing::info!("file 2");

        // advance time by one second
        advance_clock(&clock, Duration::SECOND)?;

        tracing::info!("file 2");

        drop(default);

        for (path, file) in read_log_entries(directory.path())? {
            match file_date_extension(&path)? {
                "2020-02-01-10" => {
                    ensure_eq(
                        &"file 1\nfile 1\n",
                        &file.as_str(),
                        "first hourly log file contents",
                    )?;
                }
                "2020-02-01-11" => {
                    ensure_eq(
                        &"file 2\nfile 2\n",
                        &file.as_str(),
                        "second hourly log file contents",
                    )?;
                }
                _other => ensure(false, "unexpected log file date extension")?,
            }
        }

        Ok(())
    }

    /// Verifies retention pruning keeps the newest matching hourly log files.
    #[test]
    fn test_max_log_files() -> Result<(), TestFailure> {
        let format = test_datetime_format()?;

        let start_time = parse_test_datetime("2020-02-01 10:01:00 +00:00:00", &format)?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let (state, writer) = build_inner(
            start_time,
            Rotation::HOURLY,
            directory.path(),
            Some("test_max_log_files"),
            None,
            None,
            Some(2),
        )?;

        let clock = Arc::new(Mutex::new(start_time));
        let now_fn = clocked_now(&clock);
        let appender = RollingFileAppender {
            state,
            writer,
            now: now_fn,
        };
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_level(false)
            .with_target(false)
            .with_max_level(LevelFilter::TRACE)
            .with_writer(appender)
            .finish();
        let default = set_default(subscriber);

        tracing::info!("file 1");

        // advance time by one second
        advance_clock(&clock, Duration::SECOND)?;

        tracing::info!("file 1");

        // advance time by one hour
        advance_clock(&clock, Duration::HOUR)?;

        // depending on the filesystem, the creation timestamp's resolution may
        // be as coarse as one second, so we need to wait a bit here to ensure
        // that the next file actually is newer than the old one.
        thread::sleep(StdDuration::from_secs(1));

        tracing::info!("file 2");

        // advance time by one second
        advance_clock(&clock, Duration::SECOND)?;

        tracing::info!("file 2");

        // advance time by one hour
        advance_clock(&clock, Duration::HOUR)?;

        // again, sleep to ensure that the creation timestamps actually differ.
        thread::sleep(StdDuration::from_secs(1));

        tracing::info!("file 3");

        // advance time by one second
        advance_clock(&clock, Duration::SECOND)?;

        tracing::info!("file 3");

        drop(default);

        for (path, file) in read_log_entries(directory.path())? {
            match file_date_extension(&path)? {
                "2020-02-01-10" => {
                    ensure(false, "oldest log file should have been pruned")?;
                }
                "2020-02-01-11" => {
                    ensure_eq(
                        &"file 2\nfile 2\n",
                        &file.as_str(),
                        "retained second log file contents",
                    )?;
                }
                "2020-02-01-12" => {
                    ensure_eq(
                        &"file 3\nfile 3\n",
                        &file.as_str(),
                        "retained third log file contents",
                    )?;
                }
                _other => ensure(false, "unexpected log file date extension")?,
            }
        }

        Ok(())
    }

    /// Verifies daily filenames can be parsed back to UTC midnight.
    #[test]
    fn test_parse_date_from_filename_daily() -> Result<(), TestFailure> {
        let date_format = ensure_ok(Rotation::DAILY.date_format(), "build daily date format")?;
        let filename = "app.2020-02-01.log";
        let created = parse_date_from_filename(filename, &date_format, Some("app"), Some("log"));
        let expected = Some(unix_time(1_580_515_200)?);
        ensure(created == expected, "daily filename parses to midnight UTC")
    }

    /// Verifies hourly filenames can be parsed back to their UTC hour.
    #[test]
    fn test_parse_date_from_filename_hourly() -> Result<(), TestFailure> {
        let date_format = ensure_ok(Rotation::HOURLY.date_format(), "build hourly date format")?;
        let filename = "app.2020-02-01-10.log";
        let created = parse_date_from_filename(filename, &date_format, Some("app"), Some("log"));
        let expected = Some(unix_time(1_580_551_200)?);
        ensure(created == expected, "hourly filename parses to hour UTC")
    }

    /// Verifies minutely filenames can be parsed back to their UTC minute.
    #[test]
    fn test_parse_date_from_filename_minutely() -> Result<(), TestFailure> {
        let date_format = ensure_ok(
            Rotation::MINUTELY.date_format(),
            "build minutely date format",
        )?;
        let filename = "app.2020-02-01-10-01.log";
        let created = parse_date_from_filename(filename, &date_format, Some("app"), Some("log"));
        let expected = Some(unix_time(1_580_551_260)?);
        ensure(
            created == expected,
            "minutely filename parses to minute UTC",
        )
    }

    /// Verifies latest-log symlink creation and rollover updates.
    #[test]
    fn test_latest_symlink() -> Result<(), TestFailure> {
        let format = test_datetime_format()?;

        let now = parse_test_datetime("2020-02-01 10:01:00 +00:00:00", &format)?;
        let directory = ensure_ok(tempfile::tempdir(), "create tempdir")?;
        let (state, writer) = build_inner(
            now,
            Rotation::HOURLY,
            directory.path(),
            Some("test_latest_symlink"),
            None,
            Some("latest.log"),
            None,
        )?;

        // Verify symlink was created pointing to the initial log file
        let symlink_path = directory.path().join("latest.log");
        ensure(symlink_path.is_symlink(), "latest.log should be a symlink")?;
        let initial_target = ensure_ok(fs::read_link(&symlink_path), "read initial symlink")?;
        ensure(
            initial_target.to_string_lossy().contains("2020-02-01-10"),
            "symlink points to initial log file",
        )?;

        // Set up appender with mock clock to test rotation
        let clock = Arc::new(Mutex::new(now));
        let now_fn = clocked_now(&clock);
        let mut appender = RollingFileAppender {
            state,
            writer,
            now: now_fn,
        };

        // Advance time by one hour and write to trigger rotation
        advance_clock(&clock, Duration::HOUR)?;
        ensure_ok(appender.write_all(b"test\n"), "write rotated log entry")?;
        ensure_ok(appender.flush(), "flush rotated log entry")?;

        // Verify symlink now points to the new log file
        let rotated_target = ensure_ok(fs::read_link(&symlink_path), "read rotated symlink")?;
        ensure(
            rotated_target.to_string_lossy().contains("2020-02-01-11"),
            "symlink points to rotated log file",
        )?;

        // Verify the symlink is functional
        let content = ensure_ok(fs::read_to_string(&symlink_path), "read through symlink")?;
        ensure_eq(
            &"test\n",
            &content.as_str(),
            "symlink reads rotated log contents",
        )
    }
}
