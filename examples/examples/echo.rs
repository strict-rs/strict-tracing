//! A "hello world" echo server [from Tokio][echo-example]
//!
//! This server will create a TCP listener, accept connections in a loop, and
//! write back everything that's read off of each TCP connection.
//!
//! Because the Tokio runtime uses a thread pool, each TCP connection is
//! processed concurrently with all other TCP connections across multiple
//! threads.
//!
//! To see this server in action, you can run this in one terminal:
//!
//!     cargo +nightly run --example echo
//!
//! and in another terminal you can run:
//!
//!     nc localhost 3000
//!
//! Each line you type in to the `netcat` terminal should be echo'd back to
//! you! If you open up multiple terminals with `netcat` instances connected
//! to the same address you should be able to see them all make progress simultaneously.
//!
//! [echo-example]: https://github.com/tokio-rs/tokio/blob/master/tokio/examples/echo.rs
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::net::SocketAddr;

use tokio::io::AsyncReadExt as _;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpListener;
use tracing::Instrument as _;
use tracing::debug;
use tracing::info;
use tracing::info_span;
use tracing::trace_span;
use tracing::warn;

/// Error type returned by the example.
type Error = Box<dyn StdError + Send + Sync + 'static>;

/// Error returned when a socket read reports more bytes than the buffer holds.
#[derive(Debug)]
struct BufferLengthError {
  /// Number of bytes reported by the socket read.
  bytes_read: usize,
  /// Length of the buffer passed to the socket read.
  buffer_len: usize,
}

impl fmt::Display for BufferLengthError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    let bytes_read = self.bytes_read;
    let buffer_len = self.buffer_len;
    write!(formatter, "socket read reported {bytes_read} bytes for a {buffer_len} byte buffer")
  }
}

impl StdError for BufferLengthError {}

#[tokio::main]
async fn main() -> Result<(), Error> {
  use tracing_subscriber::EnvFilter;

  tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env().add_directive("echo=trace".parse()?))
    .try_init()?;

  // Allow passing an address to listen on as the first argument of this
  // program, but otherwise we'll just set up our TCP listener on
  // 127.0.0.1:8080 for connections.
  let addr_arg = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:3000".to_owned());
  let addr = addr_arg.parse::<SocketAddr>()?;

  // Next up we create a TCP listener which will listen for incoming
  // connections. This TCP listener is bound to the address we determined
  // above and must be associated with an event loop.
  let listener = TcpListener::bind(&addr).await?;
  // Use `fmt::Debug` impl for `addr` using the `%` symbol
  info!(message = "Listening on", %addr);

  loop {
    // Asynchronously wait for an inbound socket.
    let (mut socket, peer_addr) = listener.accept().await?;

    info!(message = "Got connection from", %peer_addr);

    // And this is where much of the magic of this server happens. We
    // crucially want all clients to make progress concurrently, rather than
    // blocking one on completion of another. To achieve this we use the
    // `tokio::spawn` function to execute the work in the background.
    //
    // Essentially here we're executing a new task to run concurrently,
    // which will allow all of our clients to be processed concurrently.

    tokio::spawn(async move {
      let mut buf = [0; 1024];

      // In a loop, read data from the socket and write the data back.
      loop {
        let n = socket
          .read(&mut buf)
          .instrument(trace_span!("read"))
          .await
          .inspect(|bytes_read| {
            debug!(bytes_read = *bytes_read);
          })
          .inspect_err(|error| {
            warn!(%error);
          })?;

        if n == 0 {
          return Ok::<(), Error>(());
        }

        let bytes = buf.get(..n).ok_or(BufferLengthError {
          bytes_read: n,
          buffer_len: buf.len(),
        })?;

        socket
          .write_all(bytes)
          .instrument(trace_span!("write"))
          .await
          .inspect_err(|error| {
            warn!(%error);
          })?;
        debug!(bytes_written = n);

        info!(message = "echo'd data", %peer_addr, size = n);
      }
    })
    .instrument(info_span!("echo", %peer_addr))
    .await??;
  }
}
