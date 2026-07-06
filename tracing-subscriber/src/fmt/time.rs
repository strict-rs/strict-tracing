//! Formatters for event timestamps.
use crate::fmt::format::Writer;
use std::fmt::{self, Write as _};
use std::time::Instant;

/// UTC timestamp conversion for the default `SystemTime` timer.
mod datetime;

#[cfg(feature = "time")]
/// [`time`] crate backed timestamp formatters.
mod time_crate;

#[cfg(feature = "time")]
#[cfg_attr(docsrs, doc(cfg(feature = "time")))]
pub use time_crate::UtcTime;

#[cfg(feature = "local-time")]
#[cfg_attr(docsrs, doc(cfg(all(unsound_local_offset, feature = "local-time"))))]
pub use time_crate::LocalTime;

#[cfg(feature = "time")]
#[cfg_attr(docsrs, doc(cfg(feature = "time")))]
pub use time_crate::OffsetTime;

/// [`chrono`]-based implementation for [`FormatTime`].
#[cfg(feature = "chrono")]
mod chrono_crate;

#[cfg(feature = "chrono")]
#[cfg_attr(docsrs, doc(cfg(feature = "chrono")))]
pub use chrono_crate::ChronoLocal;

#[cfg(feature = "chrono")]
#[cfg_attr(docsrs, doc(cfg(feature = "chrono")))]
pub use chrono_crate::ChronoUtc;

/// A type that can measure and format the current time.
///
/// This trait is used by `Format` to include a timestamp with each `Event` when it is logged.
///
/// Notable default implementations of this trait are `SystemTime` and `()`. The former prints the
/// current time as reported by `std::time::SystemTime`, and the latter does not print the current
/// time at all. `FormatTime` is also automatically implemented for any function pointer with the
/// appropriate signature.
///
/// The full list of provided implementations can be found in [`time`].
///
/// [`time`]: self
pub trait FormatTime {
    /// Measure and write out the current time.
    ///
    /// When `format_time` is called, implementors should get the current time using their desired
    /// mechanism, and write it out to the given `fmt::Write`. Implementors must insert a trailing
    /// space themselves if they wish to separate the time from subsequent log message text.
    ///
    /// # Errors
    ///
    /// Returns [`fmt::Error`] if the timestamp cannot be written to the formatter.
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result;
}

/// Returns a new `SystemTime` timestamp provider.
///
/// This can then be configured further to determine how timestamps should be
/// configured.
///
/// This is equivalent to calling
/// ```rust
/// # fn timer() -> tracing_subscriber::fmt::time::SystemTime {
/// tracing_subscriber::fmt::time::SystemTime::default()
/// # }
/// ```
#[must_use]
pub const fn time() -> SystemTime {
    SystemTime
}

/// Returns a new `Uptime` timestamp provider.
///
/// With this timer, timestamps will be formatted with the amount of time
/// elapsed since the timestamp provider was constructed.
///
/// This can then be configured further to determine how timestamps should be
/// configured.
///
/// This is equivalent to calling
/// ```rust
/// # fn timer() -> tracing_subscriber::fmt::time::Uptime {
/// tracing_subscriber::fmt::time::Uptime::default()
/// # }
/// ```
#[must_use]
pub fn uptime() -> Uptime {
    Uptime::default()
}

impl<F> FormatTime for &F
where
    F: FormatTime,
{
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        (*self).format_time(writer)
    }
}

impl FormatTime for () {
    fn format_time(&self, _writer: &mut Writer<'_>) -> fmt::Result {
        Ok(())
    }
}

impl FormatTime for fn(&mut Writer<'_>) -> fmt::Result {
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        (*self)(writer)
    }
}

/// Retrieve and print the current wall-clock time.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct SystemTime;

/// Retrieve and print the relative elapsed wall-clock time since an epoch.
///
/// The `Default` implementation for `Uptime` makes the epoch the current time.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Uptime {
    /// The instant from which elapsed uptime is measured.
    epoch: Instant,
}

impl Default for Uptime {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl From<Instant> for Uptime {
    fn from(epoch: Instant) -> Self {
        Self { epoch }
    }
}

impl FormatTime for SystemTime {
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        write!(writer, "{}", datetime::DateTime::from(chrono::Utc::now()))
    }
}

