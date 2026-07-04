use std::io;
use std::path::Path;

use thiserror::Error;

use super::RollingFileAppender;
use super::Rotation;

/// A [builder] for configuring [`RollingFileAppender`]s.
///
/// [builder]: https://rust-unofficial.github.io/patterns/patterns/creational/builder.html
#[derive(Debug)]
pub struct Builder {
  /// Rotation strategy used to choose log filenames and rollover times.
  pub(super) rotation:       Rotation,
  /// Optional non-empty filename prefix written before the timestamp.
  pub(super) prefix:         Option<String>,
  /// Optional non-empty filename suffix written after the timestamp.
  pub(super) suffix:         Option<String>,
  /// Optional non-empty symlink filename that points at the latest log file.
  pub(super) latest_symlink: Option<String>,
  /// Optional maximum number of matching log files to retain.
  pub(super) max_files:      Option<usize>,
}

/// Errors returned by [`Builder::build`].
#[derive(Error, Debug)]
#[error("{context}: {source}")]
pub struct InitError {
  /// Static description of the operation that failed.
  context: &'static str,
  /// Underlying I/O error returned while initializing the appender.
  #[source]
  source:  io::Error,
}

impl InitError {
  /// Returns an error adapter that attaches `context` to an I/O error.
  pub(crate) fn ctx(context: &'static str) -> impl FnOnce(io::Error) -> Self {
    move |source| Self {
      context,
      source,
    }
  }
}

impl Builder {
  /// Returns a new `Builder` for configuring a [`RollingFileAppender`], with
  /// the default parameters.
  ///
  /// # Default Values
  ///
  /// The default values for the builder are:
  ///
  /// | Parameter | Default Value | Notes |
  /// | :-------- | :------------ | :---- |
  /// | [`rotation`] | [`Rotation::NEVER`] | By default, log files will never be rotated. |
  /// | [`filename_prefix`] | `""` | By default, log file names will not have a prefix. |
  /// | [`filename_suffix`] | `""` | By default, log file names will not have a suffix. |
  /// | [`max_log_files`] | `None` | By default, there is no limit for maximum log file count. |
  ///
  /// [`rotation`]: Self::rotation
  /// [`filename_prefix`]: Self::filename_prefix
  /// [`filename_suffix`]: Self::filename_suffix
  /// [`max_log_files`]: Self::max_log_files
  #[must_use]
  pub const fn new() -> Self {
    Self {
      rotation:       Rotation::NEVER,
      prefix:         None,
      suffix:         None,
      latest_symlink: None,
      max_files:      None,
    }
  }

  /// Sets the [rotation strategy] for log files.
  ///
  /// By default, this is [`Rotation::NEVER`].
  ///
  /// # Examples
  ///
  /// ```
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// use tracing_appender::rolling::RollingFileAppender;
  /// use tracing_appender::rolling::Rotation;
  ///
  /// let appender = RollingFileAppender::builder()
  ///     .rotation(Rotation::HOURLY) // rotate log files once every hour
  ///     // ...
  ///     .build("/var/log")?;
  ///
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [rotation strategy]: Rotation
  #[must_use]
  pub fn rotation(self, rotation: Rotation) -> Self {
    Self {
      rotation,
      ..self
    }
  }

  /// Sets the prefix for log filenames. The prefix is output before the
  /// timestamp in the file name, and if it is non-empty, it is followed by a
  /// dot (`.`).
  ///
  /// By default, log files do not have a prefix.
  ///
  /// # Examples
  ///
  /// Setting a prefix:
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .filename_prefix("myapp.log") // log files will have names like "myapp.log.2019-01-01"
  ///     // ...
  ///     .build("/var/log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// No prefix:
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .filename_prefix("") // log files will have names like "2019-01-01"
  ///     // ...
  ///     .build("/var/log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [rotation strategy]: Rotation
  #[must_use]
  pub fn filename_prefix(self, filename_prefix: impl Into<String>) -> Self {
    let configured_prefix = filename_prefix.into();
    // If the configured prefix is the empty string, then don't include a
    // separator character.
    let prefix = if configured_prefix.is_empty() {
      None
    } else {
      Some(configured_prefix)
    };
    Self {
      prefix,
      ..self
    }
  }

