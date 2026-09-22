/// Defines request span constructor functions for HTTP request metadata.
macro_rules! make_req_fns {
    ($($name:ident, $level:expr),+) => {
        $(
            #[doc = concat!("Creates a request span at `", stringify!($level), "` level.")]
            #[inline]
            pub fn $name<A>(req: &http::Request<A>) -> tracing::Span {
                tracing::span!(
                    $level,
                    "request",
                    method = ?req.method(),
                    uri = ?req.uri(),
                )
            }
        )+
    }
}

make_req_fns! {
    info_request, tracing::Level::INFO,
    warn_request, tracing::Level::WARN,
    error_request, tracing::Level::ERROR
}

#[inline]
/// Creates a debug-level request span including the request version.
pub fn debug_request<A>(req: &http::Request<A>) -> tracing::Span {
  tracing::span!(
      tracing::Level::DEBUG,
      "request",
      method = ?req.method(),
      uri = ?req.uri(),
      version = ?req.version(),
  )
}

#[inline]
/// Creates a trace-level request span including the request headers.
pub fn trace_request<A>(req: &http::Request<A>) -> tracing::Span {
  tracing::span!(
      tracing::Level::TRACE,
      "request",
      method = ?req.method(),
      uri = ?req.uri(),
      version = ?req.version(),
      headers = ?req.headers(),
  )
}

#[cfg(test)]
mod tests {

  /// Native failures from these behavioral checks.
  #[derive(Debug, thiserror::Error)]
  enum TestError {
    /// A boolean expectation failed.
    #[error(transparent)]
    Condition(#[from] strict_test_support::ConditionFailure),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    ResultHttpError(#[from] strict_test_support::ResultFailure<http::Error>),
    /// Preserves the complete native failure and its inputs.
    #[error(transparent)]
    OptionStaticTracingMetadata(#[from] strict_test_support::OptionFailure<&'static tracing::Metadata<'static>>),
  }

  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::Level;

  use super::debug_request;
  use super::error_request;
  use super::info_request;
  use super::trace_request;
  use super::warn_request;

  fn request_with_body<A>(body: A) -> Result<http::Request<A>, TestError> {
    ensure_ok(
      http::Request::builder()
        .method("PATCH")
        .uri("/direct")
        .version(http::Version::HTTP_2)
        .header("x-direct", "true")
        .body(body),
      "test HTTP request builds",
    )
    .map_err(TestError::from)
  }

  fn ensure_request_metadata(
    span: &tracing::Span,
    level: Level,
    has_version: bool,
    has_headers: bool,
    context: &'static str,
  ) -> Result<(), TestError> {
    let metadata = ensure_some(span.metadata(), "request constructor returns a metadata-bearing span")?;
    let fields = metadata.fields();

    ensure(metadata.name() == "request", context).map(drop)?;
    ensure(*metadata.level() == level, "request span level matches constructor").map(drop)?;
    ensure(fields.field("method").is_some(), "request span records method").map(drop)?;
    ensure(fields.field("uri").is_some(), "request span records uri").map(drop)?;
    ensure(
      fields.field("version").is_some() == has_version,
      "request span version field polarity matches constructor",
    )
    .map(drop)?;
    ensure(
      fields.field("headers").is_some() == has_headers,
      "request span headers field polarity matches constructor",
    )
    .map(drop)
    .map_err(TestError::from)
  }

  #[test]
  fn level_constructors_retain_metadata_without_subscriber() -> Result<(), TestError> {
    let request = request_with_body(())?;

    ensure_request_metadata(&info_request(&request), Level::INFO, false, false, "info request span metadata")?;
    ensure_request_metadata(&warn_request(&request), Level::WARN, false, false, "warn request span metadata")?;
    ensure_request_metadata(&error_request(&request), Level::ERROR, false, false, "error request span metadata")?;
    ensure_request_metadata(&debug_request(&request), Level::DEBUG, true, false, "debug request span metadata")?;
    ensure_request_metadata(&trace_request(&request), Level::TRACE, true, true, "trace request span metadata")
  }

  #[test]
  fn request_constructors_accept_different_body_types() -> Result<(), TestError> {
    let string_request = request_with_body(String::from("body"))?;
    let bytes_request = request_with_body([1_u8, 2_u8, 3_u8])?;

    ensure_request_metadata(
      &info_request(&string_request),
      Level::INFO,
      false,
      false,
      "info request span accepts string bodies",
    )?;
    ensure_request_metadata(
      &warn_request(&string_request),
      Level::WARN,
      false,
      false,
      "warn request span accepts string bodies",
    )?;
    ensure_request_metadata(
      &error_request(&bytes_request),
      Level::ERROR,
      false,
      false,
      "error request span accepts byte-array bodies",
    )?;
    ensure_request_metadata(
      &debug_request(&bytes_request),
      Level::DEBUG,
      true,
      false,
      "debug request span accepts byte-array bodies",
    )?;
    ensure_request_metadata(
      &trace_request(&bytes_request),
      Level::TRACE,
      true,
      true,
      "trace request span accepts byte-array bodies",
    )
  }
}
