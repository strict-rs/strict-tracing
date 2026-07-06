use crate::fmt::{format::Writer, time::FormatTime, writer::WriteAdaptor};
use std::fmt;
use time::{
    error::IndeterminateOffset, format_description::well_known, formatting::Formattable,
    OffsetDateTime, UtcOffset,
};

/// Formats the current [local time] using a [formatter] from the [`time` crate].
///
/// To format the current [UTC time] instead, use the [`UtcTime`] type.
///
/// <div class="example-wrap" style="display:inline-block">
/// <pre class="compile_fail" style="white-space:normal;font:inherit;">
///     <strong>Warning</strong>: The <a href = "https://docs.rs/time/0.3/time/"><code>time</code>
///     crate</a> must be compiled with <code>--cfg unsound_local_offset</code> in order to use
///     local timestamps. When this cfg is not enabled, local timestamps cannot be recorded, and
///     events will be logged without timestamps.
///
///    Alternatively, [`OffsetTime`] can log with a local offset if it is initialized early.
///
///    See the <a href="https://docs.rs/time/0.3.4/time/#feature-flags"><code>time</code>
///    documentation</a> for more details.
/// </pre></div>
///
/// [local time]: time::OffsetDateTime::now_local
/// [UTC time]:     time::OffsetDateTime::now_utc
/// [formatter]:    time::formatting::Formattable
/// [`time` crate]: time
#[derive(Clone, Debug)]
#[cfg_attr(
    docsrs,
    doc(cfg(all(unsound_local_offset, feature = "time", feature = "local-time")))
)]
#[cfg(feature = "local-time")]
pub struct LocalTime<F> {
    /// The formatter used for each local timestamp.
    format: F,
}

/// Formats the current [UTC time] using a [formatter] from the [`time` crate].
///
/// To format the current [local time] instead, use the [`LocalTime`] type.
///
/// [local time]: time::OffsetDateTime::now_local
/// [UTC time]:     time::OffsetDateTime::now_utc
/// [formatter]:    time::formatting::Formattable
/// [`time` crate]: time
#[cfg_attr(docsrs, doc(cfg(feature = "time")))]
#[derive(Clone, Debug)]
pub struct UtcTime<F> {
    /// The formatter used for each UTC timestamp.
    format: F,
}

/// Formats the current time using a fixed offset and a [formatter] from the [`time` crate].
///
/// This is typically used as an alternative to [`LocalTime`]. `LocalTime` determines the offset
/// every time it formats a message, which may be unsound or fail. With `OffsetTime`, the offset is
/// determined once. This makes it possible to do so while the program is still single-threaded and
/// handle any errors. However, this also means the offset cannot change while the program is
/// running (the offset will not change across DST changes).
///
/// [formatter]: time::formatting::Formattable
/// [`time` crate]: time
#[derive(Clone, Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "time")))]
pub struct OffsetTime<F> {
    /// The fixed timezone offset applied before formatting.
    offset: UtcOffset,
    /// The formatter used for each offset timestamp.
    format: F,
}

// === impl LocalTime ===

#[cfg(feature = "local-time")]
impl LocalTime<well_known::Rfc3339> {
    /// Returns a formatter that formats the current [local time] in the
    /// [RFC 3339] format (a subset of the [ISO 8601] timestamp format).
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time};
    ///
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(time::LocalTime::rfc_3339());
    /// # drop(subscriber);
    /// ```
    ///
    /// [local time]: time::OffsetDateTime::now_local
    /// [RFC 3339]: https://datatracker.ietf.org/doc/html/rfc3339
    /// [ISO 8601]: https://en.wikipedia.org/wiki/ISO_8601
    #[must_use]
    pub const fn rfc_3339() -> Self {
        Self::new(well_known::Rfc3339)
    }
}

