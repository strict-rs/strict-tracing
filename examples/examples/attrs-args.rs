//! Example binary for tracing workspace checks.
#![deny(rust_2018_idioms)]

use tracing::debug;
use tracing::info;
use tracing::subscriber::with_default;
use tracing_attributes::instrument;

/// Return the `n`th Fibonacci number while emitting recursive trace spans.
#[instrument]
fn nth_fibonacci(n: u64) -> u64 {
  if n == 0 || n == 1 {
    debug!("Base case");
    1
  } else {
    debug!("Recursing");
    let previous = n.saturating_sub(1);
    let before_previous = n.saturating_sub(2);

    nth_fibonacci(previous).saturating_add(nth_fibonacci(before_previous))
  }
}

/// Build the Fibonacci sequence from zero through the requested index.
#[allow(
  clippy::single_call_fn,
  reason = "keeps the sequence-building span visible alongside the recursive spans"
)]
#[instrument]
fn fibonacci_seq(to: u64) -> Vec<u64> {
  let mut sequence = vec![];

  for n in 0..=to {
    debug!("Pushing {n} fibonacci", n = n);
    sequence.push(nth_fibonacci(n));
  }

  sequence
}

fn main() {
  let subscriber = tracing_subscriber::fmt().with_env_filter("attrs_args=trace").finish();

  with_default(subscriber, || {
    let n = 5;
    let sequence = fibonacci_seq(n);
    info!("The first {} fibonacci numbers are {:?}", n, sequence);
  });
}
