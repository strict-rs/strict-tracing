//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]

use tracing::Level;
use tracing::debug;
use tracing::span;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;

/// Return a sample band recommendation while recording an instrumented span.
#[allow(
  clippy::single_call_fn,
  reason = "the named function is the minimal instrumented span demonstrated by this example"
)]
#[instrument]
#[inline]
fn suggest_band() -> String {
  debug!("Suggesting a band.");
  String::from("Wild Pink")
}

fn main() {
  let subscriber = tracing_subscriber::fmt()
    .with_env_filter("attrs_literal_field_names=trace")
    .finish();
  with_default(subscriber, || {
    let span = span!(Level::TRACE, "get_band_rec", "guid:x-request-id" = "abcdef");
    let _enter = span.enter();
    let _band = suggest_band();
  });
}