#[cfg(feature = "local-time")]
impl<F: Formattable> LocalTime<F> {
    /// Returns a formatter that formats the current [local time] using the
    /// [`time` crate] with the provided provided format. The format may be any
    /// type that implements the [`Formattable`] trait.
    ///
    ///
    /// <div class="example-wrap" style="display:inline-block">
    /// <pre class="compile_fail" style="white-space:normal;font:inherit;">
    ///     <strong>Warning</strong>: The <a href = "https://docs.rs/time/0.3/time/">
    ///     <code>time</code> crate</a> must be compiled with <code>--cfg
    ///     unsound_local_offset</code> in order to use local timestamps. When this
    ///     cfg is not enabled, local timestamps cannot be recorded, and
    ///     events will be logged without timestamps.
    ///
    ///    See the <a href="https://docs.rs/time/0.3.4/time/#feature-flags">
    ///    <code>time</code> documentation</a> for more details.
    /// </pre></div>
    ///
    /// Typically, the format will be a format description string, or one of the
    /// `time` crate's [well-known formats].
    ///
    /// If the format description is statically known, then the
    /// [`format_description!`] macro should be used. This is identical to the
    /// [`time::format_description::parse`] method, but runs at compile-time,
    /// throwing an error if the format description is invalid. If the desired format
    /// is not known statically (e.g., a user is providing a format string), then the
    /// [`time::format_description::parse`] method should be used. Note that this
    /// method is fallible.
    ///
    /// See the [`time` book] for details on the format description syntax.
    ///
    /// # Examples
    ///
    /// Using the [`format_description!`] macro:
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::LocalTime};
    /// use time::macros::format_description;
    ///
    /// let timer = LocalTime::new(format_description!("[hour]:[minute]:[second]"));
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// ```
    ///
    /// Using [`time::format_description::parse`]:
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::LocalTime};
    ///
    /// let time_format = time::format_description::parse("[hour]:[minute]:[second]")?;
    /// let timer = LocalTime::new(time_format);
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Using the [`format_description!`] macro requires enabling the `time`
    /// crate's "macros" feature flag.
    ///
    /// Using a [well-known format][well-known formats] (this is equivalent to
    /// [`LocalTime::rfc_3339`]):
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::LocalTime};
    ///
    /// let timer = LocalTime::new(time::format_description::well_known::Rfc3339);
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// ```
    ///
    /// [local time]: time::OffsetDateTime::now_local()
    /// [`time` crate]: time
    /// [`Formattable`]: time::formatting::Formattable
    /// [well-known formats]: time::format_description::well_known
    /// [`format_description!`]: https://docs.rs/time/0.3/time/macros/macro.format_description.html
    /// [`time::format_description::parse`]: time::format_description::parse()
    /// [`time` book]: https://time-rs.github.io/book/api/format-description.html
    pub const fn new(format: F) -> Self {
        Self { format }
    }
}

#[cfg(feature = "local-time")]
impl<F> FormatTime for LocalTime<F>
where
    F: Formattable,
{
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        let now = OffsetDateTime::now_local().map_err(|_error| fmt::Error)?;
        format_datetime(now, writer, &self.format)
    }
}

#[cfg(feature = "local-time")]
impl<F> Default for LocalTime<F>
where
    F: Formattable + Default,
{
    fn default() -> Self {
        Self::new(F::default())
    }
}

// === impl UtcTime ===

impl UtcTime<well_known::Rfc3339> {
    /// Returns a formatter that formats the current [UTC time] in the
    /// [RFC 3339] format, which is a subset of the [ISO 8601] timestamp format.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time};
    ///
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(time::UtcTime::rfc_3339());
    /// # drop(subscriber);
    /// ```
    ///
    /// [local time]: time::OffsetDateTime::now_utc
    /// [RFC 3339]: https://datatracker.ietf.org/doc/html/rfc3339
    /// [ISO 8601]: https://en.wikipedia.org/wiki/ISO_8601
    #[must_use]
    pub const fn rfc_3339() -> Self {
        Self::new(well_known::Rfc3339)
    }
}

