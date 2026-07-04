//! Example binary for tracing workspace checks.
//! This example shows how a field value may be recorded using the `valuable`
//! crate (<https://crates.io/crates/valuable>).
//!
//! `valuable` provides a lightweight but flexible way to record structured data, allowing
//! visitors to extract individual fields or elements of structs, maps, arrays, and other
//! nested structures.
//!
//! `tracing`'s support for `valuable` is currently feature flagged. Additionally, `valuable`
//! support is considered an *unstable feature*: in order to use `valuable` with `tracing`,
//! the project must be built with `RUSTFLAGS="--cfg tracing_unstable"`.
//!
//! Therefore, when `valuable` support is not enabled, this example falls back to using
//! `fmt::Debug` to record fields that implement `valuable::Valuable`.
use std::error::Error;

#[cfg(tracing_unstable)]
use tracing::field::valuable;
use valuable::Valuable;

/// Example user data recorded as a structured `valuable` field.
#[derive(Copy, Clone, Debug, Valuable)]
struct User {
  /// User display name.
  name:    &'static str,
  /// User age in years.
  age:     u32,
  /// User mailing address.
  address: Address,
}

/// Example address nested inside the recorded user data.
#[derive(Copy, Clone, Debug, Valuable)]
struct Address {
  /// Address country.
  country: &'static str,
  /// Address city.
  city:    &'static str,
  /// Address street.
  street:  &'static str,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing_subscriber::fmt()
    .with_max_level(tracing::Level::TRACE)
    .json()
    .try_init()?;

  let user = User {
    name:    "Arwen Undomiel",
    age:     3000,
    address: Address {
      country: "Middle Earth",
      city:    "Rivendell",
      street:  "leafy lane",
    },
  };

  // for comparison, record `user` without using its `Valuable`
  // implementation:
  tracing::info!(valuable = false, user = ?user);

  // If the `valuable` feature is enabled, record `user` using its'
  // `valuable::Valuable` implementation:
  #[cfg(tracing_unstable)]
  tracing::info!(valuable = true, user = valuable(&user));

  #[cfg(not(tracing_unstable))]
  tracing::warn!(
    "note: this example was run without `valuable` support enabled!\nrerun with `RUSTFLAGS=\"--cfg tracing_unstable\" to enable `valuable`",
  );

  Ok(())
}
