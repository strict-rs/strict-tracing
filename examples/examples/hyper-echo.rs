//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt as _, Full};
use hyper::{Error as HyperError, body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use std::error::Error as StdError;
use std::net::SocketAddr;
use std::str;
use tokio::net::TcpListener;
use tracing::{Instrument as _, Level, debug, info, span};

/// Error type returned by the HTTP echo example.
type Error = Box<dyn StdError + Send + Sync + 'static>;

/// Handle an HTTP request by echoing, uppercasing, or reversing the request body.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the `service_fn` request handler named around the route demo"
)]
async fn echo(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, HyperError> {
    let request_span = span!(
        Level::INFO,
        "request",
        method = ?req.method(),
        uri = ?req.uri(),
        headers = ?req.headers()
    );
    let _request_guard = request_span.enter();
    info!("received request");
    let mut response = Response::new(Full::new(Bytes::new()));

    let (response_span, final_response) = match (req.method(), req.uri().path()) {
        // Serve some instructions at /
        (&Method::GET, "/") => {
            /// Body returned by the root route.
            const BODY: &str = "Try POSTing data to /echo";
            *response.body_mut() = Full::new(Bytes::from_static(BODY.as_bytes()));
            (span!(Level::INFO, "response", body = %BODY), response)
        }

        // Simply echo the body back to the client.
        (&Method::POST, "/echo") => {
            let response_span = span!(Level::INFO, "response", response_kind = %"echo");
            let body = req.into_body().collect().await?.to_bytes();
            *response.body_mut() = Full::new(body);
            (response_span, response)
        }

        // Convert to uppercase before sending back to client.
        (&Method::POST, "/echo/uppercase") => {
            let body = req.into_body().collect().await?.to_bytes();
            let upper = body.iter().map(u8::to_ascii_uppercase).collect::<Vec<u8>>();
            debug!(
                body = ?str::from_utf8(body.as_ref()),
                uppercased = ?str::from_utf8(upper.as_slice()),
                "uppercased request body"
            );

            *response.body_mut() = Full::new(Bytes::from(upper));
            (
                span!(Level::INFO, "response", response_kind = %"uppercase"),
                response,
            )
        }

        // Reverse the entire body before sending back to the client.
        (&Method::POST, "/echo/reversed") => {
            let trace_span = span!(Level::TRACE, "response", response_kind = %"reversed");
            let _response_guard = trace_span.enter();
            let body = req.into_body().collect().await?.to_bytes();
            let reversed = body.iter().rev().copied().collect::<Vec<u8>>();
            debug!(
                body = ?str::from_utf8(body.as_ref()),
                "reversed request body"
            );
            *response.body_mut() = Full::new(Bytes::from(reversed));
            (
                span!(Level::INFO, "reversed", body = ?response.body()),
                response,
            )
        }

        // The 404 Not Found route...
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
            (
                span!(
                    Level::TRACE,
                    "response",
                    body = ?(),
                    status = ?StatusCode::NOT_FOUND,
                ),
                response,
            )
        }
    };
    let instrumented_response = async { final_response }.instrument(response_span);
    Ok(instrumented_response.await)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .try_init()?;

    let local_addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    let server_span = span!(Level::TRACE, "server", %local_addr);
    let _enter = server_span.enter();

    let listener = TcpListener::bind(local_addr).await?;
    info!("listening...");

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let conn_span = server_span.clone();
        let _task = tokio::spawn(
            async move {
                if let Err(err) = auto::Builder::new(TokioExecutor::new())
                    .serve_connection(io, service_fn(echo))
                    .await
                {
                    debug!(error = %err, "connection error");
                }
            }
            .instrument(conn_span),
        );
    }
}
