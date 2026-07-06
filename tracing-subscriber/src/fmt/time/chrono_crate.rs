use crate::fmt::format::Writer;
use crate::fmt::time::FormatTime;

use alloc::{fmt::Result as FmtResult, format, string::String, sync::Arc};
use chrono::{
    format::{Fixed, Item},
    Local, Utc,
};
use core::{fmt::Write as _, iter};

/// Formats [local time]s and [UTC time]s with `FormatTime` implementations
/// that use the [`chrono` crate].
///
/// [local time]: [`chrono::offset::Local`]
/// [UTC time]: [`chrono::offset::Utc`]
/// [`chrono` crate]: [`chrono`]
///
/// Formats the current [local time] using a [formatter] from the [`chrono`] crate.
///
/// [local time]: chrono::Local::now()
/// [formatter]: chrono::format
#[cfg_attr(docsrs, doc(cfg(feature = "chrono")))]
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct ChronoLocal {
    /// The chrono formatter used for each local timestamp.
    format: Arc<ChronoFmtType>,
}

impl ChronoLocal {
    /// Format the time using the [`RFC 3339`] format
    /// (a subset of [`ISO 8601`]).
    ///
    /// [`RFC 3339`]: https://tools.ietf.org/html/rfc3339
    /// [`ISO 8601`]: https://en.wikipedia.org/wiki/ISO_8601
    #[must_use]
    pub fn rfc_3339() -> Self {
        Self {
            format: Arc::new(ChronoFmtType::Rfc3339),
        }
    }

    /// Format the time using the given format string.
    ///
    /// See [`chrono::format::strftime`] for details on the supported syntax.
    #[must_use]
    pub fn new(format_string: String) -> Self {
        Self {
            format: Arc::new(ChronoFmtType::Custom(format_string)),
        }
    }
}

impl FormatTime for ChronoLocal {
    fn format_time(&self, writer: &mut Writer<'_>) -> FmtResult {
        let timestamp = Local::now();
        match *self.format.as_ref() {
            ChronoFmtType::Rfc3339 => {
                let rfc3339 = iter::once(Item::Fixed(Fixed::RFC3339));
                write!(writer, "{}", timestamp.format_with_items(rfc3339))
            }
            ChronoFmtType::Custom(ref format_string) => {
                write!(writer, "{}", timestamp.format(format_string))
            }
        }
    }
}

/// Formats the current [UTC time] using a [formatter] from the [`chrono`] crate.
///
/// [UTC time]: chrono::Utc::now()
/// [formatter]: chrono::format
#[cfg_attr(docsrs, doc(cfg(feature = "chrono")))]
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct ChronoUtc {
    /// The chrono formatter used for each UTC timestamp.
    format: Arc<ChronoFmtType>,
}

impl ChronoUtc {
    /// Format the time using the [`RFC 3339`] format
    /// (a subset of [`ISO 8601`]).
    ///
    /// [`RFC 3339`]: https://tools.ietf.org/html/rfc3339
    /// [`ISO 8601`]: https://en.wikipedia.org/wiki/ISO_8601
    #[must_use]
    pub fn rfc_3339() -> Self {
        Self {
            format: Arc::new(ChronoFmtType::Rfc3339),
        }
    }

    /// Format the time using the given format string.
    ///
    /// See [`chrono::format::strftime`] for details on the supported syntax.
    #[must_use]
    pub fn new(format_string: String) -> Self {
        Self {
            format: Arc::new(ChronoFmtType::Custom(format_string)),
        }
    }
}

impl FormatTime for ChronoUtc {
    fn format_time(&self, writer: &mut Writer<'_>) -> FmtResult {
        let timestamp = Utc::now();
        match *self.format.as_ref() {
            ChronoFmtType::Rfc3339 => writer.write_str(&timestamp.to_rfc3339()),
            ChronoFmtType::Custom(ref format_string) => {
                writer.write_str(&format!("{}", timestamp.format(format_string)))
            }
        }
    }
}

/// The RFC 3339 format is used by default but a custom format string
/// can be used. See [`chrono::format::strftime`]for details on
/// the supported syntax.
///
/// [`chrono::format::strftime`]: https://docs.rs/chrono/0.4.9/chrono/format/strftime/index.html
#[derive(Debug, Clone, Eq, PartialEq, Default)]
enum ChronoFmtType {
    /// Format according to the RFC 3339 convention.
    #[default]
    Rfc3339,
    /// Format according to a custom format string.
    Custom(String),
}

#[cfg(test)]
mod tests {
    use crate::fmt::format::Writer;
    use crate::fmt::time::FormatTime;

    use alloc::{borrow::ToOwned as _, string::String, sync::Arc};
    use strict_test_support::{TestFailure, ensure, ensure_ok};

    use super::ChronoFmtType;
    use super::ChronoLocal;
    use super::ChronoUtc;

    /// Formats time with `timer` and checks the default output is a valid RFC 3339 timestamp.
    fn ensure_default_rfc3339<T: FormatTime>(
        timer: &T,
        write_message: &'static str,
        rfc_message: &'static str,
    ) -> Result<(), TestFailure> {
        let mut buf = String::new();
        let mut dst: Writer<'_> = Writer::new(&mut buf);
        // e.g. `buf` contains "2023-08-18T19:05:08.662499+00:00"
        ensure_ok(timer.format_time(&mut dst), write_message)?;
        ensure(chrono::DateTime::parse_from_rfc3339(&buf).is_ok(), rfc_message)
    }

    /// Formats time with `timer` and checks the output parses under the custom format string.
    fn ensure_custom_format<T: FormatTime>(
        timer: &T,
        write_message: &'static str,
        format_message: &'static str,
    ) -> Result<(), TestFailure> {
        let mut buf = String::new();
        let mut dst: Writer<'_> = Writer::new(&mut buf);
        // e.g. `buf` contains "Wed Aug 23 15:53:23 2023"
        ensure_ok(timer.format_time(&mut dst), write_message)?;
        ensure(
            chrono::NaiveDateTime::parse_from_str(&buf, "%a %b %e %T %Y").is_ok(),
            format_message,
        )
    }

    #[test]
    fn test_chrono_format_time_utc_default() -> Result<(), TestFailure> {
        ensure_default_rfc3339(
            &ChronoUtc::default(),
            "default UTC chrono formatter writes time",
            "default UTC chrono formatter emits RFC3339",
        )
    }

    #[test]
    fn test_chrono_format_time_utc_custom() -> Result<(), TestFailure> {
        let fmt = ChronoUtc {
            format: Arc::new(ChronoFmtType::Custom("%a %b %e %T %Y".to_owned())),
        };
        ensure_custom_format(
            &fmt,
            "custom UTC chrono formatter writes time",
            "custom UTC chrono formatter emits configured format",
        )
    }

    #[test]
    fn test_chrono_format_time_local_default() -> Result<(), TestFailure> {
        ensure_default_rfc3339(
            &ChronoLocal::default(),
            "default local chrono formatter writes time",
            "default local chrono formatter emits RFC3339",
        )
    }

    #[test]
    fn test_chrono_format_time_local_custom() -> Result<(), TestFailure> {
        let fmt = ChronoLocal {
            format: Arc::new(ChronoFmtType::Custom("%a %b %e %T %Y".to_owned())),
        };
        ensure_custom_format(
            &fmt,
            "custom local chrono formatter writes time",
            "custom local chrono formatter emits configured format",
        )
    }
}