impl<F: Formattable> UtcTime<F> {
    /// Returns a formatter that formats the current [UTC time] using the
    /// [`time` crate], with the provided provided format. The format may be any
    /// type that implements the [`Formattable`] trait.
    ///
    /// Typically, the format will be a format description string, or one of the
    /// `time` crate's [well-known formats].
    ///
    /// If the format description is statically known, then the
    /// [`format_description!`] macro should be used. This is identical to the
    /// [`time::format_description::parse`] method, but runs at compile-time,
    /// failing  an error if the format description is invalid. If the desired format
    /// is not known statically (e.g., a user is providing a format string), then the
    /// [`time::format_description::parse`] method should be used. Note that this
    /// method is fallible.
    ///
    /// See the [`time` book] for details on the format description syntax.
    ///
    /// # Examples
    ///
    /// Using the [`format_description!`] macro:
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::UtcTime};
    /// use time::macros::format_description;
    ///
    /// let timer = UtcTime::new(format_description!("[hour]:[minute]:[second]"));
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// ```
    ///
    /// Using the [`format_description!`] macro requires enabling the `time`
    /// crate's "macros" feature flag.
    ///
    /// Using [`time::format_description::parse`]:
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::UtcTime};
    ///
    /// let time_format = time::format_description::parse("[hour]:[minute]:[second]")?;
    /// let timer = UtcTime::new(time_format);
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Using a [well-known format][well-known formats] (this is equivalent to
    /// [`UtcTime::rfc_3339`]):
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::UtcTime};
    ///
    /// let timer = UtcTime::new(time::format_description::well_known::Rfc3339);
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// ```
    ///
    /// [UTC time]: time::OffsetDateTime::now_utc()
    /// [`time` crate]: time
    /// [`Formattable`]: time::formatting::Formattable
    /// [well-known formats]: time::format_description::well_known
    /// [`format_description!`]: https://docs.rs/time/0.3/time/macros/macro.format_description.html
    /// [`time::format_description::parse`]: time::format_description::parse
    /// [`time` book]: https://time-rs.github.io/book/api/format-description.html
    pub const fn new(format: F) -> Self {
        Self { format }
    }
}

impl<F> FormatTime for UtcTime<F>
where
    F: Formattable,
{
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        format_datetime(OffsetDateTime::now_utc(), writer, &self.format)
    }
}

impl<F> Default for UtcTime<F>
where
    F: Formattable + Default,
{
    fn default() -> Self {
        Self::new(F::default())
    }
}

// === impl OffsetTime ===

#[cfg(feature = "local-time")]
impl OffsetTime<well_known::Rfc3339> {
    /// Returns a formatter that formats the current time using the [local time offset] in the [RFC
    /// 3339] format (a subset of the [ISO 8601] timestamp format).
    ///
    /// Returns an error if the local time offset cannot be determined. This typically occurs in
    /// multithreaded programs. To avoid this problem, initialize `OffsetTime` before forking
    /// threads. When using Tokio, this means initializing `OffsetTime` before the Tokio runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    /// let timer = time::OffsetTime::local_rfc_3339()?;
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// # Ok(()) }
    /// ```
    ///
    /// Using `OffsetTime` with Tokio:
    ///
    /// ```
    /// use tracing_subscriber::fmt::time::OffsetTime;
    ///
    /// #[tokio::main]
    /// async fn run() {
    ///     tracing::info!("runtime initialized");
    ///
    ///     // At this point the Tokio runtime is initialized, and we can use both Tokio and Tracing
    ///     // normally.
    /// }
    ///
    /// fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    ///     // Because we need to get the local offset before Tokio spawns any threads, our `main`
    ///     // function cannot use `tokio::main`.
    ///     let timer = OffsetTime::local_rfc_3339()?;
    ///     tracing_subscriber::fmt()
    ///         .with_timer(timer)
    ///         .try_init()?;
    ///
    ///     // Even though `run` is written as an `async fn`, because we used `tokio::main` on it
    ///     // we can call it as a synchronous function.
    ///     run();
    ///     Ok(())
    /// }
    /// ```
    ///
    /// [local time offset]: time::UtcOffset::current_local_offset
    /// [RFC 3339]: https://datatracker.ietf.org/doc/html/rfc3339
    /// [ISO 8601]: https://en.wikipedia.org/wiki/ISO_8601
    ///
    /// # Errors
    ///
    /// Returns an error if the local UTC offset cannot be determined.
    pub fn local_rfc_3339() -> Result<Self, IndeterminateOffset> {
        Ok(Self {
            offset: UtcOffset::current_local_offset()?,
            format: well_known::Rfc3339,
        })
    }
}

