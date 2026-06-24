#![deny(rust_2018_idioms)]

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use std::str;
use tokio::net::TcpListener;
use tracing::{Instrument as _, Level, debug, info, span};

async fn echo(req: Request<Incoming>) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let span = span!(
        Level::INFO,
        "request",
        method = ?req.method(),
        uri = ?req.uri(),
        headers = ?req.headers()
    );
    let _enter = span.enter();
    info!("received request");
    let mut response = Response::new(Full::new(Bytes::new()));

    let (rsp_span, resp) = match (req.method(), req.uri().path()) {
        // Serve some instructions at /
        (&Method::GET, "/") => {
            const BODY: &str = "Try POSTing data to /echo";
            *response.body_mut() = Full::new(Bytes::from_static(BODY.as_bytes()));
            (span!(Level::INFO, "response", body = %(&BODY)), response)
        }

        // Simply echo the body back to the client.
        (&Method::POST, "/echo") => {
            let span = span!(Level::INFO, "response", response_kind = %"echo");
            let body = req.into_body().collect().await?.to_bytes();
            *response.body_mut() = Full::new(body);
            (span, response)
        }

        // Convert to uppercase before sending back to client.
        (&Method::POST, "/echo/uppercase") => {
            let body = req.into_body().collect().await?.to_bytes();
            let upper = body
                .iter()
                .map(|byte| byte.to_ascii_uppercase())
                .collect::<Vec<u8>>();
            debug!(
                body = ?str::from_utf8(&body[..]),
                uppercased = ?str::from_utf8(&upper[..]),
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
            let span = span!(Level::TRACE, "response", response_kind = %"reversed");
            let _enter = span.enter();
            let body = req.into_body().collect().await?.to_bytes();
            let reversed = body.iter().rev().cloned().collect::<Vec<u8>>();
            debug!(
                body = ?str::from_utf8(&body[..]),
                "reversed request body"
            );
            *response.body_mut() = Full::new(Bytes::from(reversed));
            (
                span!(Level::INFO, "reversed", body = ?(&response.body())),
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
    let f = async { resp }.instrument(rsp_span);
    Ok(f.await)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();

    let local_addr: std::net::SocketAddr = ([127, 0, 0, 1], 3000).into();
    let server_span = span!(Level::TRACE, "server", %local_addr);
    let _enter = server_span.enter();

    let listener = TcpListener::bind(local_addr).await?;
    info!("listening...");

    loop {
        let (stream, _peer_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let conn_span = server_span.clone();
        tokio::spawn(
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
