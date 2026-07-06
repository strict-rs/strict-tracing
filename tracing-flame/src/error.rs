use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::io::Write as _;
use std::path::PathBuf;

/// The error type for `tracing-flame`
#[derive(Debug)]
pub struct FlameError(Kind);

impl FlameError {
  /// Creates an error for a failed output-file creation.
  #[allow(
    clippy::single_call_fn,
    reason = "opaque error constructor keeps Kind private while with_file maps file creation failures"
  )]
  pub(super) const fn create_file(path: PathBuf, source: io::Error) -> Self {
    Self(Kind::CreateFile {
      source,
      path,
    })
  }

  /// Creates an error for a failed writer flush.
  #[allow(
    clippy::single_call_fn,
    reason = "opaque error constructor keeps Kind private while FlushGuard maps flush failures"
  )]
  pub(super) const fn flush_file(source: io::Error) -> Self {
    Self(Kind::FlushFile(source))
  }

  /// Reports this error and its source chain to stderr.
  pub(super) fn report(&self) {
    let mut stderr = io::stderr();
    let _header_write_result: io::Result<()> = writeln!(&mut stderr, "Error:");

    for (index, error) in (ErrorSources {
      next: Some(self)
    })
    .enumerate()
    {
      let _chain_write_result: io::Result<()> = writeln!(&mut stderr, "    {index}: {error}");
    }
  }
}

impl fmt::Display for FlameError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(&self.0, f)
  }
}

impl StdError for FlameError {
  fn source(&self) -> Option<&(dyn StdError + 'static)> {
    Some(self.0.source())
  }
}

/// Iterator over an error and each source error in its chain.
struct ErrorSources<'a> {
  /// The next error to return from the chain.
  next: Option<&'a (dyn StdError + 'static)>,
}

impl<'a> Iterator for ErrorSources<'a> {
  type Item = &'a (dyn StdError + 'static);

  fn next(&mut self) -> Option<Self::Item> {
    let current_error = self.next?;
    self.next = current_error.source();
    Some(current_error)
  }
}

/// Internal categories for fallible `tracing-flame` operations.
#[derive(Debug)]
enum Kind {
  /// Creating the output file failed.
  CreateFile {
    /// The underlying file-system error.
    source: io::Error,
    /// The requested output path.
    path:   PathBuf,
  },

  /// Flushing the output writer failed.
  FlushFile(
    /// The underlying writer error.
    io::Error,
  ),
}

impl Kind {
  /// Returns the underlying source error.
  fn source(&self) -> &(dyn StdError + 'static) {
    match *self {
      Self::CreateFile {
        ref source,
        path: _,
      }
      | Self::FlushFile(ref source) => source,
    }
  }
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *self {
      Self::CreateFile {
        ref path,
        source: _,
      } => {
        let display_path = path.display();
        write!(f, "cannot create output file. path={display_path}")
      }
      Self::FlushFile(_) => write!(f, "cannot flush output buffer"),
    }
  }
}

#[cfg(test)]
mod tests {
  use std::error::Error as _;
  use std::io;
  use std::path::PathBuf;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_some;

  use super::ErrorSources;
  use super::FlameError;

  #[test]
  fn create_file_error_formats_path_and_preserves_source_chain() -> Result<(), TestFailure> {
    let error = FlameError::create_file(
      PathBuf::from("missing/tracing.folded"),
      io::Error::from(io::ErrorKind::PermissionDenied),
    );

    let displayed = error.to_string();
    ensure_contains(&displayed, "cannot create output file", "create-file display names operation")?;
    ensure_contains(&displayed, "missing/tracing.folded", "create-file display includes path")?;
    ensure_contains(&format!("{error:?}"), "CreateFile", "create-file debug names the variant")?;

    let source = ensure_some(error.source(), "create-file error preserves source")?;
    let io_source = ensure_some(source.downcast_ref::<io::Error>(), "create-file source remains an I/O error")?;
    ensure_eq(
      &io_source.kind(),
      &io::ErrorKind::PermissionDenied,
      "create-file source kind is preserved",
    )?;

    let mut sources = ErrorSources {
      next: Some(&error)
    };
    let first = ensure_some(sources.next(), "source iterator starts with the outer error")?;
    let second = ensure_some(sources.next(), "source iterator follows to the I/O error")?;
    let second_source = ensure_some(second.downcast_ref::<io::Error>(), "second source item remains an I/O error")?;

    ensure_eq(&first.to_string(), &displayed, "source iterator yields the outer error first")?;
    ensure_eq(
      &second_source.kind(),
      &io::ErrorKind::PermissionDenied,
      "source iterator yields the I/O source",
    )?;
    ensure(sources.next().is_none(), "source iterator ends after source chain")
  }

  #[test]
  fn flush_error_formats_operation_and_preserves_source_chain() -> Result<(), TestFailure> {
    let error = FlameError::flush_file(io::Error::from(io::ErrorKind::BrokenPipe));

    ensure_eq(
      &error.to_string(),
      &"cannot flush output buffer".to_owned(),
      "flush display names operation",
    )?;
    ensure_contains(&format!("{error:?}"), "FlushFile", "flush debug names the variant")?;

    let source = ensure_some(error.source(), "flush error preserves source")?;
    let io_source = ensure_some(source.downcast_ref::<io::Error>(), "flush source remains an I/O error")?;
    ensure_eq(&io_source.kind(), &io::ErrorKind::BrokenPipe, "flush source kind is preserved")?;

    let mut sources = ErrorSources {
      next: Some(&error)
    };
    let first = ensure_some(sources.next(), "source iterator starts with flush error")?;
    let second = ensure_some(sources.next(), "source iterator follows to flush source")?;
    let second_source = ensure_some(second.downcast_ref::<io::Error>(), "second source item remains an I/O error")?;

    ensure_eq(
      &first.to_string(),
      &"cannot flush output buffer".to_owned(),
      "flush error is first source item",
    )?;
    ensure_eq(
      &second_source.kind(),
      &io::ErrorKind::BrokenPipe,
      "flush source is second source item",
    )?;
    ensure(sources.next().is_none(), "flush source iterator ends after source chain")
  }
}
