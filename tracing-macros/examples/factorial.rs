//! Compare to the example given in the documentation for the `std::dbg` macro.
#![deny(rust_2018_idioms)]

use tracing::{
    Level,
    subscriber::{self, SetGlobalDefaultError},
};
use tracing_macros::dbg;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt as _};

/// Calculate the factorial of `n` while tracing each recursive step.
fn factorial(n: u32) -> u32 {
    if dbg!(n <= 1) {
        dbg!(1)
    } else {
        dbg!(n.saturating_mul(factorial(n.saturating_sub(1))))
    }
}

fn main() -> Result<(), SetGlobalDefaultError> {
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(Level::TRACE.into()))
        .with(fmt::Layer::new());

    subscriber::set_global_default(subscriber)?;
    dbg!(factorial(4));
    Ok(())
}