  /// Sets the suffix for log filenames. The suffix is output after the
  /// timestamp in the file name, and if it is non-empty, it is preceded by a
  /// dot (`.`).
  ///
  /// By default, log files do not have a suffix.
  ///
  /// # Examples
  ///
  /// Setting a suffix:
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .filename_suffix("myapp.log") // log files will have names like "2019-01-01.myapp.log"
  ///     // ...
  ///     .build("/var/log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// No suffix:
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .filename_suffix("") // log files will have names like "2019-01-01"
  ///     // ...
  ///     .build("/var/log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// [rotation strategy]: Rotation
  #[must_use]
  pub fn filename_suffix(self, filename_suffix: impl Into<String>) -> Self {
    let configured_suffix = filename_suffix.into();
    // If the configured suffix is the empty string, then don't include a
    // separator character.
    let suffix = if configured_suffix.is_empty() {
      None
    } else {
      Some(configured_suffix)
    };
    Self {
      suffix,
      ..self
    }
  }

  /// Keeps the last `n` log files on disk.
  ///
  /// When constructing a [`RollingFileAppender`] or starting a new log file,
  /// the appender will delete the oldest matching log files until at most `n`
  /// files remain. The exact number of retained files can sometimes dip below
  /// the maximum, so if you need to retain `m` log files, specify a max of
  /// `m + 1`.
  ///
  /// If `0` is supplied, the [`RollingFileAppender`] will not remove any files.
  ///
  /// Files are considered candidates for deletion based on the following
  /// criteria:
  ///
  /// * The file must not be a directory or symbolic link.
  /// * If the appender is configured with a [`filename_prefix`], the file name must start with that
  ///   prefix.
  /// * If the appender is configured with a [`filename_suffix`], the file name must end with that
  ///   suffix.
  /// * If the appender has neither a filename prefix nor a suffix, then the file name must parse as
  ///   a valid date based on the appender's date format.
  ///
  /// Files matching these criteria may be deleted if the maximum number of
  /// log files in the directory has been reached.
  ///
  /// [`filename_prefix`]: Self::filename_prefix
  /// [`filename_suffix`]: Self::filename_suffix
  ///
  /// # Examples
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .max_log_files(5) // only the most recent 5 log files will be kept
  ///     // ...
  ///     .build("/var/log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn max_log_files(self, max_files: usize) -> Self {
    Self {
      // Setting `max_files` to 0 will disable the max files (effectively make it infinite).
      max_files: (max_files > 0).then_some(max_files),
      ..self
    }
  }

  /// Create a symbolic link that points to the latest log file.
  /// The symbolic link will be updated when new log files are created.
  ///
  /// # Examples
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .latest_symlink("log.latest")
  ///     // ...
  ///     .build("/var/log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  #[must_use]
  pub fn latest_symlink(self, name: impl Into<String>) -> Self {
    let symlink_name = name.into();
    let latest_symlink = if symlink_name.is_empty() {
      None
    } else {
      Some(symlink_name)
    };
    Self {
      latest_symlink,
      ..self
    }
  }

  /// Builds a new [`RollingFileAppender`] with the configured parameters,
  /// emitting log files to the provided directory.
  ///
  /// Like [`RollingFileAppender::new`], this returns a `Result` when the
  /// appender cannot be initialized.
  ///
  /// # Errors
  ///
  /// Returns an [`InitError`] if initializing the rolling appender fails, such
  /// as when directory, file, retention cleanup, or latest-symlink I/O cannot
  /// be completed.
  ///
  /// # Examples
  ///
  /// ```
  /// use tracing_appender::rolling::RollingFileAppender;
  /// use tracing_appender::rolling::Rotation;
  ///
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = RollingFileAppender::builder()
  ///     .rotation(Rotation::DAILY) // rotate log files once per day
  ///     .filename_prefix("myapp.log") // log files will have names like "myapp.log.2019-01-01"
  ///     .build("/var/log/myapp") // write log files to the '/var/log/myapp' directory
  ///     ?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  ///
  /// This is equivalent to
  /// ```
  /// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
  /// let appender = tracing_appender::rolling::daily("/var/log/myapp", "myapp.log")?;
  /// # drop(appender);
  /// # Ok(())
  /// # }
  /// ```
  pub fn build(&self, directory: impl AsRef<Path>) -> Result<RollingFileAppender, InitError> {
    RollingFileAppender::from_builder(self, directory)
  }
}

impl Default for Builder {
  fn default() -> Self {
    Self::new()
  }
}
