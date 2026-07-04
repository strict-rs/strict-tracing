//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]
#[path = "fmt/yak_shave.rs"]
pub mod yak_shave;

use tracing::debug;
use tracing::subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;

fn main() {
  let subscriber = fmt::Subscriber::builder()
    .with_env_filter(EnvFilter::from_default_env())
    .finish();

  subscriber::with_default(subscriber, || {
    let number_of_yaks = 3;
    debug!("preparing to shave {number_of_yaks} yaks");

    let number_shaved = yak_shave::shave_all(number_of_yaks);

    debug!(
      message = "yak shaving completed.",
      all_yaks_shaved = number_shaved == number_of_yaks,
    );
  });
}
