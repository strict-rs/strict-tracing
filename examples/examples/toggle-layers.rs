//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
use std::error::Error;

/// This is a example showing how `Layer` can be enabled or disabled by
/// by wrapping them with an `Option`. This example shows `fmt` and `json`
/// being toggled based on the `json` command line flag.
///
/// You can run this example by running the following command in a terminal
///
/// ```
/// cargo run --example toggle-subscribers -- --json
/// ```
use argh::FromArgs;
use tracing::info;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

/// Shared yak-shaving helper used by this example.
#[path = "fmt/yak_shave.rs"]
pub mod yak_shave;

#[derive(FromArgs)]
/// Subscriber toggling example.
struct Args {
  /// enable JSON log format
  #[argh(switch, short = 'j')]
  json: bool,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  let args: Args = argh::from_env();

  let (json, plain) = if args.json {
    (Some(fmt::layer().json()), None)
  } else {
    (None, Some(fmt::layer()))
  };

  tracing_subscriber::registry().with(json).with(plain).try_init()?;

  let number_of_yaks = 3;
  // this creates a new event, outside of any spans.
  info!(number_of_yaks, "preparing to shave yaks");

  let number_shaved = yak_shave::shave_all(number_of_yaks);
  info!(all_yaks_shaved = number_shaved == number_of_yaks, "yak shaving completed.");

  Ok(())
}
