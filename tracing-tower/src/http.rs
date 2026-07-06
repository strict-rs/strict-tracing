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
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;
  use tracing::Level;

  use super::debug_request;
  use super::error_request;
  use super::info_request;
  use super::trace_request;
  use super::warn_request;

  fn request_with_body<A>(body: A) -> Result<http::Request<A>, TestFailure> {
    ensure_ok(
      http::Request::builder()
        .method("PATCH")
        .uri("/direct")
        .version(http::Version::HTTP_2)
        .header("x-direct", "true")
        .body(body),
      "test HTTP request builds",
    )
  }

  fn ensure_request_metadata(
    span: &tracing::Span,
    level: Level,
    has_version: bool,
    has_headers: bool,
    context: &'static str,
  ) -> Result<(), TestFailure> {
    let metadata = ensure_some(span.metadata(), "request constructor returns a metadata-bearing span")?;
    let fields = metadata.fields();

    ensure(metadata.name() == "request", context)?;
    ensure(*metadata.level() == level, "request span level matches constructor")?;
    ensure(fields.field("method").is_some(), "request span records method")?;
    ensure(fields.field("uri").is_some(), "request span records uri")?;
    ensure(
      fields.field("version").is_some() == has_version,
      "request span version field polarity matches constructor",
    )?;
    ensure(
      fields.field("headers").is_some() == has_headers,
      "request span headers field polarity matches constructor",
    )
  }

  #[test]
  fn level_constructors_retain_metadata_without_subscriber() -> Result<(), TestFailure> {
    let request = request_with_body(())?;

    ensure_request_metadata(&info_request(&request), Level::INFO, false, false, "info request span metadata")?;
    ensure_request_metadata(&warn_request(&request), Level::WARN, false, false, "warn request span metadata")?;
    ensure_request_metadata(&error_request(&request), Level::ERROR, false, false, "error request span metadata")?;
    ensure_request_metadata(&debug_request(&request), Level::DEBUG, true, false, "debug request span metadata")?;
    ensure_request_metadata(&trace_request(&request), Level::TRACE, true, true, "trace request span metadata")
  }

  #[test]
  fn request_constructors_accept_different_body_types() -> Result<(), TestFailure> {
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