impl<F: Formattable> OffsetTime<F> {
    /// Returns a formatter that formats the current time using the [`time` crate] with the provided
    /// provided format and [timezone offset]. The format may be any type that implements the
    /// [`Formattable`] trait.
    ///
    ///
    /// Typically, the offset will be the [local offset], and format will be a format description
    /// string, or one of the `time` crate's [well-known formats].
    ///
    /// If the format description is statically known, then the
    /// [`format_description!`] macro should be used. This is identical to the
    /// [`time::format_description::parse`] method, but runs at compile-time,
    /// throwing an error if the format description is invalid. If the desired format
    /// is not known statically (e.g., a user is providing a format string), then the
    /// [`time::format_description::parse`] method should be used. Note that this
    /// method is fallible.
    ///
    /// See the [`time` book] for details on the format description syntax.
    ///
    /// # Examples
    ///
    /// Using the [`format_description!`] macro:
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::OffsetTime};
    /// use time::macros::format_description;
    /// use time::UtcOffset;
    ///
    /// let offset = UtcOffset::current_local_offset()?;
    /// let timer = OffsetTime::new(offset, format_description!("[hour]:[minute]:[second]"));
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Using [`time::format_description::parse`]:
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::OffsetTime};
    /// use time::UtcOffset;
    ///
    /// let offset = UtcOffset::current_local_offset()?;
    /// let time_format = time::format_description::parse("[hour]:[minute]:[second]")?;
    /// let timer = OffsetTime::new(offset, time_format);
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Using the [`format_description!`] macro requires enabling the `time`
    /// crate's "macros" feature flag.
    ///
    /// Using a [well-known format][well-known formats] (this is equivalent to
    /// [`OffsetTime::local_rfc_3339`]):
    ///
    /// ```
    /// use tracing_subscriber::fmt::{self, time::OffsetTime};
    /// use time::UtcOffset;
    ///
    /// let offset = UtcOffset::current_local_offset()?;
    /// let timer = OffsetTime::new(offset, time::format_description::well_known::Rfc3339);
    /// let subscriber = tracing_subscriber::fmt()
    ///     .with_timer(timer);
    /// # drop(subscriber);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// [`time` crate]: time
    /// [timezone offset]: time::UtcOffset
    /// [`Formattable`]: time::formatting::Formattable
    /// [local offset]: time::UtcOffset::current_local_offset()
    /// [well-known formats]: time::format_description::well_known
    /// [`format_description!`]: https://docs.rs/time/0.3/time/macros/macro.format_description.html
    /// [`time::format_description::parse`]: time::format_description::parse
    /// [`time` book]: https://time-rs.github.io/book/api/format-description.html
    pub const fn new(offset: UtcOffset, format: F) -> Self {
        Self { offset, format }
    }
}

impl<F> FormatTime for OffsetTime<F>
where
    F: Formattable,
{
    fn format_time(&self, writer: &mut Writer<'_>) -> fmt::Result {
        let now = OffsetDateTime::now_utc().to_offset(self.offset);
        format_datetime(now, writer, &self.format)
    }
}

/// Formats an [`OffsetDateTime`] into the tracing formatter.
///
/// # Errors
///
/// Returns [`fmt::Error`] if the configured time formatter cannot write the timestamp.
fn format_datetime(
    now: OffsetDateTime,
    destination: &mut Writer<'_>,
    formatter: &(impl Formattable + ?Sized),
) -> fmt::Result {
    let mut writer = WriteAdaptor::new(destination);
    now.format_into(&mut writer, formatter)
        .map_err(|_error| fmt::Error)
        .map(|_| ())
}

#[cfg(test)]
#[cfg(feature = "time")]
mod tests {
    use std::fmt;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    use strict_test_support::TestFailure;
    use strict_test_support::ensure;
    use strict_test_support::ensure_contains;
    use strict_test_support::ensure_eq;
    use strict_test_support::ensure_ok;
    use time::format_description::BorrowedFormatItem;
    use time::macros::format_description;

    use super::*;

    #[derive(Debug)]
    struct FailingWriter;

    impl fmt::Write for FailingWriter {
        fn write_str(&mut self, _: &str) -> fmt::Result {
            Err(fmt::Error)
        }
    }

    fn render_time(timer: &impl FormatTime) -> Result<String, TestFailure> {
        let mut output = String::new();
        let mut writer = Writer::new(&mut output);
        ensure(
            timer.format_time(&mut writer).is_ok(),
            "timer should format into the writer",
        )?;
        Ok(output)
    }

    fn offset(hours: i8, minutes: i8) -> Result<UtcOffset, TestFailure> {
        ensure_ok(
            UtcOffset::from_hms(hours, minutes, 0),
            "UTC offset should be valid",
        )
    }

