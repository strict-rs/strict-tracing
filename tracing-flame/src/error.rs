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
        Self(Kind::CreateFile { source, path })
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

        for (index, error) in (ErrorSources { next: Some(self) }).enumerate() {
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
        path: PathBuf,
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
