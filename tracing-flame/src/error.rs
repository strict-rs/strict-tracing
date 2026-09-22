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

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// Retains the owning error when its source chain violates the contract.
    #[error("{context}: {error:?}")]
    MissingSource {
      /// Source-chain expectation that failed.
      context: &'static str,
      /// Complete owning error and its available sources.
      error:   FlameError,
    },
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonString(#[from] strict_test_support::ComparisonFailure<String, String>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ComparisonErrorKind(#[from] strict_test_support::ComparisonFailure<io::ErrorKind, io::ErrorKind>),
    /// Retains the searched text and expected substring.
    #[error(transparent)]
    Substring(#[from] strict_test_support::SubstringFailure<String, String>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_contains;
  use strict_test_support::ensure_eq;

  use super::ErrorSources;
  use super::FlameError;

  #[test]
  fn create_file_error_formats_path_and_preserves_source_chain() -> Result<(), TestError> {
    let error = FlameError::create_file(
      PathBuf::from("missing/tracing.folded"),
      io::Error::from(io::ErrorKind::PermissionDenied),
    );

    let displayed = error.to_string();
    ensure_contains(
      (displayed).clone(),
      String::from("cannot create output file"),
      "create-file display names operation",
    )
    .map(drop)?;
    ensure_contains(
      (displayed).clone(),
      String::from("missing/tracing.folded"),
      "create-file display includes path",
    )
    .map(drop)?;
    ensure_contains(
      format!("{error:?}"),
      String::from("CreateFile"),
      "create-file debug names the variant",
    )
    .map(drop)?;

    let Some(source) = error.source() else {
      return Err(TestError::MissingSource {
        context: "create-file error preserves source",
        error,
      });
    };
    let Some(io_source) = source.downcast_ref::<io::Error>() else {
      return Err(TestError::MissingSource {
        context: "create-file source remains an I/O error",
        error,
      });
    };
    ensure_eq(
      io_source.kind(),
      io::ErrorKind::PermissionDenied,
      "create-file source kind is preserved",
    )
    .map(drop)?;

    let mut sources = ErrorSources {
      next: Some(&error)
    };
    let Some(first) = sources.next() else {
      return Err(TestError::MissingSource {
        context: "source iterator starts with the outer error",
        error,
      });
    };
    let Some(second) = sources.next() else {
      return Err(TestError::MissingSource {
        context: "source iterator follows to the I/O error",
        error,
      });
    };
    let Some(second_source) = second.downcast_ref::<io::Error>() else {
      return Err(TestError::MissingSource {
        context: "second source item remains an I/O error",
        error,
      });
    };

    ensure_eq(first.to_string(), displayed, "source iterator yields the outer error first").map(drop)?;
    ensure_eq(
      second_source.kind(),
      io::ErrorKind::PermissionDenied,
      "source iterator yields the I/O source",
    )
    .map(drop)?;
    ensure(sources.next().is_none(), "source iterator ends after source chain")
      .map(drop)
      .map_err(TestError::from)
  }

  #[test]
  fn flush_error_formats_operation_and_preserves_source_chain() -> Result<(), TestError> {
    let error = FlameError::flush_file(io::Error::from(io::ErrorKind::BrokenPipe));

    ensure_eq(
      error.to_string(),
      "cannot flush output buffer".to_owned(),
      "flush display names operation",
    )
    .map(drop)?;
    ensure_contains(format!("{error:?}"), String::from("FlushFile"), "flush debug names the variant").map(drop)?;

    let Some(source) = error.source() else {
      return Err(TestError::MissingSource {
        context: "flush error preserves source",
        error,
      });
    };
    let Some(io_source) = source.downcast_ref::<io::Error>() else {
      return Err(TestError::MissingSource {
        context: "flush source remains an I/O error",
        error,
      });
    };
    ensure_eq(io_source.kind(), io::ErrorKind::BrokenPipe, "flush source kind is preserved").map(drop)?;

    let mut sources = ErrorSources {
      next: Some(&error)
    };
    let Some(first) = sources.next() else {
      return Err(TestError::MissingSource {
        context: "source iterator starts with flush error",
        error,
      });
    };
    let Some(second) = sources.next() else {
      return Err(TestError::MissingSource {
        context: "source iterator follows to flush source",
        error,
      });
    };
    let Some(second_source) = second.downcast_ref::<io::Error>() else {
      return Err(TestError::MissingSource {
        context: "second source item remains an I/O error",
        error,
      });
    };

    ensure_eq(
      first.to_string(),
      "cannot flush output buffer".to_owned(),
      "flush error is first source item",
    )
    .map(drop)?;
    ensure_eq(
      second_source.kind(),
      io::ErrorKind::BrokenPipe,
      "flush source is second source item",
    )
    .map(drop)?;
    ensure(sources.next().is_none(), "flush source iterator ends after source chain")
      .map(drop)
      .map_err(TestError::from)
  }
}