    #[test]
    fn utc_time_rfc3339_writes_timestamp() -> Result<(), TestFailure> {
        let output = render_time(&UtcTime::rfc_3339())?;
        let second_output = render_time(&UtcTime::rfc_3339())?;

        ensure_contains(&output, "T", "RFC3339 UTC timestamp includes date-time separator")?;
        ensure_contains(&output, "Z", "RFC3339 UTC timestamp includes UTC suffix")?;
        ensure_contains(
            &second_output,
            "T",
            "reconstructed RFC3339 UTC formatter writes timestamp",
        )
    }

    #[test]
    fn utc_time_custom_format_writes_expected_shape() -> Result<(), TestFailure> {
        let format = format_description!("[hour]:[minute]:[second]");
        let output = render_time(&UtcTime::new(format))?;

        ensure_eq(&output.len(), &8_usize, "custom UTC format length")?;
        ensure_contains(&output, ":", "custom UTC format contains separators")
    }

    #[test]
    fn offset_time_applies_fixed_offset() -> Result<(), TestFailure> {
        let timer = OffsetTime::new(offset(5, 30)?, well_known::Rfc3339);
        let output = render_time(&timer)?;

        ensure_contains(&output, "+05:30", "fixed offset is rendered in RFC3339 output")
    }

    #[test]
    fn offset_time_custom_format_writes_expected_shape() -> Result<(), TestFailure> {
        let format = format_description!("[offset_hour sign:mandatory]:[offset_minute]");
        let timer = OffsetTime::new(offset(-4, -30)?, format);
        let output = render_time(&timer)?;

        ensure_eq(&output, &String::from("-04:30"), "custom offset format uses fixed offset")
    }

    #[test]
    fn format_datetime_writes_fixed_offset_timestamp() -> Result<(), TestFailure> {
        let format = format_description!(
            "[year]-[month]-[day] [hour]:[minute] [offset_hour sign:mandatory]:[offset_minute]"
        );
        let timestamp = OffsetDateTime::UNIX_EPOCH.to_offset(offset(2, 0)?);
        let mut output = String::new();
        let mut writer = Writer::new(&mut output);

        ensure(
            format_datetime(timestamp, &mut writer, format).is_ok(),
            "format_datetime should write deterministic timestamp",
        )?;
        ensure_eq(
            &output,
            &String::from("1970-01-01 02:00 +02:00"),
            "fixed timestamp output",
        )
    }

    #[test]
    fn format_datetime_returns_error_when_writer_fails() -> Result<(), TestFailure> {
        let format = format_description!("[hour]");
        let timestamp = OffsetDateTime::UNIX_EPOCH;
        let mut output = FailingWriter;
        let mut writer = Writer::new(&mut output);

        ensure(
            format_datetime(timestamp, &mut writer, format).is_err(),
            "format_datetime returns fmt error from failing writer",
        )
    }

    #[test]
    fn default_utc_time_uses_default_formatter() -> Result<(), TestFailure> {
        let timer = UtcTime::<Vec<BorrowedFormatItem<'static>>>::default();
        let output = render_time(&timer)?;

        ensure_eq(&output, &String::new(), "default UTC formatter uses F::default")
    }

    #[test]
    #[cfg(feature = "local-time")]
    fn local_time_new_constructs_formatter() -> Result<(), TestFailure> {
        let timer = LocalTime::new(format_description!("[hour]:[minute]"));
        let output = render_time(&timer)?;

        ensure_eq(&output.len(), &5_usize, "local time custom format length")?;
        ensure_contains(&output, ":", "local time custom format contains separator")
    }

    #[test]
    #[cfg(feature = "local-time")]
    fn local_rfc3339_returns_current_offset_or_indeterminate_offset_without_panicking(
    ) -> Result<(), TestFailure> {
        let first = OffsetTime::local_rfc_3339();
        let second = OffsetTime::local_rfc_3339();
        ensure_eq(
            &first.is_ok(),
            &second.is_ok(),
            "local offset availability is stable across adjacent calls",
        )?;

        match first {
            Ok(timer) => {
                let output = render_time(&timer)?;
                ensure_contains(&output, "T", "local offset timer writes RFC3339 timestamp")
            }
            Err(error) => ensure(
                !format!("{error}").is_empty(),
                "indeterminate local offset reports a displayable error",
            ),
        }
    }
}