impl FormatTime for Uptime {
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        let elapsed = self.epoch.elapsed();
        write!(
            writer,
            "{:4}.{:09}s",
            elapsed.as_secs(),
            elapsed.subsec_nanos()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::string::String;
    use std::time::Duration;

    use strict_test_support::TestFailure;
    use strict_test_support::ensure;
    use strict_test_support::ensure_ok;
    use strict_test_support::ensure_some;

    use super::*;

    #[test]
    fn unit_timer_writes_nothing() -> Result<(), TestFailure> {
        let mut output = String::new();
        let mut writer = Writer::new(&mut output);

        ensure_ok(().format_time(&mut writer), "unit timer formats successfully")?;
        ensure(output.is_empty(), "unit timer leaves writer untouched")
    }

    #[test]
    fn function_pointer_timer_writes_and_propagates_errors() -> Result<(), TestFailure> {
        fn write_fixed_time(writer: &mut Writer<'_>) -> fmt::Result {
            writer.write_str("fixed-time")
        }

        fn fail_time(_writer: &mut Writer<'_>) -> fmt::Result {
            Err(fmt::Error)
        }

        let mut output = String::new();
        let mut writer = Writer::new(&mut output);
        let timer: fn(&mut Writer<'_>) -> fmt::Result = write_fixed_time;
        ensure_ok(timer.format_time(&mut writer), "function pointer timer writes")?;
        ensure(output == "fixed-time", "function pointer timer output is preserved")?;

        let mut failing_output = String::new();
        let mut failing_writer = Writer::new(&mut failing_output);
        let failing_timer: fn(&mut Writer<'_>) -> fmt::Result = fail_time;
        ensure(
            failing_timer.format_time(&mut failing_writer).is_err(),
            "function pointer timer propagates formatting errors",
        )
    }

    #[test]
    fn borrowed_timer_forwards_to_inner_timer() -> Result<(), TestFailure> {
        fn write_borrowed_time(writer: &mut Writer<'_>) -> fmt::Result {
            writer.write_str("borrowed-time")
        }

        let timer: fn(&mut Writer<'_>) -> fmt::Result = write_borrowed_time;
        let borrowed_timer = &timer;
        let mut output = String::new();
        let mut writer = Writer::new(&mut output);

        ensure_ok(borrowed_timer.format_time(&mut writer), "borrowed timer formats")?;
        ensure(output == "borrowed-time", "borrowed timer delegates to inner timer")
    }

    #[test]
    fn system_time_writes_wall_clock_timestamp() -> Result<(), TestFailure> {
        let mut output = String::new();
        let mut writer = Writer::new(&mut output);

        ensure_ok(time().format_time(&mut writer), "system time formats")?;
        ensure(!output.is_empty(), "system time writes timestamp")?;
        ensure(output.contains('Z'), "system time timestamp is UTC formatted")
    }

    #[test]
    fn time_constructor_matches_default_system_time() -> Result<(), TestFailure> {
        ensure(time() == SystemTime, "time constructor returns system-time formatter")
    }

    #[test]
    fn uptime_writes_elapsed_duration_shape() -> Result<(), TestFailure> {
        let epoch = ensure_some(Instant::now().checked_sub(Duration::from_secs(1)), "build uptime epoch")?;
        let mut output = String::new();
        let mut writer = Writer::new(&mut output);

        ensure_ok(Uptime::from(epoch).format_time(&mut writer), "uptime formats elapsed duration")?;
        ensure(output.contains('.'), "uptime output contains fractional seconds")?;
        ensure(output.ends_with('s'), "uptime output ends with seconds suffix")
    }

    #[test]
    fn uptime_constructor_uses_current_instant() -> Result<(), TestFailure> {
        let first_timer = uptime();
        let second_timer = uptime();
        let mut first_output = String::new();
        let mut first_writer = Writer::new(&mut first_output);
        let mut second_output = String::new();
        let mut second_writer = Writer::new(&mut second_output);

        ensure_ok(first_timer.format_time(&mut first_writer), "first uptime constructor formats")?;
        ensure_ok(second_timer.format_time(&mut second_writer), "second uptime constructor formats")?;
        ensure(first_output.ends_with('s'), "first constructed uptime output ends with seconds suffix")?;
        ensure(second_output.ends_with('s'), "second constructed uptime output ends with seconds suffix")
    }
}
