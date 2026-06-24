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
